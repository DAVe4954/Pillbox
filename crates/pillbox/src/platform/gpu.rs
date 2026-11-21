//! GPU load and temperature for the stats strip.
//!
//! * Load comes from the "GPU Engine" performance counters (PDH), the same numbers Task Manager
//!   shows. Instances are named `pid_<n>_luid_<hi>_<lo>_phys_<n>_eng_<n>_engtype_<kind>`; the
//!   3D engines are summed per adapter (LUID) and the busiest adapter wins, which on a laptop
//!   with two GPUs is the one the game runs on.
//! * Temperature comes from `D3DKMTQueryAdapterInfo(KMTQAITYPE_ADAPTERPERFDATA)` for that
//!   adapter, Task Manager's own source (documented in the Windows Driver Kit, WDDM 2.4+). It is
//!   in tenths of a degree Celsius; drivers that do not report it give 0, shown as "no reading".
//!
//! Both are read on a worker thread twice a second, only while the strip is on screen.

use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use windows::{
    Wdk::Graphics::Direct3D::{
        D3DKMT_ADAPTER_PERFDATA, D3DKMT_CLOSEADAPTER, D3DKMT_OPENADAPTERFROMLUID,
        D3DKMT_QUERYADAPTERINFO, D3DKMTCloseAdapter, D3DKMTOpenAdapterFromLuid,
        D3DKMTQueryAdapterInfo, KMTQAITYPE_ADAPTERPERFDATA,
    },
    Win32::{
        Foundation::LUID,
        System::Performance::{
            PDH_FMT, PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_DOUBLE, PDH_HCOUNTER, PDH_HQUERY,
            PDH_MORE_DATA, PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData,
            PdhGetFormattedCounterArrayW, PdhOpenQueryW,
        },
    },
    core::w,
};

/// The latest readings; `None` = not available (yet).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct GpuStats {
    /// Busiest adapter's 3D load, 0..100.
    pub load: Option<f32>,
    /// That adapter's temperature in °C.
    pub temp_c: Option<f32>,
    /// The PC has a graphics card the System card can show (`pillbox_core::sysinfo::shown_gpu`).
    pub has_card: bool,
    /// That card is integrated graphics (the PC has no dedicated one).
    pub card_integrated: bool,
    /// That card's 3D load, 0..100. A card that is powered down on a hybrid laptop has no engine
    /// instances in the counters; that reads as 0.
    pub card_load: Option<f32>,
}

/// The real graphics adapters DXGI lists, with their LUIDs. Empty if DXGI fails.
fn list_adapters() -> Vec<pillbox_core::sysinfo::GpuAdapter> {
    use windows::Win32::Graphics::Dxgi::{
        CreateDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE, IDXGIFactory1,
    };
    // SAFETY: plain COM calls on a factory we own; each adapter is read through its own
    // interface pointer and released when dropped.
    unsafe {
        let Ok(factory) = CreateDXGIFactory1::<IDXGIFactory1>() else {
            return Vec::new();
        };
        let mut list = Vec::new();
        let mut i = 0;
        while let Ok(adapter) = factory.EnumAdapters1(i) {
            i += 1;
            let Ok(desc) = adapter.GetDesc1() else {
                continue;
            };
            let len = desc
                .Description
                .iter()
                .position(|c| *c == 0)
                .unwrap_or(desc.Description.len());
            list.push(pillbox_core::sysinfo::GpuAdapter {
                luid: (desc.AdapterLuid.HighPart, desc.AdapterLuid.LowPart),
                name: String::from_utf16_lossy(&desc.Description[..len]),
                dedicated_bytes: desc.DedicatedVideoMemory as u64,
                software: desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0,
            });
        }
        list
    }
}

/// The card the System tab shows: its name as people know it ("GeForce RTX 5060 Laptop GPU")
/// and whether it is integrated graphics. `None` = no card to show. Asks DXGI (a few ms), so
/// call it when the tab or window opens, not every frame. Known at once, unlike the monitor's
/// first sample, so the GPU card is there from the first frame.
pub fn shown_card() -> Option<(String, bool)> {
    let list = list_adapters();
    pillbox_core::sysinfo::shown_gpu(&list).map(|g| {
        (
            pillbox_core::sysinfo::gpu_display_name(&g.adapter.name),
            !g.dedicated,
        )
    })
}

/// The card the System card shows: (LUID, is integrated). The dedicated card if the PC has one,
/// else the integrated GPU. Logs what it found, so a wrong pick (an integrated GPU with a big
/// memory carve-out counted as dedicated, say) shows up in the log.
fn card_luid() -> Option<((i32, u32), bool)> {
    let list = list_adapters();
    let found = pillbox_core::sysinfo::shown_gpu(&list);
    match &found {
        Some(g) => tracing::info!(
            "{} GPU: {} ({} MB of its own memory), luid {:#x}_{:#x}",
            if g.dedicated {
                "dedicated"
            } else {
                "integrated"
            },
            g.adapter.name,
            g.adapter.dedicated_bytes / (1024 * 1024),
            g.adapter.luid.0,
            g.adapter.luid.1
        ),
        None => tracing::info!(
            "no usable GPU among {:?}",
            list.iter().map(|a| a.name.as_str()).collect::<Vec<_>>()
        ),
    }
    found.map(|g| (g.adapter.luid, !g.dedicated))
}

const INTERVAL: Duration = Duration::from_millis(500);

/// Reads GPU load and temperature on a worker thread until dropped.
pub struct GpuMonitor {
    stop: Arc<AtomicBool>,
    shared: Arc<Mutex<GpuStats>>,
}

impl GpuMonitor {
    pub fn start() -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let shared = Arc::new(Mutex::new(GpuStats::default()));
        let (stop2, shared2) = (stop.clone(), shared.clone());
        let spawned = std::thread::Builder::new()
            .name("gpu-stats".into())
            .spawn(move || run(&stop2, &shared2));
        if let Err(e) = spawned {
            tracing::warn!("could not start GPU readings: {e}");
        }
        Self { stop, shared }
    }

    pub fn stats(&self) -> GpuStats {
        self.shared.lock().map(|s| *s).unwrap_or_default()
    }
}

impl Drop for GpuMonitor {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// The PDH query, closed on drop.
struct Query(PDH_HQUERY);

impl Drop for Query {
    fn drop(&mut self) {
        // SAFETY: the handle came from PdhOpenQueryW and is closed once.
        unsafe { PdhCloseQuery(self.0) };
    }
}

/// Kernel adapter handles opened for temperature reads, closed on drop.
#[derive(Default)]
struct Adapters(HashMap<(i32, u32), u32>);

impl Adapters {
    fn temperature(&mut self, luid: (i32, u32)) -> Option<f32> {
        let handle = match self.0.get(&luid) {
            Some(h) => *h,
            None => {
                let mut open = D3DKMT_OPENADAPTERFROMLUID {
                    AdapterLuid: LUID {
                        HighPart: luid.0,
                        LowPart: luid.1,
                    },
                    hAdapter: 0,
                };
                // SAFETY: `open` is a valid local struct with the LUID filled in.
                if unsafe { D3DKMTOpenAdapterFromLuid(&mut open) }.is_err() {
                    return None;
                }
                self.0.insert(luid, open.hAdapter);
                open.hAdapter
            }
        };
        let mut perf = D3DKMT_ADAPTER_PERFDATA::default();
        let mut query = D3DKMT_QUERYADAPTERINFO {
            hAdapter: handle,
            Type: KMTQAITYPE_ADAPTERPERFDATA,
            pPrivateDriverData: (&mut perf as *mut D3DKMT_ADAPTER_PERFDATA).cast(),
            PrivateDriverDataSize: size_of::<D3DKMT_ADAPTER_PERFDATA>() as u32,
        };
        // SAFETY: the query points at a local PERFDATA struct of exactly the stated size, which
        // outlives the call; the adapter handle was opened above.
        unsafe { D3DKMTQueryAdapterInfo(&mut query) }.ok().ok()?;
        // Tenths of a degree; 0 means the driver does not report it.
        (perf.Temperature > 0 && perf.Temperature < 1500).then(|| perf.Temperature as f32 / 10.0)
    }
}

impl Drop for Adapters {
    fn drop(&mut self) {
        for handle in self.0.values() {
            // SAFETY: each handle was opened by D3DKMTOpenAdapterFromLuid and is closed once.
            let _ = unsafe { D3DKMTCloseAdapter(&D3DKMT_CLOSEADAPTER { hAdapter: *handle }) };
        }
    }
}

fn run(stop: &AtomicBool, shared: &Mutex<GpuStats>) {
    let mut q = PDH_HQUERY::default();
    // SAFETY: out-pointer to a local handle; closed by `Query`'s Drop.
    if unsafe { PdhOpenQueryW(None, 0, &mut q) } != 0 {
        tracing::warn!("GPU load: PdhOpenQuery failed");
        return;
    }
    let query = Query(q);
    let mut counter = PDH_HCOUNTER::default();
    // English path, so it works whatever the Windows display language.
    // SAFETY: valid query handle, 'static path, out-pointer to a local.
    let added = unsafe {
        PdhAddEnglishCounterW(
            query.0,
            w!("\\GPU Engine(*engtype_3D)\\Utilization Percentage"),
            0,
            &mut counter,
        )
    };
    if added != 0 {
        tracing::warn!("GPU load counters are not available ({added:#x})");
    }
    let mut adapters = Adapters::default();
    let mut buf: Vec<u64> = Vec::new();
    let card = card_luid();
    // Rate counters need two collections before the first value.
    // SAFETY: valid query handle.
    unsafe { PdhCollectQueryData(query.0) };
    while !stop.load(Ordering::Relaxed) {
        std::thread::sleep(INTERVAL);
        // SAFETY: valid query handle.
        unsafe { PdhCollectQueryData(query.0) };
        let loads = if added == 0 {
            adapter_loads(counter, &mut buf)
        } else {
            None
        };
        let busiest = loads.as_ref().and_then(|m| {
            m.iter()
                .max_by(|a, b| a.1.total_cmp(b.1))
                .map(|(luid, load)| (*luid, *load))
        });
        let stats = GpuStats {
            load: busiest.map(|(_, load)| load),
            temp_c: busiest.and_then(|(luid, _)| adapters.temperature(luid)),
            has_card: card.is_some(),
            card_integrated: card.is_some_and(|(_, integrated)| integrated),
            // No readable instances while the counter works = nothing is using the GPU = 0.
            card_load: card.filter(|_| added == 0).map(|(luid, _)| {
                loads
                    .as_ref()
                    .and_then(|m| m.get(&luid))
                    .copied()
                    .unwrap_or(0.0)
            }),
        };
        if let Ok(mut s) = shared.lock() {
            *s = stats;
        }
    }
}

/// 3D engine load per adapter (LUID -> percent). `None` if the counter could not be read; an
/// empty map when no process uses any GPU engine right now.
fn adapter_loads(counter: PDH_HCOUNTER, buf: &mut Vec<u64>) -> Option<HashMap<(i32, u32), f32>> {
    // PDH_FMT_NOCAP100 (0x8000, not in the bindings): several engines can add up past 100.
    let fmt = PDH_FMT(PDH_FMT_DOUBLE.0 | 0x8000);
    let (mut bytes, mut count) = (0u32, 0u32);
    // SAFETY: a size query (no buffer); then the buffer is sized to what PDH asked for (as u64s,
    // so it is aligned for the item structs) and read only within `count` items. The name
    // pointers point into that same buffer.
    unsafe {
        let status = PdhGetFormattedCounterArrayW(counter, fmt, &mut bytes, &mut count, None);
        if status != PDH_MORE_DATA || bytes == 0 {
            return None;
        }
        buf.clear();
        buf.resize((bytes as usize).div_ceil(8), 0);
        let items = buf.as_mut_ptr().cast::<PDH_FMT_COUNTERVALUE_ITEM_W>();
        if PdhGetFormattedCounterArrayW(counter, fmt, &mut bytes, &mut count, Some(items)) != 0 {
            return None;
        }
        let mut per_adapter: HashMap<(i32, u32), f64> = HashMap::new();
        for item in std::slice::from_raw_parts(items, count as usize) {
            if item.FmtValue.CStatus != 0 {
                continue;
            }
            let Ok(name) = item.szName.to_string() else {
                continue;
            };
            if let Some(luid) = pillbox_core::games::parse_engine_luid(&name) {
                *per_adapter.entry(luid).or_default() += item.FmtValue.Anonymous.doubleValue;
            }
        }
        Some(
            per_adapter
                .into_iter()
                .map(|(luid, load)| (luid, load.clamp(0.0, 100.0) as f32))
                .collect(),
        )
    }
}
