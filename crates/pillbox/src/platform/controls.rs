//! Brightness and radios for the Controls tab, on one worker thread (both APIs can take
//! tens of milliseconds per call, and DDC/CI monitors even longer).
//!
//! - Built-in panels: WMI `root\wmi` `WmiMonitorBrightness` / `WmiMonitorBrightnessMethods`.
//! - External monitors: DDC/CI through `GetMonitorBrightness` / `SetMonitorBrightness`
//!   (only monitors that have DDC/CI switched on in their own menu answer).
//! - Wi-Fi and Bluetooth: `Windows.Devices.Radios`, the same switches as Quick Settings.

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::Duration;

use anyhow::{Result, anyhow};
use windows::{
    Devices::Radios::{Radio, RadioKind, RadioState},
    Win32::{
        Devices::Display::{
            DestroyPhysicalMonitors, GetMonitorBrightness, GetNumberOfPhysicalMonitorsFromHMONITOR,
            GetPhysicalMonitorsFromHMONITOR, PHYSICAL_MONITOR, SetMonitorBrightness,
        },
        Foundation::{LPARAM, RECT},
        Graphics::Gdi::{EnumDisplayMonitors, HDC, HMONITOR},
        System::{
            Com::{CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx},
            Variant::VARIANT,
            Wmi::{
                IWbemClassObject, IWbemLocator, IWbemServices, WBEM_FLAG_FORWARD_ONLY,
                WBEM_FLAG_RETURN_IMMEDIATELY, WBEM_INFINITE, WbemLocator,
            },
        },
    },
    core::{BOOL, BSTR, w},
};

use super::window::Waker;

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct State {
    /// 0..1; `None` when no display answers.
    pub brightness: Option<f32>,
    pub wifi: Option<bool>,
    pub bluetooth: Option<bool>,
}

enum Cmd {
    Refresh,
    Brightness(f32),
    Wifi(bool),
    Bluetooth(bool),
}

pub struct Controls {
    tx: Sender<Cmd>,
    rx: Receiver<State>,
}

impl Controls {
    pub fn start(waker: Waker) -> Option<Self> {
        let (tx, cmds) = mpsc::channel();
        let (state_tx, rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("controls".into())
            .spawn(move || run(&cmds, &state_tx, waker))
            .map_err(|e| tracing::warn!("no controls thread: {e}"))
            .ok()?;
        Some(Self { tx, rx })
    }

    pub fn refresh(&self) {
        let _ = self.tx.send(Cmd::Refresh);
    }

    pub fn set_brightness(&self, level: f32) {
        let _ = self.tx.send(Cmd::Brightness(level.clamp(0.0, 1.0)));
    }

    pub fn set_wifi(&self, on: bool) {
        let _ = self.tx.send(Cmd::Wifi(on));
    }

    pub fn set_bluetooth(&self, on: bool) {
        let _ = self.tx.send(Cmd::Bluetooth(on));
    }

    pub fn poll(&self) -> Option<State> {
        self.rx.try_iter().last()
    }
}

fn run(cmds: &Receiver<Cmd>, out: &Sender<State>, waker: Waker) {
    // SAFETY: COM for this worker thread only.
    let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    let wmi = Wmi::connect()
        .map_err(|e| tracing::info!("built-in display brightness unavailable: {e:#}"))
        .ok();
    loop {
        let Ok(first) = cmds.recv() else { return };
        // Slider drags send many values; only the newest brightness matters.
        let mut batch = vec![first];
        loop {
            match cmds.recv_timeout(Duration::from_millis(30)) {
                Ok(c) => batch.push(c),
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
        let level = batch.iter().rev().find_map(|c| match c {
            Cmd::Brightness(l) => Some(*l),
            _ => None,
        });
        if let Some(level) = level {
            set_brightness(wmi.as_ref(), level);
        }
        for c in &batch {
            let result = match c {
                Cmd::Wifi(on) => set_radio(RadioKind::WiFi, *on),
                Cmd::Bluetooth(on) => set_radio(RadioKind::Bluetooth, *on),
                _ => Ok(()),
            };
            if let Err(e) = result {
                tracing::warn!("radio switch failed: {e:#}");
            }
        }
        let (wifi, bluetooth) = radios();
        let state = State {
            brightness: get_brightness(wmi.as_ref()),
            wifi,
            bluetooth,
        };
        if out.send(state).is_err() {
            return;
        }
        waker.wake();
    }
}

// ---- radios ----------------------------------------------------------------------------------

fn radios() -> (Option<bool>, Option<bool>) {
    let mut wifi = None;
    let mut bt = None;
    let Ok(list) = Radio::GetRadiosAsync().and_then(|op| op.get()) else {
        return (None, None);
    };
    for radio in list {
        let (Ok(kind), Ok(state)) = (radio.Kind(), radio.State()) else {
            continue;
        };
        let on = state == RadioState::On;
        if kind == RadioKind::WiFi {
            wifi = Some(wifi.unwrap_or(false) || on);
        } else if kind == RadioKind::Bluetooth {
            bt = Some(bt.unwrap_or(false) || on);
        }
    }
    (wifi, bt)
}

fn set_radio(kind: RadioKind, on: bool) -> Result<()> {
    let list = Radio::GetRadiosAsync()?.get()?;
    let mut any = false;
    for radio in list {
        if radio.Kind()? == kind {
            let state = if on { RadioState::On } else { RadioState::Off };
            let status = radio.SetStateAsync(state)?.get()?;
            tracing::info!("radio {kind:?} -> {on}: {status:?}");
            any = true;
        }
    }
    if any {
        Ok(())
    } else {
        Err(anyhow!("no such radio"))
    }
}

// ---- brightness ------------------------------------------------------------------------------

struct Wmi {
    server: IWbemServices,
}

impl Wmi {
    fn connect() -> Result<Self> {
        // SAFETY: COM is initialised on this thread; all BSTRs live through the calls.
        let server = unsafe {
            let locator: IWbemLocator = CoCreateInstance(&WbemLocator, None, CLSCTX_INPROC_SERVER)?;
            locator.ConnectServer(
                &BSTR::from("ROOT\\WMI"),
                &BSTR::new(),
                &BSTR::new(),
                &BSTR::new(),
                0,
                &BSTR::new(),
                None,
            )?
        };
        let wmi = Self { server };
        // Fails on PCs without a built-in panel.
        wmi.first("SELECT CurrentBrightness FROM WmiMonitorBrightness")?;
        Ok(wmi)
    }

    fn first(&self, query: &str) -> Result<IWbemClassObject> {
        // SAFETY: valid BSTR arguments; the enumerator and row are owned COM objects.
        unsafe {
            let rows = self.server.ExecQuery(
                &BSTR::from("WQL"),
                &BSTR::from(query),
                WBEM_FLAG_FORWARD_ONLY | WBEM_FLAG_RETURN_IMMEDIATELY,
                None,
            )?;
            let mut row = [None; 1];
            let mut got = 0;
            rows.Next(WBEM_INFINITE, &mut row, &mut got).ok()?;
            row[0].take().ok_or_else(|| anyhow!("no rows for {query}"))
        }
    }

    fn brightness(&self) -> Result<f32> {
        let row = self.first("SELECT CurrentBrightness FROM WmiMonitorBrightness")?;
        let mut v = VARIANT::default();
        // SAFETY: `v` is a valid out-VARIANT owned here.
        unsafe { row.Get(w!("CurrentBrightness"), 0, &mut v, None, None)? };
        let level = i32::try_from(&v).map_err(|e| anyhow!("CurrentBrightness: {e}"))?;
        Ok(level.clamp(0, 100) as f32 / 100.0)
    }

    fn set(&self, level: f32) -> Result<()> {
        let methods = self.first("SELECT * FROM WmiMonitorBrightnessMethods")?;
        let mut path = VARIANT::default();
        // SAFETY: out-VARIANTs and out-objects are owned locals; strings are 'static.
        unsafe {
            methods.Get(w!("__PATH"), 0, &mut path, None, None)?;
            let path = BSTR::try_from(&path).map_err(|e| anyhow!("instance path: {e}"))?;
            let mut class = None;
            self.server.GetObject(
                &BSTR::from("WmiMonitorBrightnessMethods"),
                Default::default(),
                None,
                Some(&mut class),
                None,
            )?;
            let class: IWbemClassObject = class.ok_or_else(|| anyhow!("no class"))?;
            let mut in_def = None;
            class.GetMethod(w!("WmiSetBrightness"), 0, &mut in_def, std::ptr::null_mut())?;
            let in_def = in_def.ok_or_else(|| anyhow!("no method signature"))?;
            let params = in_def.SpawnInstance(0)?;
            params.Put(w!("Timeout"), 0, &VARIANT::from(0i32), 0)?;
            let value = (level.clamp(0.0, 1.0) * 100.0).round() as u8;
            params.Put(w!("Brightness"), 0, &VARIANT::from(value), 0)?;
            self.server.ExecMethod(
                &path,
                &BSTR::from("WmiSetBrightness"),
                Default::default(),
                None,
                &params,
                None,
                None,
            )?;
        }
        Ok(())
    }
}

extern "system" fn collect(hmon: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> BOOL {
    // SAFETY: `data` points at the Vec owned by `ddc_monitors` for the enumeration's duration.
    let list = unsafe { &mut *(data.0 as *mut Vec<HMONITOR>) };
    list.push(hmon);
    BOOL(1)
}

/// Physical monitors that speak DDC/CI; destroyed again by the caller.
fn ddc_monitors() -> Vec<PHYSICAL_MONITOR> {
    let mut handles: Vec<HMONITOR> = Vec::new();
    let mut out = Vec::new();
    // SAFETY: the callback writes only into `handles` during this call. Physical monitor
    // arrays are sized from the count the API reports.
    unsafe {
        let _ = EnumDisplayMonitors(
            None,
            None,
            Some(collect),
            LPARAM(&mut handles as *mut Vec<HMONITOR> as isize),
        );
        for h in handles {
            let mut n = 0u32;
            if GetNumberOfPhysicalMonitorsFromHMONITOR(h, &mut n).is_err() || n == 0 {
                continue;
            }
            let mut list = vec![PHYSICAL_MONITOR::default(); n as usize];
            if GetPhysicalMonitorsFromHMONITOR(h, &mut list).is_ok() {
                out.extend(list);
            }
        }
    }
    out
}

fn with_ddc<T>(f: impl Fn(&PHYSICAL_MONITOR) -> Option<T>) -> Vec<T> {
    let list = ddc_monitors();
    let out = list.iter().filter_map(&f).collect();
    // SAFETY: destroys exactly the handles GetPhysicalMonitorsFromHMONITOR gave us.
    let _ = unsafe { DestroyPhysicalMonitors(&list) };
    out
}

fn ddc_get(m: &PHYSICAL_MONITOR) -> Option<f32> {
    let (mut min, mut cur, mut max) = (0u32, 0u32, 0u32);
    // SAFETY: a live physical monitor handle and valid out-pointers.
    let ok = unsafe { GetMonitorBrightness(m.hPhysicalMonitor, &mut min, &mut cur, &mut max) };
    (ok != 0 && max > min).then(|| (cur.saturating_sub(min)) as f32 / (max - min) as f32)
}

fn ddc_set(m: &PHYSICAL_MONITOR, level: f32) -> Option<()> {
    let (mut min, mut cur, mut max) = (0u32, 0u32, 0u32);
    // SAFETY: a live physical monitor handle and valid out-pointers.
    unsafe {
        if GetMonitorBrightness(m.hPhysicalMonitor, &mut min, &mut cur, &mut max) == 0 || max <= min
        {
            return None;
        }
        let value = min + ((max - min) as f32 * level).round() as u32;
        (SetMonitorBrightness(m.hPhysicalMonitor, value) != 0).then_some(())
    }
}

fn get_brightness(wmi: Option<&Wmi>) -> Option<f32> {
    if let Some(level) = wmi.and_then(|w| w.brightness().ok()) {
        return Some(level);
    }
    with_ddc(ddc_get).into_iter().next()
}

fn set_brightness(wmi: Option<&Wmi>, level: f32) {
    if let Some(w) = wmi
        && let Err(e) = w.set(level)
    {
        tracing::warn!("built-in display brightness: {e:#}");
    }
    let n = with_ddc(|m| ddc_set(m, level)).len();
    if n > 0 {
        tracing::debug!("set brightness on {n} external monitor(s)");
    }
}
