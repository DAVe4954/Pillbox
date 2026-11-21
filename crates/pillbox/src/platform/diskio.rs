//! Disk activity for the System card: how busy each physical disk is and how fast it reads and
//! writes, from the `PhysicalDisk` performance counters (PDH), the numbers Task Manager's
//! Performance tab shows as "Active time" and "Transfer rate".
//!
//! "Active time" is 100 minus `% Idle Time`. Instances are named `<disk number> <drive letters>`
//! ("0 C:", "1 D: E:") plus `_Total`, which is skipped. The values are rates, so the first
//! sample after `new` is the priming read and the list is empty until the second one.

use pillbox_core::sysinfo::{DiskActivity, disk_name};
use windows::{
    Win32::System::Performance::{
        PDH_FMT, PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_DOUBLE, PDH_HCOUNTER, PDH_HQUERY,
        PDH_MORE_DATA, PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData,
        PdhGetFormattedCounterArrayW, PdhOpenQueryW,
    },
    core::w,
};

pub struct DiskMonitor {
    query: PDH_HQUERY,
    idle: PDH_HCOUNTER,
    read: PDH_HCOUNTER,
    write: PDH_HCOUNTER,
    buf: Vec<u64>,
}

impl DiskMonitor {
    /// `None` when the counters are not available (they can be disabled or corrupt).
    pub fn new() -> Option<Self> {
        let mut query = PDH_HQUERY::default();
        // SAFETY: out-pointer to a local handle; closed in Drop.
        if unsafe { PdhOpenQueryW(None, 0, &mut query) } != 0 {
            tracing::warn!("disk activity: PdhOpenQuery failed");
            return None;
        }
        let (mut idle, mut read, mut write) = (
            PDH_HCOUNTER::default(),
            PDH_HCOUNTER::default(),
            PDH_HCOUNTER::default(),
        );
        // English paths, so they work whatever the Windows display language.
        // SAFETY: valid query handle, 'static paths, out-pointers to locals.
        let (a, b, c) = unsafe {
            (
                PdhAddEnglishCounterW(query, w!("\\PhysicalDisk(*)\\% Idle Time"), 0, &mut idle),
                PdhAddEnglishCounterW(
                    query,
                    w!("\\PhysicalDisk(*)\\Disk Read Bytes/sec"),
                    0,
                    &mut read,
                ),
                PdhAddEnglishCounterW(
                    query,
                    w!("\\PhysicalDisk(*)\\Disk Write Bytes/sec"),
                    0,
                    &mut write,
                ),
            )
        };
        if a != 0 || b != 0 || c != 0 {
            tracing::warn!("disk activity counters are not available ({a:#x}, {b:#x}, {c:#x})");
            // SAFETY: the handle came from PdhOpenQueryW and is closed once.
            unsafe { PdhCloseQuery(query) };
            return None;
        }
        // The priming read: rate counters have no value until a second collection.
        // SAFETY: valid query handle.
        unsafe { PdhCollectQueryData(query) };
        Some(Self {
            query,
            idle,
            read,
            write,
            buf: Vec::new(),
        })
    }

    /// Activity of every physical disk since the previous call. Empty if nothing could be read.
    pub fn sample(&mut self) -> Vec<DiskActivity> {
        // SAFETY: valid query handle.
        if unsafe { PdhCollectQueryData(self.query) } != 0 {
            return Vec::new();
        }
        let idle = read_array(self.idle, &mut self.buf);
        let reads = read_array(self.read, &mut self.buf);
        let writes = read_array(self.write, &mut self.buf);
        let of = |list: &[(String, f64)], instance: &str| {
            list.iter()
                .find(|(i, _)| i == instance)
                .map_or(0.0, |(_, v)| v.max(0.0))
        };
        let mut list: Vec<(String, DiskActivity)> = idle
            .into_iter()
            .filter_map(|(instance, idle)| {
                let name = disk_name(&instance)?;
                let (read_bps, write_bps) = (of(&reads, &instance), of(&writes, &instance));
                Some((
                    instance,
                    DiskActivity {
                        name,
                        active: (100.0 - idle).clamp(0.0, 100.0) as f32,
                        read_bps,
                        write_bps,
                    },
                ))
            })
            .collect();
        // Disk 0 first (the system disk), as `busiest_disk` expects for the all-idle case.
        list.sort_by(|a, b| a.0.cmp(&b.0));
        list.into_iter().map(|(_, d)| d).collect()
    }
}

impl Drop for DiskMonitor {
    fn drop(&mut self) {
        // SAFETY: the handle came from PdhOpenQueryW and is closed once.
        unsafe { PdhCloseQuery(self.query) };
    }
}

/// Every instance of a counter as (instance name, value). Empty on any failure.
fn read_array(counter: PDH_HCOUNTER, buf: &mut Vec<u64>) -> Vec<(String, f64)> {
    // PDH_FMT_NOCAP100 (0x8000, not in the bindings): do not clip the value at 100.
    let fmt = PDH_FMT(PDH_FMT_DOUBLE.0 | 0x8000);
    let (mut bytes, mut count) = (0u32, 0u32);
    // SAFETY: a size query (no buffer); then the buffer is sized to what PDH asked for (as u64s,
    // so it is aligned for the item structs) and read only within `count` items. The name
    // pointers point into that same buffer.
    unsafe {
        let status = PdhGetFormattedCounterArrayW(counter, fmt, &mut bytes, &mut count, None);
        if status != PDH_MORE_DATA || bytes == 0 {
            return Vec::new();
        }
        buf.clear();
        buf.resize((bytes as usize).div_ceil(8), 0);
        let items = buf.as_mut_ptr().cast::<PDH_FMT_COUNTERVALUE_ITEM_W>();
        if PdhGetFormattedCounterArrayW(counter, fmt, &mut bytes, &mut count, Some(items)) != 0 {
            return Vec::new();
        }
        std::slice::from_raw_parts(items, count as usize)
            .iter()
            .filter(|item| item.FmtValue.CStatus == 0)
            .filter_map(|item| {
                let name = item.szName.to_string().ok()?;
                Some((name, item.FmtValue.Anonymous.doubleValue))
            })
            .collect()
    }
}
