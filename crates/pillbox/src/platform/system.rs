//! Process-level helpers: DPI awareness, single instance, autostart, monitors, clock, battery,
//! CPU load, the frame timer and the settings-folder watcher.

use std::{path::Path, time::Duration};

use anyhow::{Context, Result, anyhow};
use pillbox_core::{face::LocalTime, shape::RectI};
use windows::{
    Win32::{
        Foundation::{
            CloseHandle, ERROR_ALREADY_EXISTS, FILETIME, GetLastError, HANDLE, LPARAM, RECT,
        },
        Graphics::Gdi::{
            EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO, MONITORINFOEXW,
        },
        Storage::FileSystem::{
            FILE_NOTIFY_CHANGE_FILE_NAME, FILE_NOTIFY_CHANGE_LAST_WRITE, FILE_NOTIFY_CHANGE_SIZE,
            FindCloseChangeNotification, FindFirstChangeNotificationW, FindNextChangeNotification,
        },
        System::{
            Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS},
            SystemInformation::GetLocalTime,
            Threading::{CreateMutexW, CreateWaitableTimerExW, GetSystemTimes, SetWaitableTimer},
        },
        UI::HiDpi::{
            DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForMonitor, MDT_EFFECTIVE_DPI,
            SetProcessDpiAwarenessContext,
        },
    },
    core::{BOOL, HSTRING, w},
};

/// Make the UI thread an OLE single-threaded apartment. Needed for drag and drop and used by
/// the audio-endpoint and shell calls made from the UI thread.
pub fn init_com() {
    // SAFETY: called once on the UI thread before any COM use there; balanced by process exit.
    if let Err(e) = unsafe { windows::Win32::System::Ole::OleInitialize(None) } {
        tracing::warn!("OleInitialize failed: {e}");
    }
}

/// Windows' "Animation effects" (Settings > Accessibility > Visual effects). Off means the
/// user asked for less motion; animated themes then stand still. Assumed on if unreadable.
pub fn animations_enabled() -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{
        SPI_GETCLIENTAREAANIMATION, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW,
    };
    let mut on = BOOL(1);
    // SAFETY: SPI_GETCLIENTAREAANIMATION writes one BOOL into the local it points at.
    let read = unsafe {
        SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            Some(std::ptr::from_mut(&mut on).cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    };
    read.is_err() || on.as_bool()
}

/// Open a file, folder, program or URL the way Explorer would.
pub fn open_path(path: &str, args: &str) -> Result<()> {
    use windows::Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL};
    let file = HSTRING::from(path);
    let params = HSTRING::from(args);
    // SAFETY: both strings outlive the call; no owner window; the return value is an
    // HINSTANCE-typed status code (> 32 = success), not a handle we own.
    let r = unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            &file,
            if args.is_empty() {
                windows::core::PCWSTR::null()
            } else {
                windows::core::PCWSTR(params.as_ptr())
            },
            windows::core::PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
    if (r.0 as isize) <= 32 {
        return Err(anyhow!("could not open {path} (code {})", r.0 as isize));
    }
    Ok(())
}

/// Start an app by its app id (AUMID), as the Start menu would. `false` when Windows has no
/// Start-menu entry for it (some notification sources are not apps: system components, or a
/// program that names itself) or the launch failed.
pub fn open_app(app_id: &str) -> bool {
    use windows::Win32::UI::Shell::{IShellItem, SHCreateItemFromParsingName};
    let target = format!("shell:AppsFolder\\{app_id}");
    let wide = HSTRING::from(target.as_str());
    // SAFETY: `wide` is a valid NUL-terminated string that outlives the call; COM is initialised
    // on the UI thread.
    let known = unsafe { SHCreateItemFromParsingName::<_, _, IShellItem>(&wide, None) }.is_ok();
    known && open_path(&target, "").is_ok()
}

/// Windows' "How do you want to open this file?" dialog for `path`, on its own thread (the
/// dialog is modal until the user picks an app or cancels).
pub fn open_with(path: std::path::PathBuf) {
    use windows::Win32::{
        System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize},
        UI::Shell::{
            OAIF_ALLOW_REGISTRATION, OAIF_EXEC, OAIF_REGISTER_EXT, OPENASINFO, SHOpenWithDialog,
        },
    };
    let spawned = std::thread::Builder::new()
        .name("open-with".into())
        .spawn(move || {
            let file = HSTRING::from(path.as_os_str());
            let info = OPENASINFO {
                pcszFile: windows::core::PCWSTR(file.as_ptr()),
                pcszClass: windows::core::PCWSTR::null(),
                oaifInFlags: OAIF_ALLOW_REGISTRATION | OAIF_REGISTER_EXT | OAIF_EXEC,
            };
            // SAFETY: COM is set up for this thread and torn down once after the dialog; `file`
            // outlives the call that reads `info`.
            unsafe {
                let init = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
                if let Err(e) = SHOpenWithDialog(None, &info) {
                    // Cancelling the dialog is not an error worth logging loudly.
                    tracing::debug!("open with: {e}");
                }
                if init.is_ok() {
                    CoUninitialize();
                }
            }
        });
    if let Err(e) = spawned {
        tracing::warn!("could not start the Open with dialog: {e}");
    }
}

/// Windows' Save dialog with `name` filled in, starting in `folder`. `Ok(None)` if cancelled.
/// Blocks until the dialog closes, so call it on a worker thread.
pub fn save_dialog(name: &str, folder: Option<&std::path::Path>) -> Result<Option<String>> {
    use windows::Win32::{
        Foundation::ERROR_CANCELLED,
        System::Com::{
            CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
            CoTaskMemFree, CoUninitialize,
        },
        UI::Shell::{
            FOS_FORCEFILESYSTEM, FOS_OVERWRITEPROMPT, FileSaveDialog, IFileSaveDialog, IShellItem,
            SHCreateItemFromParsingName, SIGDN_FILESYSPATH,
        },
    };
    // SAFETY: COM is initialised for this thread and uninitialised once at the end, after the
    // dialog and items (dropped inside the block) are gone. The returned path string is
    // allocated by the shell, copied, and freed exactly once with CoTaskMemFree.
    unsafe {
        let init = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let result = (|| -> Result<Option<String>> {
            let dlg: IFileSaveDialog =
                CoCreateInstance(&FileSaveDialog, None, CLSCTX_INPROC_SERVER)
                    .context("creating the Save dialog")?;
            dlg.SetFileName(&HSTRING::from(name))?;
            if let Some(folder) = folder
                && let Ok(item) = SHCreateItemFromParsingName::<_, _, IShellItem>(
                    &HSTRING::from(folder.as_os_str()),
                    None,
                )
            {
                dlg.SetFolder(&item)?;
            }
            dlg.SetOptions(dlg.GetOptions()? | FOS_OVERWRITEPROMPT | FOS_FORCEFILESYSTEM)?;
            if let Err(e) = dlg.Show(None) {
                if e.code() == ERROR_CANCELLED.to_hresult() {
                    return Ok(None);
                }
                return Err(e).context("showing the Save dialog");
            }
            let item = dlg.GetResult()?;
            let p = item.GetDisplayName(SIGDN_FILESYSPATH)?;
            let s = p.to_string();
            CoTaskMemFree(Some(p.0 as *const _));
            Ok(Some(s.context("the chosen path is not valid text")?))
        })();
        if init.is_ok() {
            CoUninitialize();
        }
        result
    }
}

/// Opt in to per-monitor DPI awareness. Must run before any window is created.
pub fn enable_dpi_awareness() {
    // SAFETY: takes a constant, has no pointer arguments.
    if let Err(e) =
        unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) }
    {
        tracing::warn!("could not set per-monitor DPI awareness: {e}");
    }
}

/// Holds a named mutex for the lifetime of the process.
pub struct SingleInstance(HANDLE);

impl SingleInstance {
    /// `Ok(None)` means another copy holding the same name is already running.
    pub fn acquire(settings_window: bool) -> Result<Option<Self>> {
        let name = if settings_window {
            w!("Local\\Pillbox.Settings.SingleInstance")
        } else {
            w!("Local\\Pillbox.SingleInstance")
        };
        // SAFETY: the name is a valid 'static wide string; the returned handle is owned by Self
        // and closed exactly once in Drop. GetLastError is read immediately after the call.
        unsafe {
            let handle =
                CreateMutexW(None, true, name).context("creating the single-instance mutex")?;
            if GetLastError() == ERROR_ALREADY_EXISTS {
                let _ = CloseHandle(handle);
                return Ok(None);
            }
            Ok(Some(Self(handle)))
        }
    }
}

impl Drop for SingleInstance {
    fn drop(&mut self) {
        // SAFETY: self.0 is the valid handle created in acquire and not closed elsewhere.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const RUN_VALUE: &str = "Pillbox";

/// The command line Windows runs at sign-in.
fn autostart_command() -> Result<String> {
    let exe = std::env::current_exe().context("locating the executable")?;
    Ok(format!("\"{}\"", exe.display()))
}

/// Windows keeps its own on/off switch for Run entries (Settings > Apps > Startup, Task
/// Manager > Startup apps). An entry there with an odd first byte means "disabled": Windows
/// then skips the Run value even though it is present.
const APPROVED_KEY: &str =
    r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";

/// True if the `StartupApproved` bytes say "disabled" (odd first byte, as Windows writes 03).
fn approval_says_disabled(bytes: &[u8]) -> bool {
    bytes.first().is_some_and(|b| b & 1 == 1)
}

/// True if Windows' own startup switch has turned Pillbox off.
fn disabled_by_windows() -> bool {
    windows_registry::CURRENT_USER
        .open(APPROVED_KEY)
        .and_then(|key| key.get_value(RUN_VALUE))
        .map(|v| approval_says_disabled(&v))
        .unwrap_or(false)
}

/// True if "Start with Windows" points at this executable and Windows has not switched it off.
pub fn autostart_enabled() -> bool {
    let Ok(expected) = autostart_command() else {
        return false;
    };
    windows_registry::CURRENT_USER
        .open(RUN_KEY)
        .and_then(|key| key.get_string(RUN_VALUE))
        .map(|current| current.eq_ignore_ascii_case(&expected))
        .unwrap_or(false)
        && !disabled_by_windows()
}

pub fn set_autostart(enabled: bool) -> Result<()> {
    let key = windows_registry::CURRENT_USER
        .create(RUN_KEY)
        .map_err(|e| anyhow!("opening the Run key: {e}"))?;
    if enabled {
        key.set_string(RUN_VALUE, &autostart_command()?)
            .map_err(|e| anyhow!("writing the Run value: {e}"))?;
        // Turning it on here must beat an earlier "off" in Windows' Startup apps list.
        if disabled_by_windows()
            && let Ok(approved) = windows_registry::CURRENT_USER.create(APPROVED_KEY)
        {
            approved
                .remove_value(RUN_VALUE)
                .map_err(|e| anyhow!("clearing Windows' startup switch: {e}"))?;
        }
    } else if key.get_string(RUN_VALUE).is_ok() {
        key.remove_value(RUN_VALUE)
            .map_err(|e| anyhow!("removing the Run value: {e}"))?;
    }
    Ok(())
}

/// Pillbox was called Island before: a "Start with Windows" entry under the old name is
/// replaced by one for this executable, so it keeps starting at sign-in. Does nothing when
/// there is no old entry.
pub fn migrate_autostart() {
    const OLD_VALUE: &str = "Island";
    let Ok(key) = windows_registry::CURRENT_USER.create(RUN_KEY) else {
        return;
    };
    if key.get_string(OLD_VALUE).is_err() {
        return;
    }
    match autostart_command().and_then(|cmd| {
        key.set_string(RUN_VALUE, &cmd)
            .map_err(|e| anyhow!("writing the Run value: {e}"))
    }) {
        Ok(()) => {
            let _ = key.remove_value(OLD_VALUE);
            tracing::info!("start with Windows moved from the old name to Pillbox");
        }
        Err(e) => tracing::warn!("could not move the old start-with-Windows entry: {e:#}"),
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Monitor {
    /// Display device name, e.g. `\\.\DISPLAY1`. This is what the settings file stores.
    pub name: String,
    pub rect: RectI,
    /// The scale Pillbox is drawn at: the DPI scale (1.0 = 100%), times `layout.size` once
    /// the app has read it (see `app::read_monitor`).
    pub scale: f32,
    /// The DPI scale alone, for windows that do not grow with Pillbox's size (the chat
    /// drawer, the System window, popped-out widgets).
    pub dpi: f32,
    pub primary: bool,
    /// The HMONITOR value, for screen capture.
    pub handle: isize,
}

extern "system" fn collect_monitor(hmon: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> BOOL {
    // SAFETY: `data` is the address of the Vec in `monitors`, which outlives the enumeration
    // and is not touched by anything else while EnumDisplayMonitors runs.
    let list = unsafe { &mut *(data.0 as *mut Vec<HMONITOR>) };
    list.push(hmon);
    BOOL(1)
}

/// Every connected monitor, primary first.
pub fn monitors() -> Vec<Monitor> {
    let mut handles: Vec<HMONITOR> = Vec::new();
    // SAFETY: the callback only uses the pointer during this call; see collect_monitor.
    unsafe {
        let _ = EnumDisplayMonitors(
            None,
            None,
            Some(collect_monitor),
            LPARAM(&mut handles as *mut Vec<HMONITOR> as isize),
        );
    }
    let mut out: Vec<Monitor> = handles
        .into_iter()
        .filter_map(|hmon| {
            let mut info = MONITORINFOEXW::default();
            info.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;
            // SAFETY: MONITORINFOEXW starts with a MONITORINFO and cbSize says which one it is,
            // as the API documents; the out-pointers are valid locals.
            let (ok, dpi) = unsafe {
                let ok =
                    GetMonitorInfoW(hmon, &mut info as *mut MONITORINFOEXW as *mut MONITORINFO)
                        .as_bool();
                let (mut dx, mut dy) = (96u32, 96u32);
                if GetDpiForMonitor(hmon, MDT_EFFECTIVE_DPI, &mut dx, &mut dy).is_err() {
                    dx = 96;
                }
                (ok, dx)
            };
            if !ok {
                return None;
            }
            let len = info
                .szDevice
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(info.szDevice.len());
            let rc = info.monitorInfo.rcMonitor;
            Some(Monitor {
                name: String::from_utf16_lossy(&info.szDevice[..len]),
                rect: RectI {
                    left: rc.left,
                    top: rc.top,
                    right: rc.right,
                    bottom: rc.bottom,
                },
                scale: dpi as f32 / 96.0,
                dpi: dpi as f32 / 96.0,
                primary: info.monitorInfo.dwFlags & 1 != 0, // MONITORINFOF_PRIMARY
                handle: hmon.0 as isize,
            })
        })
        .collect();
    out.sort_by_key(|m| !m.primary);
    out
}

/// The monitor named in settings ("primary" or a device name); falls back to the primary one
/// when that display is not connected.
pub fn pick_monitor(wanted: &str) -> Result<Monitor> {
    let all = monitors();
    all.iter()
        .find(|m| m.name.eq_ignore_ascii_case(wanted))
        .or_else(|| all.iter().find(|m| m.primary))
        .or_else(|| all.first())
        .cloned()
        .ok_or_else(|| anyhow!("no monitors found"))
}

pub fn local_time() -> LocalTime {
    // SAFETY: no arguments; returns a plain struct by value.
    let t = unsafe { GetLocalTime() };
    LocalTime {
        year: t.wYear,
        month: t.wMonth as u8,
        day: t.wDay as u8,
        weekday: t.wDayOfWeek as u8,
        hour: t.wHour as u8,
        minute: t.wMinute as u8,
        second: t.wSecond as u8,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Battery {
    pub percent: u8,
    pub plugged_in: bool,
}

/// `None` on a PC without a battery.
pub fn battery() -> Option<Battery> {
    let mut status = SYSTEM_POWER_STATUS::default();
    // SAFETY: the out-pointer is a valid local struct.
    unsafe { GetSystemPowerStatus(&mut status) }.ok()?;
    // Flag 128 = no system battery; 255 = unknown.
    if status.BatteryFlag & 128 != 0 || status.BatteryFlag == 255 || status.BatteryLifePercent > 100
    {
        return None;
    }
    Some(Battery {
        percent: status.BatteryLifePercent,
        plugged_in: status.ACLineStatus == 1,
    })
}

/// Whole-system CPU load, measured between two calls.
#[derive(Default)]
pub struct CpuMeter {
    last: Option<(u64, u64)>,
}

impl CpuMeter {
    /// Load since the previous call, 0..100. `None` on the first call.
    pub fn sample(&mut self) -> Option<f32> {
        let as_u64 = |f: FILETIME| (u64::from(f.dwHighDateTime) << 32) | u64::from(f.dwLowDateTime);
        let (mut idle, mut kernel, mut user) = (
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
        );
        // SAFETY: the three out-pointers are valid locals.
        unsafe { GetSystemTimes(Some(&mut idle), Some(&mut kernel), Some(&mut user)) }.ok()?;
        // Kernel time includes idle time.
        let now = (as_u64(idle), as_u64(kernel) + as_u64(user));
        let previous = self.last.replace(now)?;
        let total = now.1.checked_sub(previous.1).filter(|t| *t > 0)?;
        let idle = now.0.saturating_sub(previous.0).min(total);
        Some((1.0 - idle as f32 / total as f32) * 100.0)
    }
}

/// High-resolution waitable timer used to pace animation frames without raising the global
/// timer resolution.
pub struct FrameTimer(HANDLE);

impl FrameTimer {
    pub fn new() -> Result<Self> {
        const CREATE_WAITABLE_TIMER_HIGH_RESOLUTION: u32 = 0x2;
        const TIMER_ALL_ACCESS: u32 = 0x1F_0003;
        // SAFETY: no name or security attributes; the handle is owned by Self and closed in Drop.
        let high = unsafe {
            CreateWaitableTimerExW(
                None,
                None,
                CREATE_WAITABLE_TIMER_HIGH_RESOLUTION,
                TIMER_ALL_ACCESS,
            )
        };
        let handle = match high {
            Ok(h) => h,
            Err(e) => {
                // Windows 10 before version 1803 has no high-resolution timers: a normal one
                // still paces frames, only a little less evenly.
                tracing::info!("high-resolution frame timer unavailable ({e}); using a normal one");
                // SAFETY: as above, without the high-resolution flag.
                unsafe { CreateWaitableTimerExW(None, None, 0, TIMER_ALL_ACCESS) }
                    .context("creating the frame timer")?
            }
        };
        Ok(Self(handle))
    }

    /// Arm the timer to fire once after `delay`.
    pub fn arm(&self, delay: Duration) -> Result<()> {
        // Negative due time = relative, in 100 ns units. Never zero, so the wait always yields.
        let due: i64 = -((delay.as_nanos() / 100).clamp(1, i64::MAX as u128) as i64);
        // SAFETY: self.0 is a valid timer handle; `due` outlives the call; no completion routine.
        unsafe { SetWaitableTimer(self.0, &due, 0, None, None, false) }
            .context("arming the frame timer")
    }

    pub(super) fn handle(&self) -> HANDLE {
        self.0
    }
}

impl Drop for FrameTimer {
    fn drop(&mut self) {
        // SAFETY: self.0 is the valid handle created in new and not closed elsewhere.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

/// Signals when anything in a folder is written. Used to hot-reload settings.json without a
/// polling loop or an extra thread: the main loop simply waits on this handle too.
pub struct DirWatcher(HANDLE);

impl DirWatcher {
    pub fn new(dir: &Path) -> Result<Self> {
        let path = HSTRING::from(dir.as_os_str());
        // SAFETY: `path` is a valid NUL-terminated wide string that outlives the call; the
        // handle is owned by Self and closed in Drop.
        let handle = unsafe {
            FindFirstChangeNotificationW(
                &path,
                false,
                FILE_NOTIFY_CHANGE_LAST_WRITE
                    | FILE_NOTIFY_CHANGE_FILE_NAME
                    | FILE_NOTIFY_CHANGE_SIZE,
            )
        }
        .with_context(|| format!("watching {}", dir.display()))?;
        Ok(Self(handle))
    }

    /// Must be called after each signal to keep receiving them.
    pub fn rearm(&self) {
        // SAFETY: self.0 is the live change-notification handle.
        if let Err(e) = unsafe { FindNextChangeNotification(self.0) } {
            tracing::warn!("settings watcher could not re-arm: {e}");
        }
    }

    pub(super) fn handle(&self) -> HANDLE {
        self.0
    }
}

impl Drop for DirWatcher {
    fn drop(&mut self) {
        // SAFETY: self.0 is the handle from FindFirstChangeNotificationW, closed exactly once.
        let _ = unsafe { FindCloseChangeNotification(self.0) };
    }
}

#[cfg(test)]
mod tests {
    use super::approval_says_disabled;

    #[test]
    fn startup_approved_bytes_are_read_like_windows_does() {
        assert!(approval_says_disabled(&[0x03, 0, 0, 0])); // switched off
        assert!(approval_says_disabled(&[0x01, 0, 0, 0]));
        assert!(!approval_says_disabled(&[0x02, 0, 0, 0])); // switched on
        assert!(!approval_says_disabled(&[0x06, 0, 0, 0]));
        assert!(!approval_says_disabled(&[])); // nothing there
    }
}
