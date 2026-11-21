//! The Settings window's frame: it has no title bar of its own (it draws one), so Windows is
//! asked for rounded corners and a dark frame for it.

use windows::{
    Win32::{
        Foundation::{HWND, LPARAM},
        Graphics::Dwm::{
            DWM_WINDOW_CORNER_PREFERENCE, DWMWA_USE_IMMERSIVE_DARK_MODE,
            DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DwmSetWindowAttribute,
        },
        System::Threading::GetCurrentThreadId,
        UI::WindowsAndMessaging::{EnumThreadWindows, GetWindowTextW, IsWindowVisible},
    },
    core::BOOL,
};

struct Search {
    title: Vec<u16>,
    found: Option<HWND>,
}

extern "system" fn match_title(hwnd: HWND, data: LPARAM) -> BOOL {
    // SAFETY: `data` is the address of the Search in `find_own_window`, which outlives the
    // enumeration and is used by nothing else while it runs.
    let search = unsafe { &mut *(data.0 as *mut Search) };
    let mut buf = [0u16; 128];
    // SAFETY: `buf` is a valid, writable buffer of the length passed.
    let len = unsafe { GetWindowTextW(hwnd, &mut buf) };
    let len = usize::try_from(len).unwrap_or(0).min(buf.len());
    if buf[..len] == search.title[..] {
        search.found = Some(hwnd);
        return BOOL(0); // stop
    }
    BOOL(1)
}

/// The visible top-level window of this thread with the given title.
fn find_own_window(title: &str) -> Option<HWND> {
    let mut search = Search {
        title: title.encode_utf16().collect(),
        found: None,
    };
    // SAFETY: the callback only uses the pointer during this call; see match_title.
    unsafe {
        let _ = EnumThreadWindows(
            GetCurrentThreadId(),
            Some(match_title),
            LPARAM(&mut search as *mut Search as isize),
        );
    }
    // SAFETY: IsWindowVisible only reads the window's state.
    search
        .found
        .filter(|hwnd| unsafe { IsWindowVisible(*hwnd) }.as_bool())
}

/// Ask Windows 11 for rounded corners and a dark frame on this thread's window `title`. True
/// once the window was found (it may not exist before its first frame); harmless to repeat, and
/// on Windows 10 the requests are simply ignored.
pub fn style(title: &str) -> bool {
    let Some(hwnd) = find_own_window(title) else {
        return false;
    };
    let round = DWMWCP_ROUND;
    let dark = BOOL(1);
    // SAFETY: `hwnd` is a live window of this thread; each pointer is to a local of exactly the
    // size passed, as the attributes document.
    unsafe {
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            (&round as *const DWM_WINDOW_CORNER_PREFERENCE).cast(),
            size_of_val(&round) as u32,
        );
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            (&dark as *const BOOL).cast(),
            size_of_val(&dark) as u32,
        );
    }
    true
}
