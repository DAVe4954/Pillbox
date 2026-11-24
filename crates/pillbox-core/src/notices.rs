//! Device notices on the closed pill: the charger, the battery, headphones, drives and the
//! internet. When one is due and what it says; reading the devices is the app's job.

/// Percentages a battery that runs down is announced at, highest first.
pub const LOW_LEVELS: [u8; 2] = [20, 10];

/// What the battery is doing now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Power {
    pub percent: u8,
    pub plugged_in: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerNotice {
    /// The charger was plugged in.
    Charging(u8),
    /// The charger was pulled out.
    Unplugged(u8),
    /// The battery is full while on the charger.
    Full,
    /// The battery ran down to one of `LOW_LEVELS`.
    Low(u8),
}

impl PowerNotice {
    pub fn text(self) -> String {
        match self {
            PowerNotice::Charging(100) => "Plugged in · 100%".to_string(),
            PowerNotice::Charging(p) => format!("Charging · {p}%"),
            PowerNotice::Unplugged(p) => format!("On battery · {p}%"),
            PowerNotice::Full => "Fully charged".to_string(),
            PowerNotice::Low(p) => format!("Low battery · {p}%"),
        }
    }
}

/// Turns battery readings into notices, each once.
#[derive(Debug, Default)]
pub struct PowerWatch {
    last: Option<Power>,
    /// The lowest of `LOW_LEVELS` announced since the charger was last in.
    warned: Option<u8>,
    /// "Fully charged" was said since the charger was plugged in (a full battery goes back and
    /// forth between 99% and 100%).
    full_said: bool,
}

impl PowerWatch {
    /// A new reading (`None` = no battery). `quiet`: nothing can be shown now (a game is in
    /// front): what happens then is not announced later, except a low battery.
    pub fn update(&mut self, now: Option<Power>, quiet: bool) -> Option<PowerNotice> {
        let before = std::mem::replace(&mut self.last, now);
        let now = now?;
        // Only changes count: a PC that starts on its charger, or without it, says nothing.
        let notice = if now.plugged_in {
            self.warned = None;
            let full_now = now.percent >= 100;
            let was_full = self.full_said;
            self.full_said |= full_now;
            match before {
                Some(b) if !b.plugged_in => Some(PowerNotice::Charging(now.percent)),
                Some(b) if full_now && b.percent < 100 && !was_full => Some(PowerNotice::Full),
                _ => None,
            }
        } else {
            self.full_said = false;
            let level = LOW_LEVELS.into_iter().rfind(|l| now.percent <= *l);
            if let Some(level) = level
                && self.warned.is_none_or(|w| w > level)
            {
                if quiet {
                    return None;
                }
                self.warned = Some(level);
                return Some(PowerNotice::Low(now.percent));
            }
            before
                .filter(|b| b.plugged_in)
                .map(|_| PowerNotice::Unplugged(now.percent))
        };
        notice.filter(|_| !quiet)
    }
}

/// What changed between two lists of connected devices: (name, connected now), the gone ones
/// first. A name listed twice (one pair of headphones is several outputs) counts once.
pub fn device_changes(before: &[String], now: &[String]) -> Vec<(String, bool)> {
    let mut out: Vec<(String, bool)> = Vec::new();
    let gone = before.iter().filter(|n| !now.contains(n));
    let come = now.iter().filter(|n| !before.contains(n));
    for (name, connected) in gone.map(|n| (n, false)).chain(come.map(|n| (n, true))) {
        if !out.iter().any(|(n, _)| n == name) {
            out.push((name.clone(), connected));
        }
    }
    out
}

/// The drive letters in Windows' bit mask (bit 0 = A).
pub fn drive_letters(mask: u32) -> Vec<char> {
    (0..26u8)
        .filter(|bit| mask & (1 << bit) != 0)
        .map(|bit| char::from(b'A' + bit))
        .collect()
}

/// "Drive E: connected", "Drives E: and F: removed".
pub fn drive_text(letters: &[char], connected: bool) -> Option<String> {
    let what = if connected { "connected" } else { "removed" };
    match letters {
        [] => None,
        [one] => Some(format!("Drive {one}: {what}")),
        [a, b] => Some(format!("Drives {a}: and {b}: {what}")),
        many => Some(format!("{} drives {what}", many.len())),
    }
}

/// `PKEY_AudioEndpoint_FormFactor` values that are worn on the head.
const HEADPHONES: u32 = 3;
const HEADSET: u32 = 5;

/// Whether an audio output is headphones: by what the device says it is, or by its name (many
/// USB headsets call themselves speakers).
pub fn is_headphones(form_factor: Option<u32>, name: &str) -> bool {
    if matches!(form_factor, Some(HEADPHONES | HEADSET)) {
        return true;
    }
    let name = name.to_lowercase();
    [
        "headphone",
        "headset",
        "earphone",
        "earbud",
        "airpods",
        "buds",
    ]
    .iter()
    .any(|w| name.contains(w))
}

/// The name to show for headphones, from Windows' endpoint name: "Headphones (WH-1000XM4)" →
/// "WH-1000XM4". A plain jack on the sound chip has no name of its own: "Headphones".
pub fn headphone_name(endpoint: &str) -> String {
    const PLAIN: &str = "Headphones";
    // Windows numbers a second device of the same name: "Headphones (2- WH-1000XM4)".
    fn unnumbered(s: &str) -> &str {
        match s.split_once("- ") {
            Some((n, rest)) if !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()) => rest,
            _ => s,
        }
    }
    let inner = match (endpoint.find('('), endpoint.rfind(')')) {
        (Some(a), Some(b)) if a < b => &endpoint[a + 1..b],
        _ => endpoint,
    };
    let mut name = unnumbered(inner.trim()).trim();
    for tail in [" Hands-Free AG Audio", " Hands-Free", " Stereo"] {
        if let Some(cut) = name.strip_suffix(tail) {
            name = cut.trim_end();
        }
    }
    let lower = name.to_lowercase();
    let sound_chip = [
        "realtek",
        "high definition audio",
        "conexant",
        "cirrus logic",
        "idt audio",
        "via hd audio",
        "usb audio device",
        "usb pnp sound",
    ]
    .iter()
    .any(|w| lower.contains(w));
    if name.is_empty() || sound_chip {
        PLAIN.to_string()
    } else {
        name.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn on_battery(percent: u8) -> Option<Power> {
        Some(Power {
            percent,
            plugged_in: false,
        })
    }

    fn plugged(percent: u8) -> Option<Power> {
        Some(Power {
            percent,
            plugged_in: true,
        })
    }

    #[test]
    fn plugging_in_is_announced_once() {
        let mut w = PowerWatch::default();
        // Starting on the charger is nothing new.
        assert_eq!(w.update(plugged(50), false), None);
        assert_eq!(
            w.update(on_battery(50), false),
            Some(PowerNotice::Unplugged(50))
        );
        assert_eq!(w.update(on_battery(49), false), None);
        assert_eq!(
            w.update(plugged(49), false),
            Some(PowerNotice::Charging(49))
        );
        assert_eq!(w.update(plugged(60), false), None);
    }

    #[test]
    fn a_full_battery_is_announced_once_per_charge() {
        let mut w = PowerWatch::default();
        assert_eq!(w.update(plugged(98), false), None);
        assert_eq!(w.update(plugged(100), false), Some(PowerNotice::Full));
        assert_eq!(w.update(plugged(99), false), None);
        assert_eq!(w.update(plugged(100), false), None);
        // Unplugged and charged again.
        assert_eq!(
            w.update(on_battery(90), false),
            Some(PowerNotice::Unplugged(90))
        );
        assert!(w.update(plugged(90), false).is_some());
        assert_eq!(w.update(plugged(100), false), Some(PowerNotice::Full));
        // A PC that starts full says nothing, later either.
        let mut w = PowerWatch::default();
        assert_eq!(w.update(plugged(100), false), None);
        assert_eq!(w.update(plugged(99), false), None);
        assert_eq!(w.update(plugged(100), false), None);
    }

    #[test]
    fn pulling_the_charger_out_of_a_low_battery_warns() {
        let mut w = PowerWatch::default();
        w.update(plugged(15), false);
        assert_eq!(w.update(on_battery(15), false), Some(PowerNotice::Low(15)));
        assert_eq!(w.update(on_battery(14), false), None);
    }

    #[test]
    fn device_lists_are_compared() {
        let l = |s: &[&str]| s.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(device_changes(&l(&["A"]), &l(&["A"])).is_empty());
        // Two outputs of one pair: one change each way.
        let on = device_changes(&l(&[]), &l(&["A", "A"]));
        assert_eq!(on, vec![("A".to_string(), true)]);
        let off = device_changes(&l(&["A", "A"]), &l(&[]));
        assert_eq!(off, vec![("A".to_string(), false)]);
        // One of the two outputs goes: nothing happened to the headphones.
        assert!(device_changes(&l(&["A", "A"]), &l(&["A"])).is_empty());
        // Swapped: the new one is last, so it is what stays on the pill.
        let swap = device_changes(&l(&["A"]), &l(&["B"]));
        assert_eq!(
            swap,
            vec![("A".to_string(), false), ("B".to_string(), true)]
        );
    }

    #[test]
    fn drives_are_named() {
        assert_eq!(drive_letters(0b1_0000), vec!['E']);
        assert_eq!(drive_letters(0), Vec::<char>::new());
        assert_eq!(
            drive_text(&['E'], true).as_deref(),
            Some("Drive E: connected")
        );
        assert_eq!(
            drive_text(&['E', 'F'], false).as_deref(),
            Some("Drives E: and F: removed")
        );
        assert_eq!(
            drive_text(&['E', 'F', 'G'], true).as_deref(),
            Some("3 drives connected")
        );
        assert_eq!(drive_text(&[], true), None);
    }

    #[test]
    fn a_low_battery_is_announced_at_each_level_once() {
        let mut w = PowerWatch::default();
        assert_eq!(w.update(on_battery(21), false), None);
        assert_eq!(w.update(on_battery(20), false), Some(PowerNotice::Low(20)));
        assert_eq!(w.update(on_battery(19), false), None);
        assert_eq!(w.update(on_battery(11), false), None);
        assert_eq!(w.update(on_battery(9), false), Some(PowerNotice::Low(9)));
        assert_eq!(w.update(on_battery(3), false), None);
        // After charging it starts over.
        assert!(w.update(plugged(15), false).is_some());
        assert_eq!(w.update(on_battery(15), false), Some(PowerNotice::Low(15)));
    }

    #[test]
    fn starting_almost_empty_warns_once() {
        let mut w = PowerWatch::default();
        assert_eq!(w.update(on_battery(8), false), Some(PowerNotice::Low(8)));
        assert_eq!(w.update(on_battery(7), false), None);
    }

    #[test]
    fn during_a_game_low_waits_and_charging_is_skipped() {
        let mut w = PowerWatch::default();
        assert_eq!(w.update(on_battery(40), true), None);
        assert_eq!(w.update(on_battery(18), true), None);
        assert_eq!(w.update(on_battery(17), false), Some(PowerNotice::Low(17)));
        assert_eq!(w.update(plugged(17), true), None);
        assert_eq!(w.update(plugged(30), false), None);
    }

    #[test]
    fn a_pc_without_a_battery_says_nothing() {
        let mut w = PowerWatch::default();
        assert_eq!(w.update(None, false), None);
        assert_eq!(w.update(None, false), None);
    }

    #[test]
    fn notices_read_naturally() {
        assert_eq!(PowerNotice::Charging(63).text(), "Charging · 63%");
        assert_eq!(PowerNotice::Charging(100).text(), "Plugged in · 100%");
        assert_eq!(PowerNotice::Unplugged(63).text(), "On battery · 63%");
        assert_eq!(PowerNotice::Full.text(), "Fully charged");
        assert_eq!(PowerNotice::Low(9).text(), "Low battery · 9%");
    }

    #[test]
    fn headphones_are_told_from_speakers() {
        assert!(is_headphones(Some(3), "Headphones (WH-1000XM4)"));
        assert!(is_headphones(Some(5), "Headset (Arctis)"));
        assert!(is_headphones(Some(1), "Speakers (HyperX Headset)"));
        assert!(!is_headphones(Some(1), "Speakers (Realtek(R) Audio)"));
        assert!(!is_headphones(None, "LG TV (NVIDIA High Definition Audio)"));
    }

    #[test]
    fn headphone_names_are_short() {
        assert_eq!(headphone_name("Headphones (WH-1000XM4)"), "WH-1000XM4");
        assert_eq!(headphone_name("Headphones (2- WH-1000XM4)"), "WH-1000XM4");
        assert_eq!(
            headphone_name("Headset (WH-1000XM4 Hands-Free AG Audio)"),
            "WH-1000XM4"
        );
        assert_eq!(headphone_name("Headphones (AirPods Stereo)"), "AirPods");
        assert_eq!(
            headphone_name("Headphones (Realtek(R) Audio)"),
            "Headphones"
        );
        assert_eq!(headphone_name("My headset"), "My headset");
        assert_eq!(headphone_name(""), "Headphones");
    }
}
