//! Which app is in front, for the per-app rules. Event-driven: Windows calls us when the
//! foreground window changes, so nothing polls.

use std::collections::BTreeSet;

use anyhow::{Result, anyhow};
use windows::{
    Win32::{
        Foundation::{CloseHandle, HWND, LPARAM},
        System::Threading::{
            OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
            QueryFullProcessImageNameW,
        },
        UI::{
            Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent},
            WindowsAndMessaging::{
                EVENT_SYSTEM_FOREGROUND, EnumWindows, GWL_EXSTYLE, GetForegroundWindow,
                GetWindowLongPtrW, GetWindowTextLengthW, GetWindowThreadProcessId, IsWindowVisible,
                WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS, WS_EX_TOOLWINDOW,
            },
        },
    },
    core::{BOOL, PWSTR},
};

use super::window::{Event, push_event};

extern "system" fn on_foreground(
    _: HWINEVENTHOOK,
    _: u32,
    _: HWND,
    _: i32,
    _: i32,
    _: u32,
    _: u32,
) {
    // Delivered through our own message loop (out-of-context hook), so this runs on the UI thread.
    push_event(Event::ForegroundChanged);
}

/// Keeps the foreground-change hook installed.
pub struct ForegroundHook(HWINEVENTHOOK);

impl ForegroundHook {
    pub fn install() -> Result<Self> {
        // SAFETY: the callback is a 'static function; an out-of-context hook needs no module.
        let hook = unsafe {
            SetWinEventHook(
                EVENT_SYSTEM_FOREGROUND,
                EVENT_SYSTEM_FOREGROUND,
                None,
                Some(on_foreground),
                0,
                0,
                WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
            )
        };
        if hook.is_invalid() {
            return Err(anyhow!("SetWinEventHook failed"));
        }
        Ok(Self(hook))
    }
}

impl Drop for ForegroundHook {
    fn drop(&mut self) {
        // SAFETY: self.0 is the hook installed in `install`, removed exactly once.
        let _ = unsafe { UnhookWinEvent(self.0) };
    }
}

/// Lower-case executable file name of the process that owns a window.
pub(super) fn exe_of(hwnd: HWND) -> Option<String> {
    let mut pid = 0u32;
    // SAFETY: the out-pointer is a valid local; a stale hwnd simply yields pid 0.
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    exe_of_pid(pid)
}

/// Lower-case executable file name of a process, e.g. `claude.exe`; `None` if it is gone or
/// cannot be queried.
pub fn exe_of_pid(pid: u32) -> Option<String> {
    if pid == 0 {
        return None;
    }
    let mut buf = [0u16; 1024];
    let mut len = buf.len() as u32;
    // SAFETY: the process handle is closed before returning; `buf`/`len` describe a valid
    // local buffer that the API fills with at most `len` characters.
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let result = QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(buf.as_mut_ptr()),
            &mut len,
        );
        let _ = CloseHandle(process);
        result.ok()?;
    }
    let path = String::from_utf16_lossy(&buf[..(len as usize).min(buf.len())]);
    let name = path.rsplit(['\\', '/']).next()?.trim().to_lowercase();
    (!name.is_empty()).then_some(name)
}

/// Whether any process runs the executable `exe` (a lower-case file name such as
/// `claude.exe`). One process-list snapshot, about a millisecond; `false` if Windows will not
/// give one.
pub fn process_running(exe: &str) -> bool {
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };
    let mut found = false;
    // SAFETY: the snapshot handle is closed before returning. `entry` is a local with its
    // `dwSize` set as the API requires, and the name is read only up to its first NUL within
    // the fixed-size array.
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return false;
        };
        let mut entry = PROCESSENTRY32W {
            dwSize: size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut more = Process32FirstW(snap, &mut entry).is_ok();
        while more && !found {
            let len = entry
                .szExeFile
                .iter()
                .position(|c| *c == 0)
                .unwrap_or(entry.szExeFile.len());
            found = String::from_utf16_lossy(&entry.szExeFile[..len]).eq_ignore_ascii_case(exe);
            more = Process32NextW(snap, &mut entry).is_ok();
        }
        let _ = CloseHandle(snap);
    }
    found
}

/// The process that owns the window in front, or 0.
pub fn foreground_pid() -> u32 {
    let mut pid = 0u32;
    // SAFETY: the out-pointer is a valid local; a null hwnd simply yields pid 0.
    unsafe { GetWindowThreadProcessId(GetForegroundWindow(), Some(&mut pid)) };
    pid
}

extern "system" fn find_window_of(hwnd: HWND, data: LPARAM) -> BOOL {
    // SAFETY: `data` is the address of the pair in `has_window`, alive for the enumeration.
    let (pid, found) = unsafe { &mut *(data.0 as *mut (u32, bool)) };
    let mut owner = 0u32;
    // SAFETY: hwnd comes from the enumeration; the out-pointer is a valid local.
    let visible = unsafe {
        GetWindowThreadProcessId(hwnd, Some(&mut owner));
        IsWindowVisible(hwnd).as_bool()
    };
    *found = owner == *pid && visible;
    // Stop at the first one.
    BOOL::from(!*found)
}

/// Whether a process has a window that is shown (a minimised one counts). Only window lists
/// are read: the process itself is never opened.
pub fn has_window(pid: u32) -> bool {
    let mut search = (pid, false);
    // SAFETY: the callback only uses the pointer during this call; see find_window_of.
    unsafe {
        // Stopping early makes this return an error, which means nothing here.
        let _ = EnumWindows(
            Some(find_window_of),
            LPARAM(&mut search as *mut (u32, bool) as isize),
        );
    }
    search.1
}

/// The executable currently in the foreground, e.g. `chrome.exe`.
pub fn foreground_exe() -> Option<String> {
    // SAFETY: no arguments; may return a null handle, which exe_of handles.
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.is_invalid() {
        return None;
    }
    exe_of(hwnd)
}

extern "system" fn collect_window(hwnd: HWND, data: LPARAM) -> BOOL {
    // SAFETY: `data` is the address of the set in `open_apps`, alive for the whole enumeration.
    let apps = unsafe { &mut *(data.0 as *mut BTreeSet<String>) };
    // SAFETY: hwnd comes from the enumeration and is valid for these queries.
    let wanted = unsafe {
        IsWindowVisible(hwnd).as_bool()
            && GetWindowTextLengthW(hwnd) > 0
            && (GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32 & WS_EX_TOOLWINDOW.0) == 0
    };
    if wanted && let Some(exe) = exe_of(hwnd) {
        apps.insert(exe);
    }
    BOOL(1)
}

/// Executables that currently have a visible, titled window: what the Settings window offers
/// when adding an app rule.
pub fn open_apps() -> Vec<String> {
    let mut apps: BTreeSet<String> = BTreeSet::new();
    // SAFETY: the callback only uses the pointer during this call; see collect_window.
    unsafe {
        let _ = EnumWindows(
            Some(collect_window),
            LPARAM(&mut apps as *mut BTreeSet<String> as isize),
        );
    }
    apps.into_iter().collect()
}
