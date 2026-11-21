//! Notification-area icon and its menu. Lives on its own hidden window so Pillbox window
//! can stay strictly no-activate (a popup menu needs a foreground owner to dismiss properly).

use std::cell::Cell;

use anyhow::{Context, Result, anyhow};
use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM},
        UI::{
            Shell::{
                NIF_ICON, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
                Shell_NotifyIconW,
            },
            WindowsAndMessaging::{
                AppendMenuW, CreateIconFromResourceEx, CreatePopupMenu, CreateWindowExW,
                DefWindowProcW, DestroyIcon, DestroyMenu, DestroyWindow, GetCursorPos,
                GetSystemMetrics, HICON, LR_DEFAULTCOLOR, MF_CHECKED, MF_SEPARATOR, MF_STRING,
                PostMessageW, RegisterClassW, RegisterWindowMessageW, SM_CXSMICON,
                SetForegroundWindow, TPM_BOTTOMALIGN, TPM_RIGHTBUTTON, TrackPopupMenu,
                WINDOW_EX_STYLE, WM_APP, WM_COMMAND, WM_CONTEXTMENU, WM_LBUTTONUP, WM_NULL,
                WM_RBUTTONUP, WNDCLASSW, WS_OVERLAPPED,
            },
        },
    },
    core::w,
};

use super::{
    system,
    window::{Event, TrayCommand, module_instance, push_event},
};

const WM_TRAY: u32 = WM_APP + 10;
const ICON_ID: u32 = 1;

// Menu command identifiers. Also accepted as plain WM_COMMAND, which is how tests drive them.
const CMD_SETTINGS: usize = 100;
const CMD_TOGGLE_VISIBLE: usize = 101;
const CMD_AUTOSTART: usize = 102;
const CMD_QUIT: usize = 103;
const CMD_SYSTEM: usize = 104;
// Developer commands, only ever sent programmatically.
const CMD_DUMP_FRAME: usize = 200;
const CMD_FORCE_OPEN: usize = 201;
const CMD_FORCE_CLOSE: usize = 202;

thread_local! {
    static PILLBOX_VISIBLE: Cell<bool> = const { Cell::new(true) };
    static SYSTEM_OPEN: Cell<bool> = const { Cell::new(false) };
    static TASKBAR_CREATED: Cell<u32> = const { Cell::new(0) };
    static ICON: Cell<HICON> = const { Cell::new(HICON(std::ptr::null_mut())) };
}

/// Tell the menu which label to show ("Hide Pillbox" or "Show Pillbox").
pub fn set_pillbox_visible(visible: bool) {
    PILLBOX_VISIBLE.set(visible);
}

/// Tell the menu whether the System window is open (its item is checked).
pub fn set_system_window_open(open: bool) {
    SYSTEM_OPEN.set(open);
}

fn icon_data(hwnd: HWND) -> NOTIFYICONDATAW {
    let mut nid = NOTIFYICONDATAW {
        cbSize: size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: ICON_ID,
        uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP | NIF_SHOWTIP,
        uCallbackMessage: WM_TRAY,
        hIcon: ICON.get(),
        ..Default::default()
    };
    for (dst, src) in nid.szTip.iter_mut().zip("Pillbox".encode_utf16()) {
        *dst = src;
    }
    nid
}

fn add_icon(hwnd: HWND) -> bool {
    // SAFETY: the struct is fully initialised with its size and a live window handle.
    unsafe { Shell_NotifyIconW(NIM_ADD, &icon_data(hwnd)) }.as_bool()
}

fn show_menu(hwnd: HWND) {
    // SAFETY: the menu handle is created and destroyed here; all strings are 'static; hwnd is
    // the live tray window. SetForegroundWindow + the trailing WM_NULL are the documented way
    // to make a tray menu close when the user clicks elsewhere.
    unsafe {
        let Ok(menu) = CreatePopupMenu() else { return };
        let visible_label = if PILLBOX_VISIBLE.get() {
            w!("Hide Pillbox")
        } else {
            w!("Show Pillbox")
        };
        let autostart = if system::autostart_enabled() {
            MF_CHECKED
        } else {
            MF_STRING
        };
        let _ = AppendMenuW(menu, MF_STRING, CMD_SETTINGS, w!("Settings"));
        let _ = AppendMenuW(menu, MF_STRING, CMD_TOGGLE_VISIBLE, visible_label);
        let system = if SYSTEM_OPEN.get() {
            MF_CHECKED
        } else {
            MF_STRING
        };
        let _ = AppendMenuW(menu, MF_STRING | system, CMD_SYSTEM, w!("System window"));
        let _ = AppendMenuW(
            menu,
            MF_STRING | autostart,
            CMD_AUTOSTART,
            w!("Start with Windows"),
        );
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
        let _ = AppendMenuW(menu, MF_STRING, CMD_QUIT, w!("Quit"));
        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        let _ = SetForegroundWindow(hwnd);
        let _ = TrackPopupMenu(
            menu,
            TPM_RIGHTBUTTON | TPM_BOTTOMALIGN,
            pt.x,
            pt.y,
            None,
            hwnd,
            None,
        );
        let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(menu);
    }
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_TRAY => {
            let mouse = lp.0 as u32;
            if mouse == WM_RBUTTONUP || mouse == WM_LBUTTONUP || mouse == WM_CONTEXTMENU {
                show_menu(hwnd);
            }
            LRESULT(0)
        }
        WM_COMMAND => {
            match wp.0 & 0xFFFF {
                CMD_SETTINGS => push_event(Event::Tray(TrayCommand::Settings)),
                CMD_DUMP_FRAME => push_event(Event::Tray(TrayCommand::DumpFrame)),
                CMD_FORCE_OPEN => push_event(Event::Tray(TrayCommand::ForceOpen(true))),
                CMD_FORCE_CLOSE => push_event(Event::Tray(TrayCommand::ForceOpen(false))),
                // 210..218: show the home, music, lyrics, clipboard, shelf, reminders,
                // controls, activity or notifications tab.
                id @ 210..=219 => push_event(Event::Tray(TrayCommand::ShowTab((id - 210) as u8))),
                // Developer aid: a click at window pixel (x, y), packed into lParam as
                // x | y << 16. It goes straight into Pillbox's own event queue, so tests
                // can press buttons without moving the real pointer or touching other apps.
                230 => {
                    let (x, y) = ((lp.0 & 0xFFFF) as f32, ((lp.0 >> 16) & 0xFFFF) as f32);
                    push_event(Event::MouseMove { x, y });
                    push_event(Event::Click { x, y });
                    push_event(Event::MouseUp { x, y });
                }
                // Developer aids for the hover and pressed looks: the pointer moved to (x, y)
                // only (235), or pressed there and still held (234: no button-up follows).
                id @ (234 | 235) => {
                    let (x, y) = ((lp.0 & 0xFFFF) as f32, ((lp.0 >> 16) & 0xFFFF) as f32);
                    push_event(Event::MouseMove { x, y });
                    if id == 234 {
                        push_event(Event::Click { x, y });
                    }
                }
                // Developer aids: one notch of the wheel over Pillbox, down (232) or up (233),
                // at window pixel x | y << 16 (0 for the top-left corner).
                id @ (232 | 233) => push_event(Event::Wheel {
                    x: (lp.0 & 0xFFFF) as f32,
                    y: ((lp.0 >> 16) & 0xFFFF) as f32,
                    delta: if id == 232 { -1.0 } else { 1.0 },
                }),
                // Developer aid: the same, as a right-click.
                231 => {
                    let (x, y) = ((lp.0 & 0xFFFF) as f32, ((lp.0 >> 16) & 0xFFFF) as f32);
                    push_event(Event::MouseMove { x, y });
                    push_event(Event::RightClick { x, y });
                }
                CMD_TOGGLE_VISIBLE => push_event(Event::Tray(TrayCommand::ToggleVisible)),
                CMD_AUTOSTART => push_event(Event::Tray(TrayCommand::ToggleAutostart)),
                CMD_SYSTEM => push_event(Event::Tray(TrayCommand::ToggleSystemWindow)),
                CMD_QUIT => push_event(Event::Tray(TrayCommand::Quit)),
                _ => {}
            }
            LRESULT(0)
        }
        // Explorer restarted: the notification area is new, so the icon must be added again.
        m if m != 0 && m == TASKBAR_CREATED.get() => {
            if !add_icon(hwnd) {
                tracing::warn!("could not re-add the tray icon after Explorer restarted");
            }
            LRESULT(0)
        }
        // SAFETY: forwards the arguments this procedure was called with, unchanged.
        _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
}

/// The app icon (see `pillbox_core::appicon`), at the size the notification area asks for.
fn make_icon() -> Result<HICON> {
    // SAFETY: a plain metrics query.
    let side = unsafe { GetSystemMetrics(SM_CXSMICON) }.clamp(16, 64);
    let bits = pillbox_core::appicon::dib(side as u32);
    // SAFETY: `bits` is a complete icon image (header, colour rows, mask) of the size passed;
    // the call copies it. The icon is destroyed in Drop.
    unsafe {
        CreateIconFromResourceEx(&bits, true, 0x0003_0000, side, side, LR_DEFAULTCOLOR)
            .context("creating the tray icon")
    }
}

pub struct Tray {
    hwnd: HWND,
    icon: HICON,
}

impl Tray {
    pub fn new() -> Result<Self> {
        let hinst = module_instance()?;
        let icon = make_icon()?;
        ICON.set(icon);
        let class = w!("Pillbox.Tray");
        // SAFETY: class name and wndproc are 'static; the hidden window is owned by Self and
        // destroyed in Drop.
        let hwnd = unsafe {
            TASKBAR_CREATED.set(RegisterWindowMessageW(w!("TaskbarCreated")));
            let wc = WNDCLASSW {
                lpfnWndProc: Some(wndproc),
                hInstance: hinst,
                lpszClassName: class,
                ..Default::default()
            };
            if RegisterClassW(&wc) == 0 {
                return Err(anyhow!("RegisterClassW failed for the tray window"));
            }
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                class,
                w!("Pillbox tray"),
                WS_OVERLAPPED,
                0,
                0,
                0,
                0,
                None,
                None,
                Some(hinst),
                None,
            )
            .context("creating the tray window")?
        };
        let tray = Self { hwnd, icon };
        if !add_icon(hwnd) {
            // Explorer may not be running yet at sign-in; TaskbarCreated adds it later.
            tracing::warn!("tray icon not added yet (no notification area?)");
        }
        Ok(tray)
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        // SAFETY: removes the icon we added and destroys the window and icon we created, once.
        unsafe {
            let _ = Shell_NotifyIconW(NIM_DELETE, &icon_data(self.hwnd));
            let _ = DestroyWindow(self.hwnd);
            let _ = DestroyIcon(self.icon);
        }
    }
}
