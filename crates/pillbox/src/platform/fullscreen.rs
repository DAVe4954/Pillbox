//! Is a fullscreen app (usually a game) in front, on Pillbox's monitor?
//!
//! Two documented signals are combined:
//! * `SHQueryUserNotificationState` says `QUNS_RUNNING_D3D_FULL_SCREEN` while a Direct3D app
//!   owns the screen in exclusive fullscreen. Nothing can draw over that mode anyway.
//! * Borderless ("windowed fullscreen") games are ordinary windows, so the shell reports at
//!   most `QUNS_BUSY` for them, the same as for a browser in F11 or a video player. For those
//!   the foreground window itself is checked: its *client* area covers its whole monitor. A
//!   maximised window does not count, because its title bar is outside the client area.
//!
//! Whether such a window is a game cannot be known for certain from outside the process; see
//! `pillbox_core::games::NOT_GAMES` and the "every fullscreen app" setting.

use windows::Win32::{
    Foundation::{HWND, POINT, RECT},
    Graphics::Gdi::{
        ClientToScreen, GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow,
    },
    System::Threading::GetCurrentProcessId,
    UI::{
        Shell::{QUNS_RUNNING_D3D_FULL_SCREEN, SHQueryUserNotificationState},
        WindowsAndMessaging::{
            GetClassNameW, GetClientRect, GetForegroundWindow, GetWindowThreadProcessId, IsIconic,
            IsWindowVisible,
        },
    },
};

use super::foreground;

/// A fullscreen window in front.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FullscreenApp {
    /// Lower-case executable name, when it could be read.
    pub exe: Option<String>,
    /// Direct3D exclusive fullscreen (the shell said so), as opposed to a borderless window.
    pub exclusive: bool,
}

/// Shell windows that cover the screen but are never "an app in front".
const SHELL_CLASSES: &[&str] = &[
    "Progman",
    "WorkerW",
    "Shell_TrayWnd",
    "Shell_SecondaryTrayWnd",
    "Windows.UI.Core.CoreWindow",
    "XamlExplorerHostIslandWindow",
];

fn class_name(hwnd: HWND) -> String {
    let mut buf = [0u16; 128];
    // SAFETY: buf is a valid local buffer; the API writes at most its length.
    let n = unsafe { GetClassNameW(hwnd, &mut buf) };
    String::from_utf16_lossy(&buf[..(n.max(0) as usize).min(buf.len())])
}

/// The fullscreen app in front on monitor `pillbox_monitor` (an HMONITOR value), if any.
pub fn detect(pillbox_monitor: isize) -> Option<FullscreenApp> {
    // SAFETY: no arguments; the out-value is a plain enum written on success.
    let exclusive =
        unsafe { SHQueryUserNotificationState() }.is_ok_and(|s| s == QUNS_RUNNING_D3D_FULL_SCREEN);
    // SAFETY: no arguments; may be null, checked next.
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.is_invalid() {
        return exclusive.then_some(FullscreenApp {
            exe: None,
            exclusive,
        });
    }
    let mut pid = 0u32;
    // SAFETY: valid out-pointer; a stale hwnd yields 0.
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    // SAFETY: no arguments.
    if pid == unsafe { GetCurrentProcessId() } {
        return None;
    }
    if SHELL_CLASSES.contains(&class_name(hwnd).as_str()) {
        return None;
    }
    let exe = foreground::exe_of(hwnd);
    if exclusive {
        return Some(FullscreenApp { exe, exclusive });
    }
    // SAFETY: hwnd came from GetForegroundWindow; all out-structs are valid locals.
    let covers = unsafe {
        if !IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool() {
            return None;
        }
        let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        if monitor.0 as isize != pillbox_monitor {
            return None;
        }
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return None;
        }
        let mut client = RECT::default();
        if GetClientRect(hwnd, &mut client).is_err() {
            return None;
        }
        let mut origin = POINT::default();
        if !ClientToScreen(hwnd, &mut origin).as_bool() {
            return None;
        }
        let m = info.rcMonitor;
        origin.x <= m.left
            && origin.y <= m.top
            && origin.x + client.right >= m.right
            && origin.y + client.bottom >= m.bottom
    };
    covers.then_some(FullscreenApp { exe, exclusive })
}
