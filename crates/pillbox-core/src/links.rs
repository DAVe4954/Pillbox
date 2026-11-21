//! What the Controls tab shows about Wi-Fi networks and Bluetooth devices: the lists, their
//! order, signal bars and the one-line summaries on the cards. Pure data; the readings come from
//! `platform/links.rs`.

use std::collections::HashMap;

/// A Wi-Fi network in range.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WifiNet {
    pub ssid: String,
    /// Signal quality, 0..=100.
    pub signal: u8,
    /// Needs a password.
    pub secure: bool,
    /// Windows has joined it before, so it can be connected with one click.
    pub saved: bool,
    /// The one connected now.
    pub connected: bool,
    /// The name of the saved profile (empty when not saved).
    pub profile: String,
}

/// Dedupe by name (an access point often shows up several times), drop hidden networks without
/// a name, and order them: connected, then saved, then the strongest.
pub fn sort_networks(list: Vec<WifiNet>) -> Vec<WifiNet> {
    let mut best: HashMap<String, WifiNet> = HashMap::new();
    for net in list.into_iter().filter(|n| !n.ssid.trim().is_empty()) {
        match best.get_mut(&net.ssid) {
            Some(old) => {
                let keep_flags = (old.connected || net.connected, old.saved || net.saved);
                if net.signal > old.signal {
                    let profile = if old.profile.is_empty() {
                        net.profile.clone()
                    } else {
                        old.profile.clone()
                    };
                    *old = WifiNet { profile, ..net };
                }
                old.connected = keep_flags.0;
                old.saved = keep_flags.1;
            }
            None => {
                best.insert(net.ssid.clone(), net);
            }
        }
    }
    let mut out: Vec<WifiNet> = best.into_values().collect();
    out.sort_by(|a, b| {
        b.connected
            .cmp(&a.connected)
            .then(b.saved.cmp(&a.saved))
            .then(b.signal.cmp(&a.signal))
            .then(a.ssid.to_lowercase().cmp(&b.ssid.to_lowercase()))
    });
    out
}

/// Bars to light, 0..=4, for a signal quality in percent.
pub fn signal_bars(signal: u8) -> u8 {
    match signal {
        0 => 0,
        1..=24 => 1,
        25..=49 => 2,
        50..=74 => 3,
        _ => 4,
    }
}

/// What kind of thing a Bluetooth device is, for its icon and for what the card can do with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BtKind {
    /// Headphones, earbuds, speakers, a car: has audio services that can be switched.
    Audio,
    /// Mouse, keyboard, game controller.
    Input,
    Phone,
    Computer,
    #[default]
    Other,
}

/// The major device class from the Bluetooth "class of device" (the Windows enum values).
pub fn kind_from_major_class(major: u32) -> BtKind {
    match major {
        1 => BtKind::Computer,
        2 => BtKind::Phone,
        4 => BtKind::Audio,
        5 => BtKind::Input,
        _ => BtKind::Other,
    }
}

/// A guess from the name, for devices that give no class (Low Energy ones).
pub fn kind_from_name(name: &str) -> BtKind {
    let n = name.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| n.contains(w));
    if has(&[
        "mouse",
        "keyboard",
        "controller",
        "gamepad",
        "joystick",
        "trackpad",
        "pen",
        "remote",
    ]) {
        BtKind::Input
    } else if has(&[
        "airpods",
        "buds",
        "headphone",
        "headset",
        "earbud",
        "speaker",
        "soundbar",
        "pods",
        "pins",
        "audio",
        "jbl",
        "bose",
        "sony wh",
        "sony wf",
    ]) {
        BtKind::Audio
    } else if has(&["iphone", "galaxy", "pixel", "phone", "xiaomi", "huawei"]) {
        BtKind::Phone
    } else {
        BtKind::Other
    }
}

/// A paired Bluetooth device.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BtDevice {
    pub name: String,
    /// The 48-bit address as a number.
    pub address: u64,
    pub connected: bool,
    pub kind: BtKind,
    /// Seen as a classic device (not Low Energy only): these can be connected and disconnected
    /// from here when they are audio devices.
    pub classic: bool,
}

impl BtDevice {
    /// The card can connect and disconnect it: classic audio devices. Mice, controllers and
    /// phones reconnect on their own, so they only show their state.
    pub fn can_toggle(&self) -> bool {
        self.classic && self.kind == BtKind::Audio
    }
}

/// The same device is found as a classic and as a Low Energy one: keep one entry per address
/// (connected if either is, classic if either is), named, connected first.
pub fn merge_devices(list: Vec<BtDevice>) -> Vec<BtDevice> {
    let mut by_address: HashMap<u64, BtDevice> = HashMap::new();
    for dev in list.into_iter().filter(|d| !d.name.trim().is_empty()) {
        match by_address.get_mut(&dev.address) {
            Some(old) => {
                old.connected |= dev.connected;
                old.classic |= dev.classic;
                if old.kind == BtKind::Other {
                    old.kind = dev.kind;
                }
            }
            None => {
                by_address.insert(dev.address, dev);
            }
        }
    }
    let mut out: Vec<BtDevice> = by_address.into_values().collect();
    out.sort_by(|a, b| {
        b.connected
            .cmp(&a.connected)
            .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then(a.address.cmp(&b.address))
    });
    out
}

/// The grey line under "Wi-Fi" on its card.
pub fn wifi_summary(radio: Option<bool>, adapter: bool, nets: &[WifiNet]) -> String {
    match radio {
        None if !adapter => "No Wi-Fi adapter".to_string(),
        Some(false) => "Off".to_string(),
        _ => nets
            .iter()
            .find(|n| n.connected)
            .map_or_else(|| "Not connected".to_string(), |n| n.ssid.clone()),
    }
}

/// The grey line under "Bluetooth" on its card.
pub fn bt_summary(radio: Option<bool>, devices: &[BtDevice]) -> String {
    match radio {
        None => "No Bluetooth".to_string(),
        Some(false) => "Off".to_string(),
        Some(true) => {
            let connected: Vec<&BtDevice> = devices.iter().filter(|d| d.connected).collect();
            match connected.as_slice() {
                [] => "Not connected".to_string(),
                [one] => one.name.clone(),
                many => format!("{} connected", many.len()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn net(ssid: &str, signal: u8, saved: bool, connected: bool) -> WifiNet {
        WifiNet {
            ssid: ssid.to_string(),
            signal,
            secure: true,
            saved,
            connected,
            profile: if saved {
                ssid.to_string()
            } else {
                String::new()
            },
        }
    }

    fn dev(name: &str, address: u64, connected: bool, kind: BtKind, classic: bool) -> BtDevice {
        BtDevice {
            name: name.to_string(),
            address,
            connected,
            kind,
            classic,
        }
    }

    #[test]
    fn networks_are_deduped_and_ordered() {
        let sorted = sort_networks(vec![
            net("Cafe", 80, false, false),
            net("Home", 40, true, false),
            net("Home", 70, true, false),
            net("Office", 30, true, true),
            net("", 90, false, false),
            net("Neighbour", 60, false, false),
        ]);
        let names: Vec<&str> = sorted.iter().map(|n| n.ssid.as_str()).collect();
        assert_eq!(names, ["Office", "Home", "Cafe", "Neighbour"]);
        assert_eq!(sorted[1].signal, 70, "the strongest copy of a name wins");
    }

    #[test]
    fn a_connected_copy_keeps_its_flags_when_a_stronger_copy_wins() {
        let sorted = sort_networks(vec![
            net("Home", 40, true, true),
            net("Home", 90, false, false),
        ]);
        assert_eq!(sorted.len(), 1);
        assert!(sorted[0].connected && sorted[0].saved);
        assert_eq!(sorted[0].signal, 90);
        assert_eq!(sorted[0].profile, "Home", "the saved profile name is kept");
    }

    #[test]
    fn bars_follow_the_signal() {
        let bars: Vec<u8> = [0, 1, 24, 25, 49, 50, 74, 75, 100]
            .into_iter()
            .map(signal_bars)
            .collect();
        assert_eq!(bars, [0, 1, 1, 2, 2, 3, 3, 4, 4]);
    }

    #[test]
    fn device_kinds_from_class_and_name() {
        assert_eq!(kind_from_major_class(4), BtKind::Audio);
        assert_eq!(kind_from_major_class(5), BtKind::Input);
        assert_eq!(kind_from_major_class(2), BtKind::Phone);
        assert_eq!(kind_from_major_class(0), BtKind::Other);
        assert_eq!(kind_from_name("X3-5.2 Mouse"), BtKind::Input);
        assert_eq!(kind_from_name("Wireless Controller"), BtKind::Input);
        assert_eq!(kind_from_name("Niceboy HIVE Pins 3 ANC"), BtKind::Audio);
        assert_eq!(
            kind_from_name("iPhone Galaxy S24 Pro-Ultra-Max"),
            BtKind::Phone
        );
        assert_eq!(kind_from_name("Mercedes AMG SL Roadster"), BtKind::Other);
    }

    #[test]
    fn one_entry_per_address_and_only_classic_audio_toggles() {
        let merged = merge_devices(vec![
            dev("Buds", 1, false, BtKind::Audio, true),
            dev("Buds", 1, true, BtKind::Other, false),
            dev("Mouse", 2, true, BtKind::Input, false),
            dev("", 3, true, BtKind::Other, true),
            dev("Car", 4, false, BtKind::Other, true),
        ]);
        let names: Vec<&str> = merged.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(
            names,
            ["Buds", "Mouse", "Car"],
            "connected first, unnamed dropped"
        );
        assert!(merged[0].connected && merged[0].classic);
        assert!(merged[0].can_toggle());
        assert!(!merged[1].can_toggle(), "a mouse only shows its state");
        assert!(
            !merged[2].can_toggle(),
            "unknown kinds only show their state"
        );
    }

    #[test]
    fn wifi_card_lines() {
        let nets = [net("Home", 70, true, true)];
        assert_eq!(wifi_summary(None, false, &[]), "No Wi-Fi adapter");
        assert_eq!(wifi_summary(Some(false), true, &nets), "Off");
        assert_eq!(wifi_summary(Some(true), true, &nets), "Home");
        assert_eq!(wifi_summary(Some(true), true, &[]), "Not connected");
        // The radio list can be missing while the adapter list works.
        assert_eq!(wifi_summary(None, true, &nets), "Home");
    }

    #[test]
    fn bluetooth_card_lines() {
        let one = [dev("Buds", 1, true, BtKind::Audio, true)];
        let two = [
            dev("Buds", 1, true, BtKind::Audio, true),
            dev("Mouse", 2, true, BtKind::Input, false),
        ];
        assert_eq!(bt_summary(None, &[]), "No Bluetooth");
        assert_eq!(bt_summary(Some(false), &one), "Off");
        assert_eq!(bt_summary(Some(true), &[]), "Not connected");
        assert_eq!(bt_summary(Some(true), &one), "Buds");
        assert_eq!(bt_summary(Some(true), &two), "2 connected");
    }
}
