//! Picking what the System card shows when the PC has several of something: the busiest disk,
//! the graphics card and the connection in use. Pure logic; the platform layer lists the disks,
//! adapters and counters.

/// One physical disk's activity.
#[derive(Debug, Clone, PartialEq)]
pub struct DiskActivity {
    /// "C:", "D: E:" (its drive letters) or "Disk 2" when it has none.
    pub name: String,
    /// Share of the time the disk was busy, 0..100 (Task Manager's "Active time").
    pub active: f32,
    /// Bytes read and written per second.
    pub read_bps: f64,
    pub write_bps: f64,
}

impl DiskActivity {
    /// Read plus write, bytes per second.
    pub fn total_bps(&self) -> f64 {
        self.read_bps + self.write_bps
    }
}

/// A graphics adapter's name as DXGI gives it, without the noise: "(R)" and "(TM)" marks, the
/// maker's name for NVIDIA and AMD (the product name says it: "GeForce RTX 5060 Laptop GPU",
/// "Radeon RX 7800 XT"), and repeated spaces. "Intel(R) UHD Graphics" is "Intel UHD Graphics".
pub fn gpu_display_name(raw: &str) -> String {
    let mut name = raw
        .replace("(R)", "")
        .replace("(TM)", "")
        .replace("(tm)", "");
    name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    for maker in ["NVIDIA ", "AMD "] {
        if let Some(rest) = name.strip_prefix(maker)
            && !rest.is_empty()
        {
            name = rest.to_string();
        }
    }
    name
}

/// A `PhysicalDisk` counter instance name as a label: "0 C:" -> "C:", "1 D: E:" -> "D: E:",
/// "2" -> "Disk 2". The "_Total" instance (and an empty name) is not a disk: `None`.
pub fn disk_name(instance: &str) -> Option<String> {
    let instance = instance.trim();
    if instance.is_empty() || instance.eq_ignore_ascii_case("_total") {
        return None;
    }
    let (index, letters) = instance.split_once(' ').unwrap_or((instance, ""));
    let letters = letters.trim();
    Some(if letters.is_empty() {
        format!("Disk {index}")
    } else {
        letters.to_uppercase()
    })
}

/// The disk doing the most: highest active time, then highest speed; with every disk idle, the
/// first one listed (the system disk is disk 0).
pub fn busiest_disk(disks: &[DiskActivity]) -> Option<&DiskActivity> {
    // `max_by` keeps the last of equal elements; reversing makes a full tie pick the first.
    disks.iter().rev().max_by(|a, b| {
        a.active
            .total_cmp(&b.active)
            .then(a.total_bps().total_cmp(&b.total_bps()))
    })
}

/// A graphics adapter as DXGI lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GpuAdapter {
    /// (high part, low part), the same key the "GPU Engine" counters use.
    pub luid: (i32, u32),
    pub name: String,
    /// Dedicated video memory. Integrated GPUs report a small slice carved from system memory.
    pub dedicated_bytes: u64,
    /// The software renderer ("Microsoft Basic Render Driver").
    pub software: bool,
}

/// Dedicated video memory below this is treated as an integrated GPU (Intel UHD/Iris report
/// 128 MB or less; every discrete card sold in the last decade has 1 GB or more).
pub const DEDICATED_MIN_BYTES: u64 = 512 * 1024 * 1024;

/// The dedicated graphics card: the real adapter with the most dedicated memory, if that is at
/// least `DEDICATED_MIN_BYTES`. `None` on a PC with only integrated graphics.
pub fn dedicated_gpu(adapters: &[GpuAdapter]) -> Option<&GpuAdapter> {
    adapters
        .iter()
        .filter(|a| !a.software && a.dedicated_bytes >= DEDICATED_MIN_BYTES)
        .max_by_key(|a| a.dedicated_bytes)
}

/// The graphics card the System card shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShownGpu<'a> {
    pub adapter: &'a GpuAdapter,
    /// A dedicated card; `false` = the PC only has integrated graphics.
    pub dedicated: bool,
}

/// The dedicated card if there is one; otherwise the integrated GPU (the real adapter with the
/// most video memory, the first on a tie). `None` when there is no real adapter at all.
pub fn shown_gpu(adapters: &[GpuAdapter]) -> Option<ShownGpu<'_>> {
    if let Some(adapter) = dedicated_gpu(adapters) {
        return Some(ShownGpu {
            adapter,
            dedicated: true,
        });
    }
    adapters
        .iter()
        .rev()
        .filter(|a| !a.software)
        .max_by_key(|a| a.dedicated_bytes)
        .map(|adapter| ShownGpu {
            adapter,
            dedicated: false,
        })
}

/// One network adapter's running byte counters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IfSample {
    pub index: u32,
    /// As Windows shows it: "Wi-Fi", "Ethernet".
    pub name: String,
    pub down: u64,
    pub up: u64,
}

/// The connection the card shows.
#[derive(Debug, Clone, PartialEq)]
pub struct NetReading {
    pub index: u32,
    pub name: String,
    /// Download and upload in bytes per second; `None` until there are two readings.
    pub rates: Option<(f64, f64)>,
}

/// Picks the connection that is carrying traffic when several adapters are up (Wi-Fi and
/// Ethernet at once, say). Windows' routing table cannot tell: both can have a default route
/// with the same metric. So the busiest adapter wins, and the choice only switches when another
/// one is clearly busier, so the name does not flicker while both are idle.
#[derive(Debug, Default)]
pub struct NetPicker {
    last: Option<(f64, Vec<IfSample>)>,
    current: Option<u32>,
    reading: Option<NetReading>,
}

/// Below this (bytes per second, both directions) an adapter does not count as busy.
const BUSY_MIN: f64 = 20_000.0;
/// A challenger must be this many times busier than the current choice to take over.
const SWITCH_FACTOR: f64 = 2.0;
/// Readings closer together than this (seconds) give noisy rates and are not used.
const MIN_INTERVAL: f64 = 0.5;

impl NetPicker {
    /// Feed the counters of every adapter that is up. `now` is any monotonic clock in seconds;
    /// `preferred` is the adapter Windows routes the internet through (used when nothing has
    /// moved any traffic yet). Returns the connection to show, `None` when no adapter is up.
    pub fn update(
        &mut self,
        now: f64,
        samples: Vec<IfSample>,
        preferred: Option<u32>,
    ) -> Option<NetReading> {
        if samples.is_empty() {
            *self = Self::default();
            return None;
        }
        let has = |i: u32| samples.iter().any(|s| s.index == i);
        let Some((then, old)) = self.last.as_ref() else {
            self.last = Some((now, samples));
            return self.pick(None, preferred);
        };
        if now - then < MIN_INTERVAL {
            // Too soon: keep the last reading, but drop it if its adapter went away.
            return self.reading.clone().filter(|r| has(r.index));
        }
        let secs = now - then;
        let rates: Vec<(u32, f64, f64)> = samples
            .iter()
            .filter_map(|s| {
                let o = old.iter().find(|o| o.index == s.index)?;
                Some((
                    s.index,
                    s.down.saturating_sub(o.down) as f64 / secs,
                    s.up.saturating_sub(o.up) as f64 / secs,
                ))
            })
            .collect();
        self.last = Some((now, samples));
        self.pick(Some(rates), preferred)
    }

    fn pick(
        &mut self,
        rates: Option<Vec<(u32, f64, f64)>>,
        preferred: Option<u32>,
    ) -> Option<NetReading> {
        let (_, samples) = self.last.as_ref()?;
        let has = |i: u32| samples.iter().any(|s| s.index == i);
        let busy = |i: u32| {
            rates
                .as_ref()
                .and_then(|r| r.iter().find(|r| r.0 == i))
                .map_or(0.0, |r| r.1 + r.2)
        };
        if self.current.is_some_and(|c| !has(c)) {
            self.current = None;
        }
        let busiest = rates.as_ref().and_then(|r| {
            r.iter()
                .max_by(|a, b| (a.1 + a.2).total_cmp(&(b.1 + b.2)))
                .map(|r| r.0)
        });
        let current = match (self.current, busiest) {
            (Some(c), Some(b))
                if b != c && busy(b) >= BUSY_MIN && busy(b) > busy(c) * SWITCH_FACTOR =>
            {
                b
            }
            (Some(c), _) => c,
            (None, Some(b)) if busy(b) >= BUSY_MIN => b,
            (None, _) => preferred
                .filter(|p| has(*p))
                .or_else(|| samples.first().map(|s| s.index))?,
        };
        self.current = Some(current);
        let name = samples.iter().find(|s| s.index == current)?.name.clone();
        let reading = NetReading {
            index: current,
            name,
            rates: rates
                .as_ref()
                .and_then(|r| r.iter().find(|r| r.0 == current))
                .map(|r| (r.1, r.2)),
        };
        self.reading = Some(reading.clone());
        Some(reading)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(index: u32, name: &str, down: u64, up: u64) -> IfSample {
        IfSample {
            index,
            name: name.into(),
            down,
            up,
        }
    }

    #[test]
    fn the_first_reading_shows_the_routed_adapter_without_rates() {
        let mut p = NetPicker::default();
        let r = p
            .update(
                1.0,
                vec![sample(3, "Wi-Fi", 0, 0), sample(5, "Ethernet", 0, 0)],
                Some(5),
            )
            .unwrap();
        assert_eq!((r.name.as_str(), r.rates), ("Ethernet", None));
    }

    #[test]
    fn the_adapter_with_traffic_is_shown_with_its_rates() {
        let mut p = NetPicker::default();
        p.update(
            1.0,
            vec![sample(3, "Wi-Fi", 0, 0), sample(5, "Ethernet", 0, 0)],
            Some(5),
        );
        let r = p
            .update(
                3.0,
                vec![
                    sample(3, "Wi-Fi", 4_000_000, 200_000),
                    sample(5, "Ethernet", 1_000, 0),
                ],
                Some(5),
            )
            .unwrap();
        assert_eq!(r.name, "Wi-Fi");
        assert_eq!(r.rates, Some((2_000_000.0, 100_000.0)));
    }

    #[test]
    fn idle_adapters_do_not_make_the_name_flicker() {
        let mut p = NetPicker::default();
        p.update(
            1.0,
            vec![sample(3, "Wi-Fi", 0, 0), sample(5, "Ethernet", 0, 0)],
            Some(5),
        );
        // Ethernet is chosen; a trickle on Wi-Fi (below the busy minimum) changes nothing.
        let r = p
            .update(
                3.0,
                vec![sample(3, "Wi-Fi", 5_000, 0), sample(5, "Ethernet", 0, 0)],
                Some(5),
            )
            .unwrap();
        assert_eq!(r.name, "Ethernet");
    }

    #[test]
    fn a_slightly_busier_challenger_does_not_take_over_but_a_much_busier_one_does() {
        let mut p = NetPicker::default();
        p.update(
            1.0,
            vec![sample(3, "Wi-Fi", 0, 0), sample(5, "Ethernet", 0, 0)],
            Some(5),
        );
        p.update(
            3.0,
            vec![
                sample(3, "Wi-Fi", 0, 0),
                sample(5, "Ethernet", 1_000_000, 0),
            ],
            Some(5),
        );
        let r = p
            .update(
                5.0,
                vec![
                    sample(3, "Wi-Fi", 1_200_000, 0),
                    sample(5, "Ethernet", 2_000_000, 0),
                ],
                Some(5),
            )
            .unwrap();
        assert_eq!(r.name, "Ethernet");
        let r = p
            .update(
                7.0,
                vec![
                    sample(3, "Wi-Fi", 21_200_000, 0),
                    sample(5, "Ethernet", 2_100_000, 0),
                ],
                Some(5),
            )
            .unwrap();
        assert_eq!(r.name, "Wi-Fi");
    }

    #[test]
    fn readings_too_close_together_keep_the_previous_rates() {
        let mut p = NetPicker::default();
        p.update(1.0, vec![sample(3, "Wi-Fi", 0, 0)], None);
        let a = p
            .update(3.0, vec![sample(3, "Wi-Fi", 2_000_000, 0)], None)
            .unwrap();
        let b = p
            .update(3.1, vec![sample(3, "Wi-Fi", 2_000_100, 0)], None)
            .unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn no_adapter_up_means_offline_and_a_fresh_start_afterwards() {
        let mut p = NetPicker::default();
        p.update(1.0, vec![sample(3, "Wi-Fi", 0, 0)], None);
        assert!(p.update(3.0, Vec::new(), None).is_none());
        let r = p
            .update(5.0, vec![sample(3, "Wi-Fi", 99, 99)], None)
            .unwrap();
        assert_eq!(r.rates, None);
    }

    #[test]
    fn a_vanished_adapter_is_dropped() {
        let mut p = NetPicker::default();
        p.update(
            1.0,
            vec![sample(3, "Wi-Fi", 0, 0), sample(5, "Ethernet", 0, 0)],
            Some(5),
        );
        let r = p
            .update(3.0, vec![sample(3, "Wi-Fi", 10, 10)], Some(5))
            .unwrap();
        assert_eq!(r.name, "Wi-Fi");
    }

    fn gpu(name: &str, mb: u64, software: bool) -> GpuAdapter {
        GpuAdapter {
            luid: (0, 1),
            name: name.into(),
            dedicated_bytes: mb * 1024 * 1024,
            software,
        }
    }

    fn disk(name: &str, active: f32, mb_s: f64) -> DiskActivity {
        DiskActivity {
            name: name.into(),
            active,
            read_bps: mb_s * 1e6,
            write_bps: 0.0,
        }
    }

    #[test]
    fn gpu_names_lose_the_marks_and_the_maker_where_the_product_says_it() {
        assert_eq!(
            gpu_display_name("NVIDIA GeForce RTX 5060 Laptop GPU"),
            "GeForce RTX 5060 Laptop GPU"
        );
        assert_eq!(
            gpu_display_name("AMD Radeon RX 7800 XT"),
            "Radeon RX 7800 XT"
        );
        assert_eq!(
            gpu_display_name("Intel(R) UHD Graphics"),
            "Intel UHD Graphics"
        );
        assert_eq!(gpu_display_name("Intel(R)  Arc(TM) A770"), "Intel Arc A770");
        assert_eq!(gpu_display_name("NVIDIA "), "NVIDIA");
        assert_eq!(gpu_display_name(""), "");
    }

    #[test]
    fn disk_speed_is_read_plus_write() {
        let d = DiskActivity {
            name: "C:".into(),
            active: 5.0,
            read_bps: 3.0e6,
            write_bps: 1.0e6,
        };
        assert_eq!(d.total_bps(), 4.0e6);
    }

    #[test]
    fn counter_instance_names_become_drive_letters() {
        assert_eq!(disk_name("0 C:").as_deref(), Some("C:"));
        assert_eq!(disk_name("1 D: E:").as_deref(), Some("D: E:"));
        assert_eq!(disk_name("2").as_deref(), Some("Disk 2"));
        assert_eq!(disk_name("_Total"), None);
        assert_eq!(disk_name("  "), None);
    }

    #[test]
    fn the_busiest_disk_wins_by_active_time_then_by_speed() {
        let disks = [
            disk("C:", 3.0, 1.0),
            disk("D:", 40.0, 5.0),
            disk("E:", 40.0, 90.0),
        ];
        assert_eq!(busiest_disk(&disks).map(|d| d.name.as_str()), Some("E:"));
        assert!(busiest_disk(&[]).is_none());
    }

    #[test]
    fn with_every_disk_idle_the_first_one_is_shown() {
        let disks = [disk("C:", 0.0, 0.0), disk("D:", 0.0, 0.0)];
        assert_eq!(busiest_disk(&disks).map(|d| d.name.as_str()), Some("C:"));
    }

    #[test]
    fn only_an_integrated_gpu_is_shown_as_integrated() {
        let list = [
            gpu("Microsoft Basic Render Driver", 0, true),
            gpu("Intel UHD Graphics", 128, false),
        ];
        let shown = shown_gpu(&list).unwrap();
        assert_eq!(shown.adapter.name, "Intel UHD Graphics");
        assert!(!shown.dedicated);
    }

    #[test]
    fn a_dedicated_card_beats_the_integrated_one_and_is_marked_dedicated() {
        let list = [
            gpu("Intel UHD Graphics", 128, false),
            gpu("NVIDIA GeForce RTX 5060 Laptop GPU", 8151, false),
        ];
        let shown = shown_gpu(&list).unwrap();
        assert_eq!(shown.adapter.name, "NVIDIA GeForce RTX 5060 Laptop GPU");
        assert!(shown.dedicated);
    }

    #[test]
    fn no_real_adapter_means_no_gpu() {
        assert!(shown_gpu(&[gpu("Microsoft Basic Render Driver", 0, true)]).is_none());
        assert!(shown_gpu(&[]).is_none());
    }

    #[test]
    fn integrated_and_software_adapters_are_not_dedicated() {
        let list = [
            gpu("Intel UHD Graphics", 128, false),
            gpu("Microsoft Basic Render Driver", 0, true),
        ];
        assert!(dedicated_gpu(&list).is_none());
    }

    #[test]
    fn the_discrete_card_is_picked_next_to_an_integrated_one() {
        let list = [
            gpu("Intel UHD Graphics", 128, false),
            gpu("NVIDIA GeForce RTX 5060 Laptop GPU", 8151, false),
        ];
        assert_eq!(
            dedicated_gpu(&list).map(|a| a.name.as_str()),
            Some("NVIDIA GeForce RTX 5060 Laptop GPU")
        );
    }

    #[test]
    fn with_two_discrete_cards_the_bigger_one_is_shown() {
        let list = [gpu("Small", 2048, false), gpu("Big", 16384, false)];
        assert_eq!(dedicated_gpu(&list).map(|a| a.name.as_str()), Some("Big"));
    }
}
