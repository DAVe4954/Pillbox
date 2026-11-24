//! Pillbox's host window and the message pump.
//!
//! The window procedure never touches application state. It translates messages into `Event`s
//! and queues them; the main loop drains the queue. That keeps the app free of raw pointers.

use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    ffi::c_void,
};

use anyhow::{Context, Result, anyhow};
use pillbox_core::shape::RegionKey;
use windows::{
    Win32::{
        Foundation::{HANDLE, HINSTANCE, HWND, LPARAM, LRESULT, POINT, WAIT_OBJECT_0, WPARAM},
        Graphics::Gdi::{CreateRoundRectRgn, ScreenToClient, SetWindowRgn},
        System::{
            DataExchange::{AddClipboardFormatListener, RemoveClipboardFormatListener},
            LibraryLoader::GetModuleHandleW,
            Threading::{GetCurrentProcessId, INFINITE},
        },
        UI::{
            Input::{
                GetRawInputData, HRAWINPUT,
                KeyboardAndMouse::{
                    GetKeyState, HOT_KEY_MODIFIERS, MOD_NOREPEAT, RegisterHotKey, ReleaseCapture,
                    SetCapture, SetFocus, TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent,
                    UnregisterHotKey, VK_CONTROL, VK_SHIFT,
                },
                RAWINPUT, RAWINPUTDEVICE, RAWINPUTHEADER, RID_INPUT, RIDEV_INPUTSINK, RIDEV_REMOVE,
                RIM_TYPEKEYBOARD, RegisterRawInputDevices,
            },
            WindowsAndMessaging::{
                CreateWindowExW, DBT_DEVICEARRIVAL, DBT_DEVICEREMOVECOMPLETE, DBT_DEVTYP_VOLUME,
                DBTF_NET, DEV_BROADCAST_HDR, DEV_BROADCAST_VOLUME, DefWindowProcW, DestroyWindow,
                DispatchMessageW, GetForegroundWindow, GetWindowThreadProcessId, HWND_TOPMOST,
                IDC_ARROW, IsWindow, IsWindowVisible, KillTimer, LoadCursorW, MA_NOACTIVATE, MSG,
                MsgWaitForMultipleObjects, PM_REMOVE, PeekMessageW, PostMessageW, PostQuitMessage,
                QS_ALLINPUT, RegisterClassW, SW_HIDE, SW_SHOWNOACTIVATE, SWP_NOACTIVATE,
                SetForegroundWindow, SetTimer, SetWindowDisplayAffinity, SetWindowPos, ShowWindow,
                TranslateMessage, WDA_EXCLUDEFROMCAPTURE, WDA_NONE, WM_APP, WM_CHAR,
                WM_CLIPBOARDUPDATE, WM_DESTROY, WM_DEVICECHANGE, WM_DISPLAYCHANGE, WM_DPICHANGED,
                WM_HOTKEY, WM_INPUT, WM_KEYDOWN, WM_KILLFOCUS, WM_LBUTTONDOWN, WM_LBUTTONUP,
                WM_MOUSEACTIVATE, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_POWERBROADCAST, WM_QUIT,
                WM_RBUTTONUP, WM_SETTINGCHANGE, WM_TIMER, WNDCLASSW, WS_EX_NOACTIVATE,
                WS_EX_NOREDIRECTIONBITMAP, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
            },
        },
    },
    core::w,
};

use super::system::{DirWatcher, FrameTimer};

const WM_MOUSELEAVE: u32 = 0x02A3;
/// Posted by the capture thread when a new backdrop frame is waiting.
pub(super) const WM_APP_BACKDROP: u32 = WM_APP + 1;
/// Posted by worker threads that have a result ready.
const WM_APP_WAKE: u32 = WM_APP + 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayCommand {
    Settings,
    ToggleVisible,
    ToggleAutostart,
    /// Open or close the System window (tray menu).
    ToggleSystemWindow,
    /// Developer aid: save the next frame to the path in `PILLBOX_DUMP`.
    DumpFrame,
    /// Developer aid: open or close as if the pointer had rested on / left Pillbox.
    ForceOpen(bool),
    /// Developer aid: switch the card to tab 0 (home), 1 (music) or 2 (lyrics).
    ShowTab(u8),
    Quit,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Event {
    /// Pointer moved over Pillbox; window pixel coordinates.
    MouseMove {
        x: f32,
        y: f32,
    },
    MouseLeave,
    /// Left button pressed on Pillbox.
    Click {
        x: f32,
        y: f32,
    },
    /// Left button released (possibly outside Pillbox, while dragging).
    MouseUp {
        x: f32,
        y: f32,
    },
    /// Right button released on Pillbox.
    RightClick {
        x: f32,
        y: f32,
    },
    /// A lock key (Caps, Num or Scroll Lock) went down or up, anywhere in Windows.
    LockKey {
        vk: u16,
        down: bool,
    },
    Timer(usize),
    /// DPI, resolution, monitor layout or work area changed.
    DisplayChanged,
    /// Battery level or power source changed.
    PowerChanged,
    /// Drives were plugged in or removed: Windows' bit mask of their letters.
    Drives {
        mask: u32,
        connected: bool,
    },
    /// A different app came to the front.
    ForegroundChanged,
    /// A new capture frame of the screen behind Pillbox is waiting.
    BackdropFrame,
    /// A worker thread finished something.
    Wake,
    /// A registered global shortcut was pressed; the id given at registration.
    Hotkey(i32),
    /// The clipboard contents changed (any app, including us).
    ClipboardChanged,
    /// The mouse wheel turned over Pillbox; positive = away from the user. Window pixels.
    Wheel {
        x: f32,
        y: f32,
        delta: f32,
    },
    /// A character was typed while a text box in Pillbox has the keyboard.
    Char(char),
    /// A non-character key went down while a text box has the keyboard (virtual-key code).
    Key {
        vk: u16,
        ctrl: bool,
        shift: bool,
    },
    /// Pillbox lost the keyboard (the user clicked another window).
    FocusLost,
    /// Files are being dragged over Pillbox.
    DragEnter,
    DragLeave,
    /// Files were dropped; collect them with `dragdrop::take_dropped`.
    FilesDropped,
    /// A popped-out widget window was moved (its window handle value).
    WidgetMoved(isize),
    /// A widget window changed size (the System window's right edge was dragged).
    WidgetResized(isize),
    /// A popped-out widget window was right-clicked: close it.
    WidgetClose(isize),
    /// A button in a popped-out widget window was clicked (window id, client pixels).
    WidgetClick {
        id: isize,
        x: f32,
        y: f32,
    },
    Tray(TrayCommand),
}

thread_local! {
    static EVENTS: RefCell<VecDeque<Event>> = const { RefCell::new(VecDeque::new()) };
    static TRACKING_LEAVE: Cell<bool> = const { Cell::new(false) };
    static HIGH_SURROGATE: Cell<u16> = const { Cell::new(0) };
}

pub(super) fn push_event(event: Event) {
    EVENTS.with_borrow_mut(|q| q.push_back(event));
}

/// Take every queued event, oldest first.
pub fn drain_events() -> Vec<Event> {
    EVENTS.with_borrow_mut(|q| q.drain(..).collect())
}

pub(super) fn module_instance() -> Result<HINSTANCE> {
    // SAFETY: None asks for the handle of the current executable, which is always valid.
    Ok(unsafe { GetModuleHandleW(None) }
        .context("GetModuleHandleW")?
        .into())
}

fn point(lp: LPARAM) -> (f32, f32) {
    // Client coordinates are signed 16-bit values packed into the low and high words.
    let x = (lp.0 & 0xFFFF) as u16 as i16;
    let y = ((lp.0 >> 16) & 0xFFFF) as u16 as i16;
    (f32::from(x), f32::from(y))
}

/// Typing into a text box: turn WM_CHAR, WM_KEYDOWN and WM_KILLFOCUS into events. Shared by the
/// Pillbox and the desktop widget windows. Returns false for any other message.
pub(super) fn keyboard_message(msg: u32, wp: WPARAM) -> bool {
    match msg {
        WM_CHAR => {
            let unit = (wp.0 & 0xFFFF) as u16;
            // Characters outside the basic plane arrive as two messages (a surrogate pair).
            if (0xD800..0xDC00).contains(&unit) {
                HIGH_SURROGATE.set(unit);
            } else {
                let units = match HIGH_SURROGATE.replace(0) {
                    0 => vec![unit],
                    high => vec![high, unit],
                };
                for c in char::decode_utf16(units).flatten() {
                    // Control characters (Enter, Backspace, Ctrl+letters) come as keys instead.
                    if !c.is_control() {
                        push_event(Event::Char(c));
                    }
                }
            }
            true
        }
        WM_KEYDOWN => {
            // SAFETY: no pointers; reads this thread's key state for the message being handled.
            let (ctrl, shift) = unsafe {
                (
                    GetKeyState(i32::from(VK_CONTROL.0)) < 0,
                    GetKeyState(i32::from(VK_SHIFT.0)) < 0,
                )
            };
            push_event(Event::Key {
                vk: wp.0 as u16,
                ctrl,
                shift,
            });
            true
        }
        WM_KILLFOCUS => {
            push_event(Event::FocusLost);
            true
        }
        _ => false,
    }
}

/// Developer aid for automated tests: `PILLBOX_NO_FOCUS` stops Pillbox from ever changing
/// the foreground window, so tests can post typed characters without disturbing other apps.
fn no_focus() -> bool {
    std::env::var_os("PILLBOX_NO_FOCUS").is_some()
}

/// Whether a window belongs to this process (Pillbox or one of its widget windows).
fn is_ours(hwnd: HWND) -> bool {
    let mut pid = 0u32;
    // SAFETY: valid out-pointer; a null or stale hwnd yields pid 0.
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    // SAFETY: no arguments.
    pid != 0 && pid == unsafe { GetCurrentProcessId() }
}

/// Give `hwnd` the keyboard. `from` remembers the other app's window that had it, to hand it
/// back later; moving between Pillbox's own windows keeps the original. Only works right
/// after the user clicked `hwnd`: Windows lets the process that received the last input take
/// the foreground.
pub(super) fn take_keyboard_for(hwnd: HWND, from: &mut Option<isize>) -> bool {
    if no_focus() {
        tracing::debug!("keyboard not taken (PILLBOX_NO_FOCUS)");
        return false;
    }
    // SAFETY: plain window-handle calls on live handles; no pointers are passed.
    unsafe {
        let previous = GetForegroundWindow();
        if !previous.is_invalid() && !is_ours(previous) {
            *from = Some(previous.0 as isize);
        }
        let ok = SetForegroundWindow(hwnd).as_bool();
        let _ = SetFocus(Some(hwnd));
        tracing::debug!("keyboard taken from {from:?}: {ok}");
        ok
    }
}

/// Hand the keyboard back to the window remembered by `take_keyboard_for`, if one of the
/// Pillbox's windows still has it.
pub(super) fn give_back_keyboard_for(from: &mut Option<isize>) {
    if no_focus() {
        return;
    }
    let Some(raw) = from.take() else {
        tracing::debug!("keyboard: nothing to give back to");
        return;
    };
    // SAFETY: no arguments.
    if !is_ours(unsafe { GetForegroundWindow() }) {
        // The user already moved on to another window; leave it there.
        return;
    }
    let previous = HWND(raw as *mut c_void);
    // SAFETY: IsWindow tolerates stale handles; we are the foreground process, which is
    // what allows SetForegroundWindow to succeed.
    let ok =
        unsafe { IsWindow(Some(previous)).as_bool() && SetForegroundWindow(previous).as_bool() };
    tracing::debug!("keyboard given back to {raw:#x}: {ok}");
}

/// Drives plugged in or removed, from `WM_DEVICECHANGE`. Network drives do not count.
fn drive_event(what: u32, lp: LPARAM) -> Option<Event> {
    let connected = match what {
        DBT_DEVICEARRIVAL => true,
        DBT_DEVICEREMOVECOMPLETE => false,
        _ => return None,
    };
    let header = lp.0 as *const DEV_BROADCAST_HDR;
    if header.is_null() {
        return None;
    }
    // SAFETY: for these two codes Windows passes a DEV_BROADCAST_HDR that is valid during the
    // message; the larger structure is read only when its type and size say it is there.
    let volume = unsafe {
        let h = header.read_unaligned();
        if h.dbch_devicetype != DBT_DEVTYP_VOLUME
            || (h.dbch_size as usize) < size_of::<DEV_BROADCAST_VOLUME>()
        {
            return None;
        }
        (lp.0 as *const DEV_BROADCAST_VOLUME).read_unaligned()
    };
    let network = volume.dbcv_flags.0 & DBTF_NET.0 != 0;
    (!network).then_some(Event::Drives {
        mask: volume.dbcv_unitmask,
        connected,
    })
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        // A click must never activate Pillbox or take focus from the app being used.
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_MOUSEMOVE => {
            if !TRACKING_LEAVE.get() {
                let mut tme = TRACKMOUSEEVENT {
                    cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: hwnd,
                    dwHoverTime: 0,
                };
                // SAFETY: tme is fully initialised and outlives the call.
                if unsafe { TrackMouseEvent(&mut tme) }.is_ok() {
                    TRACKING_LEAVE.set(true);
                }
            }
            let (x, y) = point(lp);
            push_event(Event::MouseMove { x, y });
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            TRACKING_LEAVE.set(false);
            push_event(Event::MouseLeave);
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            // Capture so a drag (scrubbing the progress bar) keeps reporting to us after the
            // pointer leaves Pillbox. Capturing does not activate the window.
            // SAFETY: hwnd is the live window this procedure was called for.
            unsafe { SetCapture(hwnd) };
            let (x, y) = point(lp);
            push_event(Event::Click { x, y });
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            // SAFETY: releasing a capture we may or may not hold is harmless.
            let _ = unsafe { ReleaseCapture() };
            let (x, y) = point(lp);
            push_event(Event::MouseUp { x, y });
            LRESULT(0)
        }
        WM_INPUT => {
            let mut raw = RAWINPUT::default();
            let mut size = size_of::<RAWINPUT>() as u32;
            // SAFETY: `raw` is a valid buffer of `size` bytes; lParam is the raw-input handle
            // Windows passed with this message. The keyboard union member is read only when
            // the header says the data is keyboard data.
            let read = unsafe {
                GetRawInputData(
                    HRAWINPUT(lp.0 as *mut c_void),
                    RID_INPUT,
                    Some((&mut raw as *mut RAWINPUT).cast()),
                    &mut size,
                    size_of::<RAWINPUTHEADER>() as u32,
                )
            };
            if read != u32::MAX && read > 0 && raw.header.dwType == RIM_TYPEKEYBOARD.0 {
                // SAFETY: dwType says the union holds keyboard data.
                let key = unsafe { raw.data.keyboard };
                // Only the three lock keys are ever looked at; every other key is ignored here.
                if matches!(key.VKey, 0x14 | 0x90 | 0x91) {
                    push_event(Event::LockKey {
                        vk: key.VKey,
                        down: key.Flags & 1 == 0,
                    });
                }
            }
            // SAFETY: forwards the unmodified arguments, as WM_INPUT requires for cleanup.
            unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
        }
        WM_RBUTTONUP => {
            let (x, y) = point(lp);
            push_event(Event::RightClick { x, y });
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            // Wheel messages carry screen coordinates, unlike the other mouse messages.
            let (sx, sy) = point(lp);
            let mut pt = POINT {
                x: sx as i32,
                y: sy as i32,
            };
            // SAFETY: hwnd is the live window; pt is a valid local.
            let _ = unsafe { ScreenToClient(hwnd, &mut pt) };
            let delta = f32::from(((wp.0 >> 16) & 0xFFFF) as u16 as i16) / 120.0;
            push_event(Event::Wheel {
                x: pt.x as f32,
                y: pt.y as f32,
                delta,
            });
            LRESULT(0)
        }
        WM_HOTKEY => {
            push_event(Event::Hotkey(wp.0 as i32));
            LRESULT(0)
        }
        WM_CLIPBOARDUPDATE => {
            push_event(Event::ClipboardChanged);
            LRESULT(0)
        }
        WM_CHAR | WM_KEYDOWN | WM_KILLFOCUS => {
            keyboard_message(msg, wp);
            LRESULT(0)
        }
        WM_TIMER => {
            push_event(Event::Timer(wp.0));
            LRESULT(0)
        }
        WM_DPICHANGED | WM_DISPLAYCHANGE | WM_SETTINGCHANGE => {
            push_event(Event::DisplayChanged);
            LRESULT(0)
        }
        WM_POWERBROADCAST => {
            push_event(Event::PowerChanged);
            LRESULT(1)
        }
        WM_DEVICECHANGE => {
            if let Some(event) = drive_event(wp.0 as u32, lp) {
                push_event(event);
            }
            LRESULT(1)
        }
        WM_APP_BACKDROP => {
            push_event(Event::BackdropFrame);
            LRESULT(0)
        }
        WM_APP_WAKE => {
            push_event(Event::Wake);
            LRESULT(0)
        }
        WM_DESTROY => {
            // SAFETY: no pointer arguments.
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        // SAFETY: forwards the arguments this procedure was called with, unchanged.
        _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
}

/// Lets a worker thread nudge the UI thread. Holds only the window handle's numeric value.
#[derive(Clone, Copy)]
pub struct Waker(isize);

impl Waker {
    pub fn wake(&self) {
        // SAFETY: PostMessageW is thread-safe and tolerates a window that no longer exists.
        let _ = unsafe {
            PostMessageW(
                Some(HWND(self.0 as *mut c_void)),
                WM_APP_WAKE,
                WPARAM(0),
                LPARAM(0),
            )
        };
    }
}

/// The borderless, transparent, topmost, never-activated window Pillbox is drawn in.
pub struct PillboxWindow {
    hwnd: HWND,
    region: Option<RegionKey>,
    excluded_from_capture: bool,
    listening_clipboard: bool,
}

impl PillboxWindow {
    pub fn create(x: i32, y: i32, width: u32, height: u32) -> Result<Self> {
        let hinst = module_instance()?;
        let class = w!("Pillbox.Shell");
        // SAFETY: the class name and wndproc are 'static; all handles passed are valid.
        let hwnd = unsafe {
            let wc = WNDCLASSW {
                lpfnWndProc: Some(wndproc),
                hInstance: hinst,
                hCursor: LoadCursorW(None, IDC_ARROW).context("loading the arrow cursor")?,
                lpszClassName: class,
                ..Default::default()
            };
            if RegisterClassW(&wc) == 0 {
                return Err(anyhow!("RegisterClassW failed for Pillbox window"));
            }
            CreateWindowExW(
                // TOOLWINDOW: no taskbar button. NOACTIVATE: never takes focus.
                // NOREDIRECTIONBITMAP: content comes only from DirectComposition (per-pixel alpha).
                WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TOPMOST | WS_EX_NOREDIRECTIONBITMAP,
                class,
                w!("Pillbox"),
                WS_POPUP,
                x,
                y,
                width as i32,
                height as i32,
                None,
                None,
                Some(hinst),
                None,
            )
            .context("creating Pillbox window")?
        };
        Ok(Self {
            hwnd,
            region: None,
            excluded_from_capture: false,
            listening_clipboard: false,
        })
    }

    /// Register a system-wide shortcut. Windows refuses if another app already owns it.
    pub fn register_hotkey(&self, id: i32, modifiers: u32, vk: u16) -> Result<()> {
        // SAFETY: hwnd is our live window; the id is ours; no pointers.
        unsafe {
            RegisterHotKey(
                Some(self.hwnd),
                id,
                HOT_KEY_MODIFIERS(modifiers) | MOD_NOREPEAT,
                u32::from(vk),
            )
        }
        .map_err(|e| anyhow!("{e}"))
    }

    pub fn unregister_hotkey(&self, id: i32) {
        // SAFETY: hwnd is our live window. Failing for an id that is not registered is fine.
        let _ = unsafe { UnregisterHotKey(Some(self.hwnd), id) };
    }

    /// Ask Windows to post WM_CLIPBOARDUPDATE whenever any app changes the clipboard.
    pub fn listen_clipboard(&mut self, on: bool) {
        if on == self.listening_clipboard {
            return;
        }
        // SAFETY: hwnd is our live window; add and remove are paired by the flag.
        let result = unsafe {
            if on {
                AddClipboardFormatListener(self.hwnd)
            } else {
                RemoveClipboardFormatListener(self.hwnd)
            }
        };
        match result {
            Ok(()) => self.listening_clipboard = on,
            Err(e) => tracing::warn!("clipboard listener ({on}): {e}"),
        }
    }

    /// Give Pillbox the keyboard so a text box can be typed into. Only called right after
    /// the user clicked the text box: Windows lets a process that received the last input
    /// take the foreground. The window that had the keyboard is remembered for later.
    /// Take the keyboard for a text box; `from` keeps the app to hand it back to.
    pub fn take_keyboard(&self, from: &mut Option<isize>) -> bool {
        take_keyboard_for(self.hwnd, from)
    }

    /// Hand the keyboard back to the window that had it before `take_keyboard`.
    pub fn give_back_keyboard(&self, from: &mut Option<isize>) {
        give_back_keyboard_for(from);
    }

    pub fn hwnd(&self) -> HWND {
        self.hwnd
    }

    pub fn waker(&self) -> Waker {
        Waker(self.hwnd.0 as isize)
    }

    /// Limit the window to this shape. Outside it the window does not exist as far as input is
    /// concerned, so clicks go to whatever is underneath, in any process.
    pub fn set_region(&mut self, key: RegionKey) {
        if self.region == Some(key) {
            return;
        }
        self.region = Some(key);
        // SAFETY: CreateRoundRectRgn returns a fresh GDI region. SetWindowRgn takes ownership
        // of it on success, so it must not be deleted here.
        unsafe {
            let rgn = CreateRoundRectRgn(
                key.left,
                key.top,
                key.right,
                key.bottom,
                key.ellipse,
                key.ellipse,
            );
            SetWindowRgn(self.hwnd, Some(rgn), false);
        }
    }

    pub fn set_rect(&mut self, x: i32, y: i32, width: u32, height: u32) -> Result<()> {
        // SAFETY: hwnd is our live window.
        unsafe {
            SetWindowPos(
                self.hwnd,
                Some(HWND_TOPMOST),
                x,
                y,
                width as i32,
                height as i32,
                SWP_NOACTIVATE,
            )
        }
        .context("moving Pillbox window")
    }

    /// Put Pillbox back on top of the other always-on-top windows, without moving, sizing
    /// or activating it.
    ///
    /// Undocumented Windows behaviour, seen on Windows 11 build 26300: when an app Pillbox
    /// started (the Photos app, opened from the clipboard) closes, Windows activates the
    /// window behind it and stacks that normal window *above* Pillbox, although Pillbox
    /// keeps its `WS_EX_TOPMOST` flag. Nothing puts it back by itself, so Pillbox calls this
    /// whenever the foreground window changes and when a notice comes up.
    pub fn raise(&self) {
        use windows::Win32::UI::WindowsAndMessaging::{SWP_NOMOVE, SWP_NOSIZE};
        // SAFETY: hwnd is our live window; no position or size is changed.
        let _ = unsafe {
            SetWindowPos(
                self.hwnd,
                Some(HWND_TOPMOST),
                0,
                0,
                0,
                0,
                SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE,
            )
        };
    }

    pub fn set_visible(&self, visible: bool) {
        if visible == self.is_visible() {
            return;
        }
        // SAFETY: hwnd is our live window. SW_SHOWNOACTIVATE shows without taking focus.
        let _ = unsafe { ShowWindow(self.hwnd, if visible { SW_SHOWNOACTIVATE } else { SW_HIDE }) };
    }

    pub fn is_visible(&self) -> bool {
        // SAFETY: hwnd is our live window.
        unsafe { IsWindowVisible(self.hwnd) }.as_bool()
    }

    /// Keep Pillbox out of screenshots, recordings and our own backdrop capture.
    pub fn set_excluded_from_capture(&mut self, excluded: bool) {
        if excluded == self.excluded_from_capture {
            return;
        }
        let affinity = if excluded {
            WDA_EXCLUDEFROMCAPTURE
        } else {
            WDA_NONE
        };
        // SAFETY: hwnd is our live top-level window.
        match unsafe { SetWindowDisplayAffinity(self.hwnd, affinity) } {
            Ok(()) => self.excluded_from_capture = excluded,
            Err(e) => tracing::warn!("SetWindowDisplayAffinity failed: {e}"),
        }
    }

    pub fn is_excluded_from_capture(&self) -> bool {
        self.excluded_from_capture
    }

    /// (Re)start a timer. It keeps firing until killed.
    pub fn set_timer(&self, id: usize, ms: u32) {
        // SAFETY: hwnd is our live window; no callback, the timer posts WM_TIMER.
        if unsafe { SetTimer(Some(self.hwnd), id, ms.max(1), None) } == 0 {
            tracing::warn!("SetTimer({id}) failed");
        }
    }

    pub fn kill_timer(&self, id: usize) {
        // SAFETY: hwnd is our live window. Failing because the timer is not set is fine.
        let _ = unsafe { KillTimer(Some(self.hwnd), id) };
    }

    /// Ask Windows to tell us about keyboard input even while another app has focus, so
    /// lock-key changes can be shown. Uses Raw Input (no hook, cannot delay typing). While
    /// this is on, every key press wakes Pillbox briefly; only lock keys are examined.
    pub fn watch_lock_keys(&self, on: bool) {
        let device = RAWINPUTDEVICE {
            usUsagePage: 1, // generic desktop
            usUsage: 6,     // keyboard
            dwFlags: if on { RIDEV_INPUTSINK } else { RIDEV_REMOVE },
            // Removal requires a null target.
            hwndTarget: if on { self.hwnd } else { HWND::default() },
        };
        // SAFETY: one fully initialised device entry with its correct size.
        if let Err(e) =
            unsafe { RegisterRawInputDevices(&[device], size_of::<RAWINPUTDEVICE>() as u32) }
            && on
        {
            tracing::warn!("lock-key pop-ups are off: {e}");
        }
    }
}

/// Whether a lock key (0x14 Caps, 0x90 Num, 0x91 Scroll) is currently switched on, as this
/// thread last saw it. Only reliable at start-up: a thread that receives no keyboard messages
/// does not get its key state refreshed, which is why the app tracks changes itself.
pub fn lock_key_is_on(vk: u16) -> bool {
    // SAFETY: no pointers; any virtual-key value is accepted.
    (unsafe { GetKeyState(i32::from(vk)) } & 1) != 0
}

impl Drop for PillboxWindow {
    fn drop(&mut self) {
        // SAFETY: hwnd was created by us on this thread and is destroyed at most once.
        let _ = unsafe { DestroyWindow(self.hwnd) };
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pumped {
    /// Messages were dispatched; check the event queue.
    Messages,
    /// The frame timer fired: time to step the animation and draw.
    FrameDue,
    /// Something in the settings folder was written.
    SettingsChanged,
    Quit,
}

fn dispatch_pending() -> bool {
    let mut msg = MSG::default();
    // SAFETY: msg is a valid local; the messages belong to this thread's queue.
    unsafe {
        while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
            if msg.message == WM_QUIT {
                return false;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    true
}

/// Sleep in the kernel until there is something to do: a window message, a settings-file
/// change, or (only while animating) the next frame. Uses no CPU while nothing happens.
pub fn pump(frame_timer: Option<&FrameTimer>, watcher: Option<&DirWatcher>) -> Pumped {
    // Anything already queued is handled first, so a wait never starts with work pending.
    if !dispatch_pending() {
        return Pumped::Quit;
    }
    if EVENTS.with_borrow(|q| !q.is_empty()) {
        return Pumped::Messages;
    }
    let mut handles: Vec<HANDLE> = Vec::with_capacity(2);
    let mut kinds: Vec<Pumped> = Vec::with_capacity(2);
    if let Some(t) = frame_timer {
        handles.push(t.handle());
        kinds.push(Pumped::FrameDue);
    }
    if let Some(w) = watcher {
        handles.push(w.handle());
        kinds.push(Pumped::SettingsChanged);
    }
    // SAFETY: the handle slice is a valid local holding live handles owned by the callers.
    let woke = unsafe {
        MsgWaitForMultipleObjects(
            if handles.is_empty() {
                None
            } else {
                Some(&handles)
            },
            false,
            INFINITE,
            QS_ALLINPUT,
        )
    };
    let index = woke.0.wrapping_sub(WAIT_OBJECT_0.0) as usize;
    let result = kinds.get(index).copied().unwrap_or(Pumped::Messages);
    if !dispatch_pending() {
        return Pumped::Quit;
    }
    result
}
