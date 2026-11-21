//! A widget popped out of Pillbox onto the desktop: its own small borderless window with
//! per-pixel transparency, drawn with the same renderer. Its buttons (focus timer, to-do
//! ticks, next photo) and text boxes work; pressing anywhere else drags it; right-click closes
//! it. It takes the keyboard only while one of its text boxes is being typed into (like the
//! Pillbox), and has no taskbar button.

use std::{cell::RefCell, collections::HashMap, sync::Once};

use anyhow::{Context, Result, anyhow};
use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
        Graphics::Gdi::ScreenToClient,
        UI::WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, DestroyWindow, GetWindowRect, HTCAPTION, HTCLIENT,
            HTRIGHT, IDC_HAND, IDC_SIZEALL, LoadCursorW, MA_NOACTIVATE, MINMAXINFO, RegisterClassW,
            SW_SHOWNOACTIVATE, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
            SWP_NOZORDER, SetCursor, SetWindowPos, ShowWindow, WM_CHAR, WM_EXITSIZEMOVE,
            WM_GETMINMAXINFO, WM_KEYDOWN, WM_KILLFOCUS, WM_LBUTTONUP, WM_MOUSEACTIVATE,
            WM_NCCALCSIZE, WM_NCHITTEST, WM_NCRBUTTONUP, WM_RBUTTONUP, WM_SETCURSOR, WM_SIZE,
            WNDCLASSW, WS_EX_NOACTIVATE, WS_EX_NOREDIRECTIONBITMAP, WS_EX_TOOLWINDOW, WS_POPUP,
            WS_THICKFRAME,
        },
    },
    core::w,
};

use super::{
    gfx::Renderer,
    window::{Event, keyboard_message, module_instance, push_event, take_keyboard_for},
};

/// Left, top, right, bottom in client pixels.
type ButtonRect = (f32, f32, f32, f32);

thread_local! {
    /// Button rectangles (client pixels) of each widget window, from its last paint.
    static BUTTONS: RefCell<HashMap<isize, Vec<ButtonRect>>> =
        RefCell::new(HashMap::new());
}

/// Width limits and the fixed height (window pixels) of the widget windows whose right edge can
/// be dragged: (minimum width, maximum width, height). Absent = not resizable.
#[derive(Clone, Copy)]
struct Limits {
    min_w: i32,
    max_w: i32,
    height: i32,
}

thread_local! {
    static LIMITS: RefCell<HashMap<isize, Limits>> = RefCell::new(HashMap::new());
}

fn limits_of(hwnd: HWND) -> Option<Limits> {
    LIMITS.with_borrow(|l| l.get(&(hwnd.0 as isize)).copied())
}

/// How close to the right edge (window pixels) a press starts a resize.
const EDGE: i32 = 10;

fn on_button(hwnd: HWND, x: f32, y: f32) -> bool {
    BUTTONS.with_borrow(|b| {
        b.get(&(hwnd.0 as isize)).is_some_and(|rects| {
            rects
                .iter()
                .any(|&(l, t, r, bt)| x >= l && x < r && y >= t && y < bt)
        })
    })
}

fn client_point(lp: LPARAM) -> (f32, f32) {
    let x = (lp.0 & 0xFFFF) as u16 as i16;
    let y = ((lp.0 >> 16) & 0xFFFF) as u16 as i16;
    (f32::from(x), f32::from(y))
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        // A resizable window (it has WS_THICKFRAME so that Windows allows sizing it) has no
        // frame to draw: the whole window is client area.
        WM_NCCALCSIZE if limits_of(hwnd).is_some() => LRESULT(0),
        WM_GETMINMAXINFO => {
            if let Some(l) = limits_of(hwnd) {
                // SAFETY: for this message lParam points to a MINMAXINFO owned by the system.
                let info = unsafe { &mut *(lp.0 as *mut MINMAXINFO) };
                info.ptMinTrackSize = POINT {
                    x: l.min_w,
                    y: l.height,
                };
                info.ptMaxTrackSize = POINT {
                    x: l.max_w,
                    y: l.height,
                };
                LRESULT(0)
            } else {
                // SAFETY: forwards the arguments this procedure was called with, unchanged.
                unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
            }
        }
        WM_SIZE => {
            if limits_of(hwnd).is_some() {
                push_event(Event::WidgetResized(hwnd.0 as isize));
            }
            LRESULT(0)
        }
        // Buttons are ordinary client area (clickable); everything else is a drag handle. The
        // right edge of a resizable window is its sizing handle.
        WM_NCHITTEST => {
            let (sx, sy) = client_point(lp);
            let mut pt = POINT {
                x: sx as i32,
                y: sy as i32,
            };
            // SAFETY: hwnd is the live window this procedure runs for; pt is a valid local.
            let _ = unsafe { ScreenToClient(hwnd, &mut pt) };
            if limits_of(hwnd).is_some() {
                let mut r = RECT::default();
                // SAFETY: the live window and a valid out-struct.
                if unsafe { GetWindowRect(hwnd, &mut r) }.is_ok() && pt.x >= r.right - r.left - EDGE
                {
                    return LRESULT(HTRIGHT as isize);
                }
            }
            if on_button(hwnd, pt.x as f32, pt.y as f32) {
                LRESULT(HTCLIENT as isize)
            } else {
                LRESULT(HTCAPTION as isize)
            }
        }
        WM_SETCURSOR if (lp.0 & 0xFFFF) as u32 == HTCLIENT => {
            // SAFETY: a shared system cursor; SetCursor takes no ownership.
            unsafe {
                if let Ok(hand) = LoadCursorW(None, IDC_HAND) {
                    SetCursor(Some(hand));
                }
            }
            LRESULT(1)
        }
        WM_LBUTTONUP => {
            let (x, y) = client_point(lp);
            push_event(Event::WidgetClick {
                id: hwnd.0 as isize,
                x,
                y,
            });
            LRESULT(0)
        }
        WM_CHAR | WM_KEYDOWN | WM_KILLFOCUS => {
            keyboard_message(msg, wp);
            LRESULT(0)
        }
        WM_RBUTTONUP => {
            push_event(Event::WidgetClose(hwnd.0 as isize));
            LRESULT(0)
        }
        WM_NCRBUTTONUP => {
            push_event(Event::WidgetClose(hwnd.0 as isize));
            LRESULT(0)
        }
        WM_EXITSIZEMOVE => {
            push_event(Event::WidgetMoved(hwnd.0 as isize));
            LRESULT(0)
        }
        // SAFETY: forwards the arguments this procedure was called with, unchanged.
        _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
}

static REGISTER: Once = Once::new();

pub struct WidgetWindow {
    hwnd: HWND,
    pub renderer: Renderer,
    pub size: (u32, u32),
}

impl WidgetWindow {
    pub fn create(x: i32, y: i32, w: u32, h: u32) -> Result<Self> {
        Self::create_with(x, y, w, h, None)
    }

    /// A window whose right edge can be dragged to change its width (between `min_w` and
    /// `max_w`; the height stays `h`).
    pub fn create_resizable(
        x: i32,
        y: i32,
        w: u32,
        h: u32,
        (min_w, max_w): (u32, u32),
    ) -> Result<Self> {
        Self::create_with(x, y, w, h, Some((min_w, max_w)))
    }

    fn create_with(x: i32, y: i32, w: u32, h: u32, resizable: Option<(u32, u32)>) -> Result<Self> {
        let hinst = module_instance()?;
        let class = w!("Pillbox.Widget");
        let mut registered = Ok(());
        REGISTER.call_once(|| {
            // SAFETY: 'static class name and procedure; the cursor is a shared system cursor.
            let ok = unsafe {
                let wc = WNDCLASSW {
                    lpfnWndProc: Some(wndproc),
                    hInstance: hinst,
                    hCursor: LoadCursorW(None, IDC_SIZEALL).unwrap_or_default(),
                    lpszClassName: class,
                    ..Default::default()
                };
                RegisterClassW(&wc) != 0
            };
            if !ok {
                registered = Err(anyhow!("RegisterClassW failed for widget windows"));
            }
        });
        registered?;
        // SAFETY: the class was registered above; all handles passed are valid.
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_NOREDIRECTIONBITMAP,
                class,
                w!("Pillbox widget"),
                if resizable.is_some() {
                    WS_POPUP | WS_THICKFRAME
                } else {
                    WS_POPUP
                },
                x,
                y,
                w as i32,
                h as i32,
                None,
                None,
                Some(hinst),
                None,
            )
            .context("creating a widget window")?
        };
        if let Some((min_w, max_w)) = resizable {
            LIMITS.with_borrow_mut(|l| {
                l.insert(
                    hwnd.0 as isize,
                    Limits {
                        min_w: min_w as i32,
                        max_w: max_w as i32,
                        height: h as i32,
                    },
                );
            });
            // The window was created before it was marked resizable: have Windows measure the
            // (now empty) frame again, so that the client area is the whole window.
            // SAFETY: the window was created just above.
            let _ = unsafe {
                SetWindowPos(
                    hwnd,
                    None,
                    0,
                    0,
                    0,
                    0,
                    SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                )
            };
        }
        let renderer = match Renderer::new(hwnd, w, h) {
            Ok(r) => r,
            Err(e) => {
                // SAFETY: the window was created just above and is destroyed once.
                let _ = unsafe { DestroyWindow(hwnd) };
                return Err(e);
            }
        };
        Ok(Self {
            hwnd,
            renderer,
            size: (w, h),
        })
    }

    pub fn show(&self) {
        // SAFETY: our live window; shown without taking focus.
        let _ = unsafe { ShowWindow(self.hwnd, SW_SHOWNOACTIVATE) };
    }

    pub fn id(&self) -> isize {
        self.hwnd.0 as isize
    }

    /// Take the keyboard for a text box in this widget (right after the user clicked it).
    pub fn take_keyboard(&self, from: &mut Option<isize>) -> bool {
        take_keyboard_for(self.hwnd, from)
    }

    /// Where the buttons are (client pixels), so presses on them click instead of drag.
    pub fn set_buttons(&self, rects: Vec<ButtonRect>) {
        BUTTONS.with_borrow_mut(|b| {
            b.insert(self.id(), rects);
        });
    }

    /// The window's current size in pixels (it can change while the right edge is dragged).
    pub fn current_size(&self) -> (u32, u32) {
        let mut r = RECT::default();
        // SAFETY: our live window and a valid out-struct.
        if unsafe { GetWindowRect(self.hwnd, &mut r) }.is_ok() {
            (
                (r.right - r.left).max(1) as u32,
                (r.bottom - r.top).max(1) as u32,
            )
        } else {
            self.size
        }
    }

    /// Resize what is drawn to the window's current size. `true` if the size changed.
    pub fn sync_size(&mut self) -> bool {
        let now = self.current_size();
        if now == self.size {
            return false;
        }
        if let Err(e) = self.renderer.resize(now.0, now.1) {
            tracing::warn!("widget window resize: {e:#}");
        }
        self.size = now;
        true
    }

    /// Top-left corner on the screen.
    pub fn position(&self) -> (i32, i32) {
        let mut r = RECT::default();
        // SAFETY: our live window and a valid out-struct.
        if unsafe { GetWindowRect(self.hwnd, &mut r) }.is_ok() {
            (r.left, r.top)
        } else {
            (0, 0)
        }
    }
}

impl Drop for WidgetWindow {
    fn drop(&mut self) {
        LIMITS.with_borrow_mut(|l| {
            l.remove(&self.id());
        });
        BUTTONS.with_borrow_mut(|b| {
            b.remove(&self.id());
        });
        // SAFETY: created by us on this thread and destroyed once.
        let _ = unsafe { DestroyWindow(self.hwnd) };
    }
}
