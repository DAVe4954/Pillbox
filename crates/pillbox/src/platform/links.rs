//! Wi-Fi networks and paired Bluetooth devices for the Controls tab, on one worker thread (the
//! calls take from tens of milliseconds to several seconds). The radios' on/off switches live in
//! `controls.rs`; this lists what is around and connects and disconnects.
//!
//! - Wi-Fi: the Native Wifi API (`wlanapi`), the same service the Windows flyout talks to.
//!   Only networks Windows already has a saved profile for can be joined from here; a new
//!   password-protected network needs Windows' own dialog.
//! - Bluetooth: `Windows.Devices.Bluetooth` lists the paired devices and their state. A classic
//!   audio device is disconnected with the driver's disconnect request, and connected by
//!   switching its audio services off and on again (what the device's Properties > Services tab
//!   does). Mice, controllers and phones only show their state.

use std::{
    ffi::c_void,
    sync::mpsc::{self, Receiver, Sender},
    time::Duration,
};

use anyhow::{Result, anyhow};
use pillbox_core::links::{
    BtDevice, BtKind, WifiNet, kind_from_major_class, kind_from_name, merge_devices, sort_networks,
};
use windows::{
    Devices::{
        Bluetooth::{BluetoothConnectionStatus, BluetoothDevice, BluetoothLEDevice},
        Enumeration::DeviceInformation,
    },
    Win32::{
        Devices::Bluetooth::{
            BLUETOOTH_DEVICE_INFO, BLUETOOTH_FIND_RADIO_PARAMS, BLUETOOTH_SERVICE_DISABLE,
            BLUETOOTH_SERVICE_ENABLE, BluetoothFindFirstRadio, BluetoothFindRadioClose,
            BluetoothSetServiceState,
        },
        Foundation::{CloseHandle, HANDLE},
        NetworkManagement::WiFi::{
            DOT11_SSID, WLAN_AVAILABLE_NETWORK_CONNECTED, WLAN_AVAILABLE_NETWORK_HAS_PROFILE,
            WLAN_AVAILABLE_NETWORK_LIST, WLAN_CONNECTION_PARAMETERS, WLAN_INTERFACE_INFO_LIST,
            WlanCloseHandle, WlanConnect, WlanDisconnect, WlanEnumInterfaces, WlanFreeMemory,
            WlanGetAvailableNetworkList, WlanOpenHandle, WlanScan, dot11_BSS_type_any,
            wlan_connection_mode_profile,
        },
        System::{
            Com::{COINIT_MULTITHREADED, CoInitializeEx},
            IO::DeviceIoControl,
        },
    },
    core::{GUID, PCWSTR},
};

use super::window::Waker;

/// `IOCTL_BTH_DISCONNECT_DEVICE` (`CTL_CODE(FILE_DEVICE_BLUETOOTH, 3, METHOD_BUFFERED, ...)`),
/// which `windows-rs` does not define. Input: the device's 64-bit address.
const IOCTL_BTH_DISCONNECT_DEVICE: u32 = 0x0041_000C;

/// Audio services switched off and on to reconnect a device: A2DP sink, hands-free, headset.
const AUDIO_SERVICES: [u128; 3] = [
    0x0000_110B_0000_1000_8000_0080_5F9B_34FB,
    0x0000_111E_0000_1000_8000_0080_5F9B_34FB,
    0x0000_1108_0000_1000_8000_0080_5F9B_34FB,
];

/// What the Wi-Fi card lists.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct WifiList {
    /// The PC has a Wi-Fi adapter and the service that drives it runs.
    pub adapter: bool,
    pub nets: Vec<WifiNet>,
}

/// Something the worker found out, for the app to take over.
#[derive(Debug, Clone, PartialEq)]
pub enum Update {
    Wifi(WifiList),
    Bluetooth(Vec<BtDevice>),
    /// A connect or disconnect finished; `Some` is what went wrong, in plain words.
    Done(Option<String>),
}

enum Cmd {
    /// List the networks; `true` also asks the adapter to scan again first.
    Wifi(bool),
    Bluetooth,
    WifiJoin(String),
    WifiLeave,
    BtConnect(u64),
    BtDisconnect(u64),
}

pub struct Links {
    tx: Sender<Cmd>,
    rx: Receiver<Update>,
}

impl Links {
    pub fn start(waker: Waker) -> Option<Self> {
        let (tx, cmds) = mpsc::channel();
        let (out, rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("links".into())
            .spawn(move || run(&cmds, &out, &waker))
            .map_err(|e| tracing::warn!("no links thread: {e}"))
            .ok()?;
        Some(Self { tx, rx })
    }

    pub fn refresh_wifi(&self, scan: bool) {
        let _ = self.tx.send(Cmd::Wifi(scan));
    }

    pub fn refresh_bluetooth(&self) {
        let _ = self.tx.send(Cmd::Bluetooth);
    }

    pub fn wifi_join(&self, profile: &str) {
        let _ = self.tx.send(Cmd::WifiJoin(profile.to_string()));
    }

    pub fn wifi_leave(&self) {
        let _ = self.tx.send(Cmd::WifiLeave);
    }

    pub fn bt_connect(&self, address: u64) {
        let _ = self.tx.send(Cmd::BtConnect(address));
    }

    pub fn bt_disconnect(&self, address: u64) {
        let _ = self.tx.send(Cmd::BtDisconnect(address));
    }

    pub fn poll(&self) -> Vec<Update> {
        self.rx.try_iter().collect()
    }
}

fn run(cmds: &Receiver<Cmd>, out: &Sender<Update>, waker: &Waker) {
    // SAFETY: COM for this worker thread only; the WinRT calls below block on it.
    let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    while let Ok(first) = cmds.recv() {
        // Refreshes queued while a slow call ran are the same refresh: do each kind once.
        let mut batch = vec![first];
        batch.extend(cmds.try_iter());
        let (mut wifi, mut scan, mut bluetooth) = (false, false, false);
        // Finished switches are reported after the new lists, so a row never shows its old
        // state once its "working" mark is gone.
        let mut done = Vec::new();
        for cmd in batch {
            let result = match cmd {
                Cmd::Wifi(s) => {
                    wifi = true;
                    scan |= s;
                    continue;
                }
                Cmd::Bluetooth => {
                    bluetooth = true;
                    continue;
                }
                Cmd::WifiJoin(profile) => wifi_join(&profile).map(|()| true),
                Cmd::WifiLeave => wifi_leave().map(|()| true),
                Cmd::BtConnect(address) => bt_connect(address).map(|()| false),
                Cmd::BtDisconnect(address) => bt_disconnect(address).map(|()| false),
            };
            let note = match result {
                Ok(is_wifi) => {
                    // The change takes a moment to show in the lists.
                    std::thread::sleep(Duration::from_millis(1500));
                    wifi |= is_wifi;
                    bluetooth |= !is_wifi;
                    None
                }
                Err(e) => {
                    tracing::warn!("connection change failed: {e:#}");
                    Some(format!("{e}"))
                }
            };
            done.push(Update::Done(note));
        }
        if wifi && out.send(Update::Wifi(wifi_list(scan))).is_err() {
            return;
        }
        if bluetooth && out.send(Update::Bluetooth(bluetooth_list())).is_err() {
            return;
        }
        for update in done {
            if out.send(update).is_err() {
                return;
            }
        }
        waker.wake();
    }
}

// ---- Wi-Fi -----------------------------------------------------------------------------------

/// A Native Wifi client handle, closed on drop.
struct Wlan(HANDLE);

impl Wlan {
    fn open() -> Option<Self> {
        let mut version = 0u32;
        let mut handle = HANDLE::default();
        // SAFETY: valid out-pointers; the handle is closed in Drop.
        let status = unsafe { WlanOpenHandle(2, None, &mut version, &mut handle) };
        (status == 0).then_some(Self(handle))
    }

    /// The first Wi-Fi interface, if the PC has one.
    fn first_interface(&self) -> Option<GUID> {
        let mut list: *mut WLAN_INTERFACE_INFO_LIST = std::ptr::null_mut();
        // SAFETY: `list` receives a block owned by the API, freed below.
        let status = unsafe { WlanEnumInterfaces(self.0, None, &mut list) };
        if status != 0 || list.is_null() {
            return None;
        }
        // SAFETY: the block holds `dwNumberOfItems` interface records starting at the array.
        unsafe {
            let n = (*list).dwNumberOfItems as usize;
            let items = std::slice::from_raw_parts((*list).InterfaceInfo.as_ptr(), n);
            let guid = items.first().map(|i| i.InterfaceGuid);
            WlanFreeMemory(list.cast::<c_void>());
            guid
        }
    }
}

impl Drop for Wlan {
    fn drop(&mut self) {
        // SAFETY: closes the handle `open` returned, once.
        unsafe { WlanCloseHandle(self.0, None) };
    }
}

fn utf16_until_nul(chars: &[u16]) -> String {
    let end = chars.iter().position(|&c| c == 0).unwrap_or(chars.len());
    String::from_utf16_lossy(&chars[..end])
}

fn wifi_list(scan: bool) -> WifiList {
    let none = WifiList::default();
    let Some(wlan) = Wlan::open() else {
        return none;
    };
    let Some(guid) = wlan.first_interface() else {
        return none;
    };
    if scan {
        // Asks for a fresh scan; the results show up in the next listing a few seconds later.
        // SAFETY: valid handle and interface; the optional arguments are absent.
        let _ = unsafe { WlanScan(wlan.0, &guid, None, None, None) };
    }
    let mut list: *mut WLAN_AVAILABLE_NETWORK_LIST = std::ptr::null_mut();
    // SAFETY: `list` receives a block owned by the API, freed below.
    let status = unsafe { WlanGetAvailableNetworkList(wlan.0, &guid, 0, None, &mut list) };
    if status != 0 || list.is_null() {
        return WifiList {
            adapter: true,
            nets: Vec::new(),
        };
    }
    // SAFETY: the block holds `dwNumberOfItems` network records starting at the array.
    let nets: Vec<WifiNet> = unsafe {
        let n = (*list).dwNumberOfItems as usize;
        let items = std::slice::from_raw_parts((*list).Network.as_ptr(), n);
        let nets = items
            .iter()
            .map(|n| {
                let len = (n.dot11Ssid.uSSIDLength as usize).min(n.dot11Ssid.ucSSID.len());
                let saved = n.dwFlags & WLAN_AVAILABLE_NETWORK_HAS_PROFILE != 0;
                WifiNet {
                    ssid: String::from_utf8_lossy(&n.dot11Ssid.ucSSID[..len]).into_owned(),
                    signal: n.wlanSignalQuality.min(100) as u8,
                    secure: n.bSecurityEnabled.as_bool(),
                    saved,
                    connected: n.dwFlags & WLAN_AVAILABLE_NETWORK_CONNECTED != 0,
                    profile: if saved {
                        utf16_until_nul(&n.strProfileName)
                    } else {
                        String::new()
                    },
                }
            })
            .collect();
        WlanFreeMemory(list.cast::<c_void>());
        nets
    };
    WifiList {
        adapter: true,
        nets: sort_networks(nets),
    }
}

fn wifi_join(profile: &str) -> Result<()> {
    let wlan = Wlan::open().ok_or_else(|| anyhow!("Wi-Fi is not available"))?;
    let guid = wlan
        .first_interface()
        .ok_or_else(|| anyhow!("No Wi-Fi adapter found"))?;
    let name: Vec<u16> = profile.encode_utf16().chain(std::iter::once(0)).collect();
    let params = WLAN_CONNECTION_PARAMETERS {
        wlanConnectionMode: wlan_connection_mode_profile,
        strProfile: PCWSTR(name.as_ptr()),
        pDot11Ssid: std::ptr::null_mut::<DOT11_SSID>(),
        pDesiredBssidList: std::ptr::null_mut(),
        dot11BssType: dot11_BSS_type_any,
        dwFlags: 0,
    };
    // SAFETY: valid handle, interface and parameters; `name` outlives the call, which copies it.
    let status = unsafe { WlanConnect(wlan.0, &guid, &params, None) };
    if status == 0 {
        Ok(())
    } else {
        Err(anyhow!(
            "Windows could not start joining it (error {status})"
        ))
    }
}

fn wifi_leave() -> Result<()> {
    let wlan = Wlan::open().ok_or_else(|| anyhow!("Wi-Fi is not available"))?;
    let guid = wlan
        .first_interface()
        .ok_or_else(|| anyhow!("No Wi-Fi adapter found"))?;
    // SAFETY: valid handle and interface.
    let status = unsafe { WlanDisconnect(wlan.0, &guid, None) };
    if status == 0 {
        Ok(())
    } else {
        Err(anyhow!("Windows could not disconnect (error {status})"))
    }
}

// ---- Bluetooth -------------------------------------------------------------------------------

fn bluetooth_list() -> Vec<BtDevice> {
    let mut all = Vec::new();
    match classic_devices() {
        Ok(list) => all.extend(list),
        Err(e) => tracing::info!("no classic Bluetooth list: {e:#}"),
    }
    match low_energy_devices() {
        Ok(list) => all.extend(list),
        Err(e) => tracing::info!("no Low Energy Bluetooth list: {e:#}"),
    }
    merge_devices(all)
}

fn classic_devices() -> Result<Vec<BtDevice>> {
    let selector = BluetoothDevice::GetDeviceSelectorFromPairingState(true)?;
    let infos = DeviceInformation::FindAllAsyncAqsFilter(&selector)?.get()?;
    let mut out = Vec::new();
    for info in infos {
        let (Ok(id), Ok(name)) = (info.Id(), info.Name()) else {
            continue;
        };
        let Ok(device) = BluetoothDevice::FromIdAsync(&id).and_then(|op| op.get()) else {
            continue;
        };
        let name = name.to_string();
        let major = device
            .ClassOfDevice()
            .and_then(|c| c.MajorClass())
            .map_or(0, |m| m.0 as u32);
        let mut kind = kind_from_major_class(major);
        if kind == BtKind::Other {
            kind = kind_from_name(&name);
        }
        out.push(BtDevice {
            address: device.BluetoothAddress().unwrap_or(0),
            connected: device.ConnectionStatus() == Ok(BluetoothConnectionStatus::Connected),
            kind,
            classic: true,
            name,
        });
    }
    Ok(out)
}

fn low_energy_devices() -> Result<Vec<BtDevice>> {
    let selector = BluetoothLEDevice::GetDeviceSelectorFromPairingState(true)?;
    let infos = DeviceInformation::FindAllAsyncAqsFilter(&selector)?.get()?;
    let mut out = Vec::new();
    for info in infos {
        let (Ok(id), Ok(name)) = (info.Id(), info.Name()) else {
            continue;
        };
        let Ok(device) = BluetoothLEDevice::FromIdAsync(&id).and_then(|op| op.get()) else {
            continue;
        };
        let name = name.to_string();
        out.push(BtDevice {
            address: device.BluetoothAddress().unwrap_or(0),
            connected: device.ConnectionStatus() == Ok(BluetoothConnectionStatus::Connected),
            kind: kind_from_name(&name),
            classic: false,
            name,
        });
    }
    Ok(out)
}

/// The first Bluetooth radio's handle, closed on drop.
struct Radio {
    handle: HANDLE,
}

impl Radio {
    fn open() -> Result<Self> {
        let params = BLUETOOTH_FIND_RADIO_PARAMS {
            dwSize: size_of::<BLUETOOTH_FIND_RADIO_PARAMS>() as u32,
        };
        let mut handle = HANDLE::default();
        // SAFETY: valid parameter block and out-handle; the search handle is closed right after,
        // the radio handle in Drop.
        unsafe {
            let find = BluetoothFindFirstRadio(&params, &mut handle)
                .map_err(|_| anyhow!("No Bluetooth adapter found"))?;
            let _ = BluetoothFindRadioClose(find);
        }
        Ok(Self { handle })
    }
}

impl Drop for Radio {
    fn drop(&mut self) {
        // SAFETY: closes the radio handle `open` obtained, once.
        let _ = unsafe { CloseHandle(self.handle) };
    }
}

fn bt_disconnect(address: u64) -> Result<()> {
    let radio = Radio::open()?;
    let mut returned = 0u32;
    // SAFETY: the input buffer is the 8 byte address and lives through the call; no output.
    unsafe {
        DeviceIoControl(
            radio.handle,
            IOCTL_BTH_DISCONNECT_DEVICE,
            Some((&raw const address).cast::<c_void>()),
            size_of::<u64>() as u32,
            None,
            0,
            Some(&mut returned),
            None,
        )
    }
    .map_err(|e| anyhow!("Windows would not disconnect it ({e})"))
}

fn bt_connect(address: u64) -> Result<()> {
    let mut info = BLUETOOTH_DEVICE_INFO {
        dwSize: size_of::<BLUETOOTH_DEVICE_INFO>() as u32,
        ..Default::default()
    };
    info.Address.Anonymous.ullLong = address;
    let mut switched = false;
    for service in AUDIO_SERVICES {
        let guid = GUID::from_u128(service);
        // Off and on again makes Windows connect the service. A device without this service
        // answers with an error, which is fine: it just has fewer services.
        // SAFETY: a valid device record and service GUID that live through both calls.
        let on = unsafe {
            let _ = BluetoothSetServiceState(None, &info, &guid, BLUETOOTH_SERVICE_DISABLE);
            std::thread::sleep(Duration::from_millis(150));
            BluetoothSetServiceState(None, &info, &guid, BLUETOOTH_SERVICE_ENABLE)
        };
        switched |= on == 0;
    }
    if switched {
        Ok(())
    } else {
        Err(anyhow!(
            "Windows could not reach the device. Is it on and in range?"
        ))
    }
}
