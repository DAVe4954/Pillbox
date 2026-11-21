//! Which apps are using the microphone or camera right now.
//!
//! UNDOCUMENTED: this reads the registry data behind Windows' own privacy indicators,
//! `HKCU\Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\
//! {microphone,webcam}`. Each app that used a device has a subkey (desktop apps under
//! `NonPackaged`, named after the exe path with `\` replaced by `#`) holding
//! `LastUsedTimeStart` and `LastUsedTimeStop`; a stop time of 0 means "in use now". The layout
//! is not a published API and could change in a Windows update; then the dot and call timer
//! simply stop appearing. Nothing polls: a thread waits on registry change notifications.

use std::{
    sync::mpsc::{self, Receiver, Sender},
    thread::JoinHandle,
};

use windows::{
    Win32::{
        Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0},
        System::{
            Registry::{
                HKEY, HKEY_CURRENT_USER, KEY_NOTIFY, KEY_READ, REG_NOTIFY_CHANGE_LAST_SET,
                REG_NOTIFY_CHANGE_NAME, RegCloseKey, RegNotifyChangeKeyValue, RegOpenKeyExW,
            },
            Threading::{CreateEventW, INFINITE, SetEvent, WaitForMultipleObjects},
        },
    },
    core::HSTRING,
};

use super::window::Waker;

const BASE: &str =
    r"Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InUse {
    /// Friendly names of apps using the microphone.
    pub mic: Vec<String>,
    pub camera: Vec<String>,
}

/// Turn a ConsentStore subkey name into something readable:
/// `C:#Program Files#Zoom#bin#Zoom.exe` → `zoom`, `Microsoft.WindowsCamera_8wekyb3d8bbwe` →
/// `windowscamera`.
pub fn friendly(key: &str) -> String {
    let last = key.rsplit('#').next().unwrap_or(key);
    let stem = last
        .strip_suffix(".exe")
        .or_else(|| last.strip_suffix(".EXE"))
        .unwrap_or(last);
    let stem = stem.split('_').next().unwrap_or(stem);
    let stem = stem.rsplit('.').next().unwrap_or(stem);
    stem.to_lowercase()
}

fn active_in(device: &str) -> Vec<String> {
    let mut out = Vec::new();
    let Ok(root) = windows_registry::CURRENT_USER.open(format!(r"{BASE}\{device}")) else {
        return out;
    };
    let mut check = |key: &windows_registry::Key, name: &str| {
        let start = key.get_u64("LastUsedTimeStart").unwrap_or(0);
        let stop = key.get_u64("LastUsedTimeStop").unwrap_or(1);
        if start != 0 && stop == 0 {
            out.push(friendly(name));
        }
    };
    if let Ok(names) = root.keys() {
        for name in names {
            if name.eq_ignore_ascii_case("NonPackaged") {
                if let Ok(np) = root.open(&name)
                    && let Ok(inner) = np.keys()
                {
                    for app in inner {
                        if let Ok(k) = np.open(&app) {
                            check(&k, &app);
                        }
                    }
                }
            } else if let Ok(k) = root.open(&name) {
                check(&k, &name);
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

pub fn read() -> InUse {
    InUse {
        mic: active_in("microphone"),
        camera: active_in("webcam"),
    }
}

/// Raw registry key opened for change notifications; closed on drop.
struct NotifyKey(HKEY);

impl NotifyKey {
    fn open(path: &str) -> Option<Self> {
        let mut key = HKEY::default();
        let path = HSTRING::from(path);
        // SAFETY: path is a valid wide string; key is a valid out-pointer; closed in Drop.
        let r = unsafe {
            RegOpenKeyExW(
                HKEY_CURRENT_USER,
                &path,
                Some(0),
                KEY_NOTIFY | KEY_READ,
                &mut key,
            )
        };
        r.is_ok().then_some(Self(key))
    }

    /// Ask for `event` to be signalled on the next change anywhere below this key.
    fn arm(&self, event: HANDLE) -> bool {
        // SAFETY: live key and event handles; asynchronous mode with an event is documented.
        unsafe {
            RegNotifyChangeKeyValue(
                self.0,
                true,
                REG_NOTIFY_CHANGE_LAST_SET | REG_NOTIFY_CHANGE_NAME,
                Some(event),
                true,
            )
        }
        .is_ok()
    }
}

impl Drop for NotifyKey {
    fn drop(&mut self) {
        // SAFETY: the key was opened in `open` and is closed once.
        let _ = unsafe { RegCloseKey(self.0) };
    }
}

struct Event(HANDLE);

impl Event {
    fn new() -> Option<Self> {
        // SAFETY: an unnamed auto-reset event; the handle is closed in Drop.
        unsafe { CreateEventW(None, false, false, None) }
            .ok()
            .map(Self)
    }
}

impl Drop for Event {
    fn drop(&mut self) {
        // SAFETY: closed once.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

// SAFETY: an event handle is a kernel object; signalling and waiting on it from any thread
// is what events are for, and the handle value itself is never mutated after creation.
unsafe impl Send for Event {}
// SAFETY: as above; shared references only signal or wait.
unsafe impl Sync for Event {}

/// Watches the two registry keys on a thread and sends each new state.
pub struct SensorWatch {
    rx: Receiver<InUse>,
    stop: std::sync::Arc<Event>,
    thread: Option<JoinHandle<()>>,
}

impl SensorWatch {
    pub fn start(waker: Waker) -> Option<Self> {
        let stop = std::sync::Arc::new(Event::new()?);
        let (tx, rx) = mpsc::channel();
        let stop_for_thread = stop.clone();
        let thread = std::thread::Builder::new()
            .name("sensors".into())
            .spawn(move || watch(&tx, &stop_for_thread, waker))
            .map_err(|e| tracing::warn!("could not start the sensor watcher: {e}"))
            .ok()?;
        Some(Self {
            rx,
            stop,
            thread: Some(thread),
        })
    }

    /// The newest state, if it changed since the last call.
    pub fn poll(&self) -> Option<InUse> {
        self.rx.try_iter().last()
    }
}

impl Drop for SensorWatch {
    fn drop(&mut self) {
        // SAFETY: live event handle owned by the Arc.
        let _ = unsafe { SetEvent(self.stop.0) };
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn watch(tx: &Sender<InUse>, stop: &Event, waker: Waker) {
    let keys: Vec<NotifyKey> = ["microphone", "webcam"]
        .iter()
        .filter_map(|d| NotifyKey::open(&format!(r"{BASE}\{d}")))
        .collect();
    let Some(changed) = Event::new() else { return };
    if keys.is_empty() {
        tracing::warn!("mic/camera indicator unavailable: ConsentStore keys not found");
        return;
    }
    let mut last = InUse::default();
    loop {
        for k in &keys {
            k.arm(changed.0);
        }
        let now = read();
        if now != last {
            last = now.clone();
            if tx.send(now).is_err() {
                return;
            }
            waker.wake();
        }
        // SAFETY: both handles are live for the duration of the wait.
        let woke = unsafe { WaitForMultipleObjects(&[stop.0, changed.0], false, INFINITE) };
        if woke == WAIT_OBJECT_0 {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::friendly;

    #[test]
    fn friendly_names() {
        assert_eq!(friendly(r"C:#Program Files#Zoom#bin#Zoom.exe"), "zoom");
        assert_eq!(
            friendly("Microsoft.WindowsCamera_8wekyb3d8bbwe"),
            "windowscamera"
        );
        assert_eq!(friendly("MSTeams_8wekyb3d8bbwe"), "msteams");
        assert_eq!(
            friendly(r"C:#Users#me#AppData#Local#Discord#app-1.0#Discord.exe"),
            "discord"
        );
    }
}
