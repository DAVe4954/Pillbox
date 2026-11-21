//! Web and chat drawers: a web page (WebView2) in a panel below Pillbox.
//!
//! * The window is topmost with no taskbar button. It appears without taking focus; clicking
//!   into the page activates it like any window, so chat can be typed into. A thin header shows
//!   the title and a close button and can be dragged.
//! * WebView2 is created asynchronously: the environment and controller callbacks arrive
//!   through this thread's ordinary message loop, so Pillbox never blocks waiting for it.
//! * The WebView2 loader DLL (Microsoft's redistributable `WebView2Loader.dll`) is embedded in
//!   the executable and written to `%LOCALAPPDATA%\Pillbox\` on first use, then loaded from that
//!   full path. Nothing is loaded, and no browser process runs, until a drawer is opened.
//! * Closing the drawer closes the controller, which ends its browser processes.

use std::{cell::RefCell, collections::HashMap, ffi::c_void, path::PathBuf, rc::Rc, sync::Once};

use anyhow::{Context, Result, anyhow};
use webview2_com::{
    CreateCoreWebView2ControllerCompletedHandler, CreateCoreWebView2EnvironmentCompletedHandler,
    Microsoft::Web::WebView2::Win32::{
        COREWEBVIEW2_COLOR, ICoreWebView2Controller, ICoreWebView2Controller2,
        ICoreWebView2CreateCoreWebView2EnvironmentCompletedHandler, ICoreWebView2Environment,
    },
};
use windows::{
    Win32::{
        Foundation::{COLORREF, FreeLibrary, HMODULE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
        Graphics::{
            Dwm::{
                DWM_WINDOW_CORNER_PREFERENCE, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
                DwmSetWindowAttribute,
            },
            Gdi::{
                BeginPaint, CreateSolidBrush, DT_CENTER, DT_END_ELLIPSIS, DT_LEFT, DT_SINGLELINE,
                DT_VCENTER, DeleteObject, DrawTextW, EndPaint, FillRect, InvalidateRect,
                PAINTSTRUCT, ScreenToClient, SetBkMode, SetTextColor, TRANSPARENT,
            },
        },
        System::LibraryLoader::{GetProcAddress, LoadLibraryW},
        UI::WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect, HTCAPTION, HTCLIENT,
            IDC_ARROW, LoadCursorW, RegisterClassW, SW_SHOWNOACTIVATE, SWP_NOACTIVATE,
            SWP_NOZORDER, SetWindowPos, ShowWindow, WM_LBUTTONUP, WM_NCHITTEST, WM_PAINT, WM_SIZE,
            WNDCLASSW, WS_CLIPCHILDREN, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
        },
    },
    core::{HRESULT, HSTRING, Interface, PCWSTR, s, w},
};

use super::window::{Event, module_instance, push_event};

/// Height of the header strip, in pixels at 100%.
const HEADER: f32 = 30.0;

/// The loader DLL, shipped inside the executable.
static LOADER_DLL: &[u8] = include_bytes!("../../vendor/WebView2Loader.dll");

type CreateEnvironment =
    unsafe extern "system" fn(PCWSTR, PCWSTR, *mut c_void, *mut c_void) -> HRESULT;

/// What the window procedure needs per drawer: header text, status line, header height.
struct Paint {
    title: String,
    status: String,
    header: i32,
}

thread_local! {
    static PAINT: RefCell<HashMap<isize, Paint>> = RefCell::new(HashMap::new());
    static LOADER: RefCell<Option<(HMODULE, CreateEnvironment)>> = const { RefCell::new(None) };
}

fn close_rect(hwnd: HWND, header: i32) -> RECT {
    let mut r = RECT::default();
    // SAFETY: hwnd is a live drawer window; r is a valid out-struct.
    let _ = unsafe { GetClientRect(hwnd, &mut r) };
    RECT {
        left: r.right - header,
        top: 0,
        right: r.right,
        bottom: header,
    }
}

fn header_of(hwnd: HWND) -> i32 {
    PAINT.with_borrow(|p| p.get(&(hwnd.0 as isize)).map_or(30, |p| p.header))
}

fn inside(r: &RECT, x: i32, y: i32) -> bool {
    x >= r.left && x < r.right && y >= r.top && y < r.bottom
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_NCHITTEST => {
            let mut pt = POINT {
                x: (lp.0 & 0xFFFF) as u16 as i16 as i32,
                y: ((lp.0 >> 16) & 0xFFFF) as u16 as i16 as i32,
            };
            // SAFETY: live window, valid local point.
            let _ = unsafe { ScreenToClient(hwnd, &mut pt) };
            let header = header_of(hwnd);
            // The header drags the drawer, except for its close button.
            if pt.y < header && !inside(&close_rect(hwnd, header), pt.x, pt.y) {
                LRESULT(HTCAPTION as isize)
            } else {
                LRESULT(HTCLIENT as isize)
            }
        }
        WM_LBUTTONUP => {
            let (x, y) = (
                (lp.0 & 0xFFFF) as u16 as i16 as i32,
                ((lp.0 >> 16) & 0xFFFF) as u16 as i16 as i32,
            );
            if inside(&close_rect(hwnd, header_of(hwnd)), x, y) {
                push_event(Event::DrawerClose(hwnd.0 as isize));
            }
            LRESULT(0)
        }
        WM_SIZE => {
            push_event(Event::DrawerResized(hwnd.0 as isize));
            LRESULT(0)
        }
        WM_PAINT => {
            paint(hwnd);
            LRESULT(0)
        }
        // SAFETY: forwards the arguments this procedure was called with, unchanged.
        _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
}

/// Header (title, close button) and, until the page shows, a status line. Plain GDI: the
/// page itself covers the rest of the window.
fn paint(hwnd: HWND) {
    let (title, status, header) = PAINT.with_borrow(|p| {
        p.get(&(hwnd.0 as isize))
            .map(|p| (p.title.clone(), p.status.clone(), p.header))
            .unwrap_or_default()
    });
    let mut ps = PAINTSTRUCT::default();
    // SAFETY: BeginPaint/EndPaint are paired on the live window; the brush is created and
    // deleted here; text buffers are locals that outlive the calls.
    unsafe {
        let dc = BeginPaint(hwnd, &mut ps);
        let mut client = RECT::default();
        let _ = GetClientRect(hwnd, &mut client);
        let bg = CreateSolidBrush(COLORREF(0x001C_1818));
        FillRect(dc, &client, bg);
        let _ = DeleteObject(bg.into());
        SetBkMode(dc, TRANSPARENT);
        SetTextColor(dc, COLORREF(0x00E6_E6E6));
        let mut title_rect = RECT {
            left: header / 3,
            top: 0,
            right: client.right - header,
            bottom: header,
        };
        let mut t: Vec<u16> = title.encode_utf16().collect();
        DrawTextW(
            dc,
            &mut t,
            &mut title_rect,
            DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS,
        );
        let mut close = close_rect(hwnd, header);
        let mut x: Vec<u16> = "✕".encode_utf16().collect();
        DrawTextW(
            dc,
            &mut x,
            &mut close,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE,
        );
        if !status.is_empty() {
            SetTextColor(dc, COLORREF(0x00A0_A0A0));
            let mut body = RECT {
                left: 12,
                top: header,
                right: client.right - 12,
                bottom: client.bottom,
            };
            let mut s: Vec<u16> = status.encode_utf16().collect();
            DrawTextW(
                dc,
                &mut s,
                &mut body,
                DT_CENTER | DT_VCENTER | DT_SINGLELINE,
            );
        }
        let _ = EndPaint(hwnd, &ps);
    }
}

fn set_status(hwnd: HWND, status: &str) {
    PAINT.with_borrow_mut(|p| {
        if let Some(p) = p.get_mut(&(hwnd.0 as isize)) {
            p.status = status.to_string();
        }
    });
    // SAFETY: live window; repaint the whole client area.
    let _ = unsafe { InvalidateRect(Some(hwnd), None, true) };
}

/// Where the loader DLL and the browser profile live.
fn base_dir() -> Result<PathBuf> {
    let dir = match std::env::var_os("PILLBOX_DATA_DIR") {
        Some(d) => PathBuf::from(d),
        None => PathBuf::from(
            std::env::var_os("LOCALAPPDATA").ok_or_else(|| anyhow!("LOCALAPPDATA is not set"))?,
        )
        .join("Pillbox"),
    };
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    Ok(dir)
}

/// Write the embedded loader next to the data (if it is not there yet) and load it.
fn loader() -> Result<CreateEnvironment> {
    if let Some((_, f)) = LOADER.with_borrow(|l| *l) {
        return Ok(f);
    }
    let path = base_dir()?.join("WebView2Loader.dll");
    let current = std::fs::metadata(&path).map(|m| m.len()).ok();
    if current != Some(LOADER_DLL.len() as u64) {
        std::fs::write(&path, LOADER_DLL).with_context(|| format!("writing {}", path.display()))?;
    }
    let wide = HSTRING::from(path.as_os_str());
    // SAFETY: a full path to the Microsoft-signed loader we just wrote; the module stays loaded
    // for the life of the process (kept in LOADER, freed never), so the function pointer stays
    // valid. The transmute matches the documented signature of
    // CreateCoreWebView2EnvironmentWithOptions.
    unsafe {
        let module = LoadLibraryW(&wide).context("loading WebView2Loader.dll")?;
        let Some(proc) = GetProcAddress(module, s!("CreateCoreWebView2EnvironmentWithOptions"))
        else {
            let _ = FreeLibrary(module);
            return Err(anyhow!(
                "WebView2Loader.dll has no CreateCoreWebView2EnvironmentWithOptions"
            ));
        };
        let f: CreateEnvironment = std::mem::transmute(proc);
        LOADER.with_borrow_mut(|l| *l = Some((module, f)));
        Ok(f)
    }
}

static REGISTER: Once = Once::new();

/// What the asynchronous creation leaves behind for the drawer.
#[derive(Default)]
struct Inner {
    controller: Option<ICoreWebView2Controller>,
    header: i32,
}

pub struct Drawer {
    hwnd: HWND,
    inner: Rc<RefCell<Inner>>,
}

impl Drawer {
    /// Open a drawer at (x, y) with size (w, h) in screen pixels, showing `url`.
    pub fn open(title: &str, url: &str, rect: (i32, i32, i32, i32), scale: f32) -> Result<Self> {
        let hinst = module_instance()?;
        let class = w!("Pillbox.Drawer");
        let mut registered = Ok(());
        REGISTER.call_once(|| {
            // SAFETY: 'static class name and procedure; shared system cursor.
            let ok = unsafe {
                let wc = WNDCLASSW {
                    lpfnWndProc: Some(wndproc),
                    hInstance: hinst,
                    hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
                    lpszClassName: class,
                    ..Default::default()
                };
                RegisterClassW(&wc) != 0
            };
            if !ok {
                registered = Err(anyhow!("RegisterClassW failed for drawer windows"));
            }
        });
        registered?;
        let (x, y, w, h) = rect;
        // SAFETY: the class was registered above; all handles passed are valid.
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
                class,
                w!("Pillbox drawer"),
                WS_POPUP | WS_CLIPCHILDREN,
                x,
                y,
                w,
                h,
                None,
                None,
                Some(hinst),
                None,
            )
            .context("creating a drawer window")?
        };
        let header = (HEADER * scale).round() as i32;
        PAINT.with_borrow_mut(|p| {
            p.insert(
                hwnd.0 as isize,
                Paint {
                    title: title.to_string(),
                    status: "Loading…".into(),
                    header,
                },
            );
        });
        // SAFETY: live window; the attribute value is a local of the documented size.
        unsafe {
            let round = DWMWCP_ROUND;
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_WINDOW_CORNER_PREFERENCE,
                (&round as *const DWM_WINDOW_CORNER_PREFERENCE).cast(),
                size_of_val(&round) as u32,
            );
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        }
        let inner = Rc::new(RefCell::new(Inner {
            controller: None,
            header,
        }));
        let drawer = Self { hwnd, inner };
        if let Err(e) = drawer.start(url) {
            tracing::warn!("drawer: {e:#}");
            set_status(
                hwnd,
                "WebView2 could not start (is the WebView2 runtime installed?)",
            );
        }
        Ok(drawer)
    }

    fn start(&self, url: &str) -> Result<()> {
        let create = loader()?;
        let profile = HSTRING::from(base_dir()?.join("WebView2").as_os_str());
        let hwnd = self.hwnd;
        let inner = self.inner.clone();
        let url = url.to_string();
        let handler: ICoreWebView2CreateCoreWebView2EnvironmentCompletedHandler =
            CreateCoreWebView2EnvironmentCompletedHandler::create(Box::new(
                move |result: windows::core::Result<()>, env: Option<ICoreWebView2Environment>| {
                    let env = match (result, env) {
                        (Ok(()), Some(env)) => env,
                        (r, _) => {
                            tracing::warn!("WebView2 environment failed: {r:?}");
                            set_status(hwnd, "WebView2 could not start");
                            return Ok(());
                        }
                    };
                    let inner2 = inner.clone();
                    let controller_done = CreateCoreWebView2ControllerCompletedHandler::create(
                        Box::new(move |result: windows::core::Result<()>, controller: Option<ICoreWebView2Controller>| {
                            match (result, controller) {
                                (Ok(()), Some(c)) => {
                                    if let Err(e) = show_page(hwnd, &c, &inner2, &url) {
                                        tracing::warn!("drawer page: {e}");
                                        set_status(hwnd, "The page could not be shown");
                                    }
                                }
                                (r, _) => {
                                    tracing::warn!("WebView2 controller failed: {r:?}");
                                    set_status(hwnd, "WebView2 could not start");
                                }
                            }
                            Ok(())
                        }),
                    );
                    // SAFETY: the environment and our live window; the handler is called back
                    // on this thread through the message loop.
                    unsafe { env.CreateCoreWebView2Controller(hwnd, &controller_done) }
                },
            ));
        // SAFETY: the loader's documented entry point: no browser folder (use the installed
        // runtime), our profile folder, no options, and a completion handler that WebView2
        // keeps a reference to until it is called.
        unsafe {
            create(
                PCWSTR::null(),
                PCWSTR(profile.as_ptr()),
                std::ptr::null_mut(),
                handler.as_raw(),
            )
        }
        .ok()
        .context("CreateCoreWebView2EnvironmentWithOptions")
    }

    pub fn id(&self) -> isize {
        self.hwnd.0 as isize
    }

    pub fn is_window(&self, id: isize) -> bool {
        self.id() == id
    }

    /// Fit the page to the window (after a move or resize).
    pub fn fit(&self) {
        let inner = self.inner.borrow();
        if let Some(c) = &inner.controller {
            fit(self.hwnd, c, inner.header);
        }
    }

    pub fn set_rect(&self, x: i32, y: i32, w: i32, h: i32) {
        // SAFETY: our live window.
        let _ = unsafe { SetWindowPos(self.hwnd, None, x, y, w, h, SWP_NOACTIVATE | SWP_NOZORDER) };
        self.fit();
    }
}

fn fit(hwnd: HWND, c: &ICoreWebView2Controller, header: i32) {
    let mut r = RECT::default();
    // SAFETY: live window and controller; r is a valid local.
    unsafe {
        let _ = GetClientRect(hwnd, &mut r);
        let _ = c.SetBounds(RECT {
            left: 0,
            top: header,
            right: r.right,
            bottom: r.bottom,
        });
    }
}

fn show_page(
    hwnd: HWND,
    c: &ICoreWebView2Controller,
    inner: &Rc<RefCell<Inner>>,
    url: &str,
) -> windows::core::Result<()> {
    let header = inner.borrow().header;
    // SAFETY: a live controller created for our window.
    unsafe {
        if let Ok(c2) = c.cast::<ICoreWebView2Controller2>() {
            // Dark until the page paints, instead of a white flash.
            let _ = c2.SetDefaultBackgroundColor(COREWEBVIEW2_COLOR {
                A: 255,
                R: 24,
                G: 24,
                B: 28,
            });
        }
        fit(hwnd, c, header);
        c.SetIsVisible(true)?;
        c.CoreWebView2()?.Navigate(&HSTRING::from(url))?;
    }
    set_status(hwnd, "");
    inner.borrow_mut().controller = Some(c.clone());
    tracing::info!("drawer showing {url}");
    Ok(())
}

impl Drop for Drawer {
    fn drop(&mut self) {
        if let Some(c) = self.inner.borrow_mut().controller.take() {
            // SAFETY: closing the live controller ends its browser processes.
            let _ = unsafe { c.Close() };
        }
        PAINT.with_borrow_mut(|p| {
            p.remove(&self.id());
        });
        // SAFETY: created by us on this thread and destroyed once.
        let _ = unsafe { DestroyWindow(self.hwnd) };
    }
}
