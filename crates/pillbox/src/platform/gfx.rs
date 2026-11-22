//! Direct3D 11 + Direct2D renderer presenting through DirectComposition, which is what gives
//! the window real per-pixel transparency on the GPU. Also owns the optional "backdrop": a
//! live capture of the screen behind Pillbox, used by the glass surfaces.

use std::{collections::HashMap, ffi::c_void, path::PathBuf};

use anyhow::{Context, Result, anyhow};
use pillbox_core::{shape::Pill, theme::Rgba};
use windows::{
    Foundation::TypedEventHandler,
    Graphics::{
        Capture::{
            Direct3D11CaptureFramePool, GraphicsCaptureDirtyRegionMode, GraphicsCaptureItem,
            GraphicsCaptureSession,
        },
        DirectX::{Direct3D11::IDirect3DDevice, DirectXPixelFormat},
    },
    Win32::{
        Foundation::{HMODULE, HWND, LPARAM, WPARAM},
        Graphics::{
            Direct2D::{
                CLSID_D2D1DisplacementMap, CLSID_D2D1GaussianBlur,
                Common::{
                    D2D_RECT_F, D2D_SIZE_U, D2D1_ALPHA_MODE_IGNORE, D2D1_ALPHA_MODE_PREMULTIPLIED,
                    D2D1_BORDER_MODE_HARD, D2D1_COLOR_F, D2D1_GRADIENT_STOP, D2D1_PIXEL_FORMAT,
                },
                D2D1_ANTIALIAS_MODE_ALIASED, D2D1_BITMAP_BRUSH_PROPERTIES,
                D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
                D2D1_BITMAP_OPTIONS_NONE, D2D1_BITMAP_OPTIONS_TARGET, D2D1_BITMAP_PROPERTIES1,
                D2D1_BRUSH_PROPERTIES, D2D1_CHANNEL_SELECTOR_G, D2D1_CHANNEL_SELECTOR_R,
                D2D1_DEVICE_CONTEXT_OPTIONS_NONE, D2D1_DISPLACEMENTMAP_PROP_SCALE,
                D2D1_DISPLACEMENTMAP_PROP_X_CHANNEL_SELECT,
                D2D1_DISPLACEMENTMAP_PROP_Y_CHANNEL_SELECT,
                D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT, D2D1_ELLIPSE, D2D1_EXTEND_MODE_CLAMP,
                D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_GAMMA_2_2,
                D2D1_GAUSSIANBLUR_PROP_BORDER_MODE, D2D1_GAUSSIANBLUR_PROP_STANDARD_DEVIATION,
                D2D1_IMAGE_BRUSH_PROPERTIES, D2D1_INTERPOLATION_MODE_LINEAR,
                D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES, D2D1_PROPERTY_TYPE_ENUM,
                D2D1_PROPERTY_TYPE_FLOAT, D2D1_ROUNDED_RECT, D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE,
                D2D1CreateFactory, ID2D1Bitmap1, ID2D1DeviceContext, ID2D1Effect, ID2D1Factory1,
                ID2D1Image, ID2D1ImageBrush, ID2D1RenderTarget, ID2D1SolidColorBrush,
            },
            Direct3D::{
                D3D_DRIVER_TYPE, D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_UNKNOWN,
                D3D_DRIVER_TYPE_WARP,
            },
            Direct3D11::{
                D3D11_BIND_SHADER_RESOURCE, D3D11_BOX, D3D11_CPU_ACCESS_READ,
                D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE,
                D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT, D3D11_USAGE_STAGING,
                D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Multithread,
                ID3D11Texture2D,
            },
            DirectComposition::{
                DCompositionCreateDevice, IDCompositionDevice, IDCompositionTarget,
                IDCompositionVisual,
            },
            DirectWrite::{
                DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_WEIGHT_SEMI_BOLD,
                DWRITE_MEASURING_MODE_NATURAL, DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
                DWRITE_PARAGRAPH_ALIGNMENT_NEAR, DWRITE_TEXT_ALIGNMENT_CENTER,
                DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_TEXT_ALIGNMENT_TRAILING, DWRITE_TEXT_METRICS,
                DWRITE_TRIMMING, DWRITE_TRIMMING_GRANULARITY_CHARACTER,
                DWRITE_WORD_WRAPPING_NO_WRAP, DWRITE_WORD_WRAPPING_WRAP, DWriteCreateFactory,
                IDWriteFactory, IDWriteFontCollection, IDWriteTextFormat,
            },
            Dxgi::{
                Common::{
                    DXGI_ALPHA_MODE_PREMULTIPLIED, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_UNKNOWN,
                    DXGI_SAMPLE_DESC,
                },
                CreateDXGIFactory2, DXGI_CREATE_FACTORY_FLAGS, DXGI_GPU_PREFERENCE_MINIMUM_POWER,
                DXGI_PRESENT, DXGI_SCALING_STRETCH, DXGI_SWAP_CHAIN_DESC1, DXGI_SWAP_CHAIN_FLAG,
                DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL, DXGI_USAGE_RENDER_TARGET_OUTPUT, IDXGIAdapter,
                IDXGIAdapter1, IDXGIDevice, IDXGIFactory2, IDXGIFactory6, IDXGISurface,
                IDXGISwapChain1,
            },
            Gdi::HMONITOR,
        },
        System::WinRT::{
            Direct3D11::{CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess},
            Graphics::Capture::IGraphicsCaptureItemInterop,
        },
        UI::WindowsAndMessaging::PostMessageW,
    },
    core::{BOOL, Interface, PCWSTR, w},
};
use windows_numerics::{Matrix3x2, Vector2};

use super::window::WM_APP_BACKDROP;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Align {
    Left,
    Center,
    Right,
}

/// Images the scene can draw. Uploaded by the app, kept on the GPU.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ImageSlot {
    /// Album art.
    Art,
    /// Blurred, darkened album art used as a background.
    ArtSoft,
    /// A thumbnail or icon (clipboard picture, shelf file, launcher app), keyed by the app.
    Thumb(u64),
    /// The latest camera frame, for the mirror.
    Camera,
}

/// Most thumbnails kept on the GPU at once; the oldest are dropped first.
const MAX_THUMBS: usize = 160;

struct Image {
    bitmap: ID2D1Bitmap1,
    width: f32,
    height: f32,
}

#[derive(Debug, Clone, Copy)]
pub struct TextStyle {
    /// Draw with the Windows icon font (Segoe Fluent Icons) instead of the text font.
    pub icon: bool,
    /// Font size in pixels.
    pub size: f32,
    pub bold: bool,
    pub color: Rgba,
    pub align: Align,
}

fn color(c: Rgba) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: c.r,
        g: c.g,
        b: c.b,
        a: c.a,
    }
}

fn rounded(pill: &Pill) -> D2D1_ROUNDED_RECT {
    let r = pill.effective_radius();
    D2D1_ROUNDED_RECT {
        rect: D2D_RECT_F {
            left: pill.left,
            top: pill.top,
            right: pill.right,
            bottom: pill.bottom,
        },
        radiusX: r,
        radiusY: r,
    }
}

/// Live capture of the screen behind Pillbox, blurred and optionally refracted.
struct Backdrop {
    pool: Direct3D11CaptureFramePool,
    _session: GraphicsCaptureSession,
    /// Window-sized copy of what is behind Pillbox.
    tex: ID3D11Texture2D,
    /// CPU-readable copy, used only to detect unchanged frames.
    staging: ID3D11Texture2D,
    bitmap: ID2D1Bitmap1,
    blur: ID2D1Effect,
    blur_brush: ID2D1ImageBrush,
    bend: ID2D1Effect,
    bend_map: ID2D1Bitmap1,
    bend_brush: ID2D1ImageBrush,
    /// Window origin relative to the captured monitor, in pixels.
    origin: (u32, u32),
    /// Monitor size, to keep the copy box inside the captured frame.
    source: (u32, u32),
    dirty_regions: bool,
    last_hash: u64,
    last_blur: f32,
    /// False until the first frame has been copied; until then there is nothing to show.
    ready: bool,
}

pub struct Renderer {
    hwnd: HWND,
    size: (u32, u32),
    device: ID3D11Device,
    ctx: ID3D11DeviceContext,
    dxgi_device: IDXGIDevice,
    swapchain: IDXGISwapChain1,
    dc: ID2D1DeviceContext,
    brush: ID2D1SolidColorBrush,
    dwrite: IDWriteFactory,
    /// The text and icon font families this PC has (`Fonts::pick`).
    fonts: Fonts,
    formats: HashMap<(u32, bool, Align, bool, bool), IDWriteTextFormat>,
    /// Widths of measured text by (text, quarter-pixel size, bold): labels and numbers repeat
    /// from frame to frame, and laying text out is the costly part of measuring.
    widths: HashMap<(String, u32, bool), f32>,
    images: HashMap<ImageSlot, Image>,
    /// Upload order of thumbnails, for dropping the oldest.
    thumb_order: Vec<u64>,
    backdrop: Option<Backdrop>,
    dump_to: Option<PathBuf>,
    // Kept alive for as long as the window shows our content.
    _dcomp: IDCompositionDevice,
    _target: Option<IDCompositionTarget>,
    _visual: IDCompositionVisual,
}

/// Text fonts, best first: Windows 11 has Segoe UI Variable; Windows 10 only Segoe UI.
const TEXT_FONTS: [PCWSTR; 2] = [w!("Segoe UI Variable Display"), w!("Segoe UI")];
/// Icon fonts, best first: Windows 11 has Segoe Fluent Icons; Windows 10 has Segoe MDL2
/// Assets, with the same code points for every icon Pillbox uses.
const ICON_FONTS: [PCWSTR; 2] = [w!("Segoe Fluent Icons"), w!("Segoe MDL2 Assets")];

/// The first of `candidates` for which `installed` is true, else the last one (DirectWrite
/// then substitutes a font of its own rather than failing).
fn first_installed<T: Copy>(candidates: &[T], installed: impl Fn(T) -> bool) -> Option<T> {
    candidates
        .iter()
        .copied()
        .find(|c| installed(*c))
        .or_else(|| candidates.last().copied())
}

#[derive(Clone, Copy)]
struct Fonts {
    text: PCWSTR,
    icon: PCWSTR,
}

impl Fonts {
    /// Pick the font families this PC has. `PILLBOX_WIN10_FONTS=1` (developer aid) pretends the
    /// Windows 11 fonts are missing, to see the Windows 10 look.
    fn pick(dwrite: &IDWriteFactory) -> Self {
        let pretend_win10 = std::env::var_os("PILLBOX_WIN10_FONTS").is_some();
        let mut collection: Option<IDWriteFontCollection> = None;
        // SAFETY: the out-pointer is a valid local Option; the factory is alive.
        let _ = unsafe { dwrite.GetSystemFontCollection(&mut collection, false) };
        // A family counts only if it is installed and really has `probe` (a name alone is not
        // enough: a broken or partial copy of a font would draw empty boxes).
        let usable = |name: PCWSTR, probe: u32| -> bool {
            if pretend_win10 && (name == TEXT_FONTS[0] || name == ICON_FONTS[0]) {
                return false;
            }
            let Some(c) = &collection else {
                return false;
            };
            let (mut index, mut exists) = (0u32, BOOL(0));
            // SAFETY: `name` is a 'static wide literal; both out-pointers are valid locals; the
            // family and font are owned COM objects released on drop.
            unsafe {
                if c.FindFamilyName(name, &mut index, &mut exists).is_err() || !exists.as_bool() {
                    return false;
                }
                c.GetFontFamily(index)
                    .and_then(|family| {
                        family.GetFirstMatchingFont(
                            DWRITE_FONT_WEIGHT_NORMAL,
                            DWRITE_FONT_STRETCH_NORMAL,
                            DWRITE_FONT_STYLE_NORMAL,
                        )
                    })
                    .and_then(|font| font.HasCharacter(probe))
                    .is_ok_and(|has| has.as_bool())
            }
        };
        // 'A' for text; the Home icon (on every tab row) for icons.
        let fonts = Self {
            text: first_installed(&TEXT_FONTS, |n| usable(n, 'A' as u32)).unwrap_or(TEXT_FONTS[1]),
            icon: first_installed(&ICON_FONTS, |n| usable(n, 0xE80F)).unwrap_or(ICON_FONTS[1]),
        };
        // SAFETY: both are 'static, null-terminated wide literals.
        let (t, i) = unsafe { (fonts.text.to_string(), fonts.icon.to_string()) };
        tracing::info!(
            "fonts: {} and {}",
            t.unwrap_or_default(),
            i.unwrap_or_default()
        );
        fonts
    }
}

/// Create a D3D11 device, preferring the integrated GPU so the discrete one can stay asleep.
fn create_device(factory: &IDXGIFactory2) -> Result<(ID3D11Device, ID3D11DeviceContext)> {
    let low_power: Option<IDXGIAdapter> = factory
        .cast::<IDXGIFactory6>()
        .ok()
        // SAFETY: index 0 with a valid preference constant; the result is an owned interface.
        .and_then(|f6| unsafe {
            f6.EnumAdapterByGpuPreference::<IDXGIAdapter1>(0, DXGI_GPU_PREFERENCE_MINIMUM_POWER)
                .ok()
        })
        .and_then(|a| a.cast().ok());

    let attempts: [(Option<&IDXGIAdapter>, D3D_DRIVER_TYPE, &str); 3] = [
        (
            low_power.as_ref(),
            D3D_DRIVER_TYPE_UNKNOWN,
            "low-power adapter",
        ),
        (None, D3D_DRIVER_TYPE_HARDWARE, "default adapter"),
        (None, D3D_DRIVER_TYPE_WARP, "software (WARP)"),
    ];
    let mut last_err = anyhow!("no adapter tried");
    for (adapter, driver, label) in attempts {
        if driver == D3D_DRIVER_TYPE_UNKNOWN && adapter.is_none() {
            continue;
        }
        let mut device = None;
        let mut ctx = None;
        // SAFETY: the out-pointers are valid local Options; the adapter, when given, is alive.
        let result = unsafe {
            D3D11CreateDevice(
                adapter,
                driver,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut ctx),
            )
        };
        match (result, device, ctx) {
            (Ok(()), Some(device), Some(ctx)) => {
                tracing::info!("Direct3D device created on the {label}");
                return Ok((device, ctx));
            }
            (Err(e), _, _) => last_err = anyhow!("{label}: {e}"),
            _ => last_err = anyhow!("{label}: no device returned"),
        }
    }
    Err(last_err.context("creating a Direct3D 11 device"))
}

impl Renderer {
    pub fn new(hwnd: HWND, width: u32, height: u32) -> Result<Self> {
        // SAFETY: plain COM object creation. Every interface returned is owned by a windows-rs
        // smart pointer and released on drop; hwnd is the caller's live window.
        unsafe {
            let factory: IDXGIFactory2 = CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0))
                .context("creating the DXGI factory")?;
            let (device, ctx) = create_device(&factory)?;
            // The capture frame pool uses the device from its own thread.
            if let Ok(mt) = ctx.cast::<ID3D11Multithread>() {
                let _ = mt.SetMultithreadProtected(true);
            }
            let dxgi_device: IDXGIDevice = device.cast()?;

            let desc = DXGI_SWAP_CHAIN_DESC1 {
                Width: width.max(1),
                Height: height.max(1),
                Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
                BufferCount: 2,
                Scaling: DXGI_SCALING_STRETCH,
                SwapEffect: DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
                AlphaMode: DXGI_ALPHA_MODE_PREMULTIPLIED,
                ..Default::default()
            };
            let swapchain = factory
                .CreateSwapChainForComposition(&device, &desc, None)
                .context("creating the composition swap chain")?;

            let d2d_factory: ID2D1Factory1 =
                D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)
                    .context("creating the Direct2D factory")?;
            let dc = d2d_factory
                .CreateDevice(&dxgi_device)?
                .CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)?;
            // We lay out in physical pixels and apply the DPI scale ourselves.
            dc.SetDpi(96.0, 96.0);
            // ClearType needs an opaque background; Pillbox is drawn over transparency.
            dc.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
            let brush = dc.CreateSolidColorBrush(&color(Rgba::rgb(0)), None)?;
            let dwrite: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)
                .context("creating the DirectWrite factory")?;
            let fonts = Fonts::pick(&dwrite);

            let dcomp: IDCompositionDevice = DCompositionCreateDevice(&dxgi_device)
                .context("creating the composition device")?;
            let target = dcomp
                .CreateTargetForHwnd(hwnd, true)
                .context("creating the composition target")?;
            let visual = dcomp.CreateVisual()?;
            visual.SetContent(&swapchain)?;
            target.SetRoot(&visual)?;
            dcomp.Commit()?;

            let renderer = Self {
                hwnd,
                size: (width.max(1), height.max(1)),
                device,
                ctx,
                dxgi_device,
                swapchain,
                dc,
                brush,
                dwrite,
                fonts,
                formats: HashMap::new(),
                widths: HashMap::new(),
                images: HashMap::new(),
                thumb_order: Vec::new(),
                backdrop: None,
                dump_to: None,
                _dcomp: dcomp,
                _target: Some(target),
                _visual: visual,
            };
            renderer.bind_back_buffer()?;
            Ok(renderer)
        }
    }

    /// Throw everything away and start again on a fresh device (after the GPU device was lost).
    pub fn rebuild(&mut self) -> Result<()> {
        self.backdrop = None;
        // A window can have only one composition target, so the old one must go first.
        self._target = None;
        let dump_to = self.dump_to.take();
        *self = Self::new(self.hwnd, self.size.0, self.size.1)?;
        self.dump_to = dump_to;
        Ok(())
    }

    /// Point Direct2D at the swap chain's back buffer.
    fn bind_back_buffer(&self) -> Result<()> {
        // SAFETY: buffer 0 always exists; the bitmap properties struct outlives the call.
        unsafe {
            let surface: IDXGISurface = self.swapchain.GetBuffer(0)?;
            let bitmap = self.dc.CreateBitmapFromDxgiSurface(
                &surface,
                Some(&D2D1_BITMAP_PROPERTIES1 {
                    pixelFormat: D2D1_PIXEL_FORMAT {
                        format: DXGI_FORMAT_B8G8R8A8_UNORM,
                        alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                    },
                    dpiX: 96.0,
                    dpiY: 96.0,
                    bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET | D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
                    ..Default::default()
                }),
            )?;
            self.dc.SetTarget(&bitmap);
        }
        Ok(())
    }

    /// Resize the drawing surface. Any backdrop is dropped; enable it again afterwards.
    pub fn resize(&mut self, width: u32, height: u32) -> Result<()> {
        self.backdrop = None;
        self.size = (width.max(1), height.max(1));
        // SAFETY: the back buffer must have no outstanding references when ResizeBuffers runs,
        // so the Direct2D target is released first and re-bound afterwards.
        unsafe {
            self.dc.SetTarget(None::<&ID2D1Image>);
            self.swapchain
                .ResizeBuffers(
                    0,
                    self.size.0,
                    self.size.1,
                    DXGI_FORMAT_UNKNOWN,
                    DXGI_SWAP_CHAIN_FLAG(0),
                )
                .context("resizing the swap chain")?;
        }
        self.bind_back_buffer()
    }

    /// Save the next presented frame as a 32-bit BMP. Glass looks are excluded from screen
    /// capture, so this is the only way to see exactly what was drawn.
    pub fn request_dump(&mut self, path: PathBuf) {
        self.dump_to = Some(path);
    }

    pub fn has_backdrop(&self) -> bool {
        self.backdrop.is_some()
    }

    pub fn disable_backdrop(&mut self) {
        if self.backdrop.take().is_some() {
            tracing::info!("backdrop capture stopped");
        }
    }

    /// Start capturing `monitor` (an HMONITOR value). `origin` is the window's top-left corner
    /// relative to that monitor and `source` the monitor size, both in pixels.
    pub fn enable_backdrop(
        &mut self,
        monitor: isize,
        origin: (u32, u32),
        source: (u32, u32),
    ) -> Result<()> {
        self.backdrop = None;
        let (w, h) = self.size;
        // SAFETY: COM/WinRT calls with live objects and valid locals. The monitor handle comes
        // straight from the OS enumeration. The frame handler only posts a message.
        let backdrop = unsafe {
            let interop =
                windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()?;
            let item: GraphicsCaptureItem = interop
                .CreateForMonitor(HMONITOR(monitor as *mut c_void))
                .context("creating the capture item")?;
            let d3d: IDirect3DDevice =
                CreateDirect3D11DeviceFromDXGIDevice(&self.dxgi_device)?.cast()?;
            let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
                &d3d,
                DirectXPixelFormat::B8G8R8A8UIntNormalized,
                1,
                item.Size()?,
            )?;
            let hwnd_raw = self.hwnd.0 as isize;
            pool.FrameArrived(&TypedEventHandler::new(move |_, _| {
                // Runs on a capture thread: wake the UI thread and do nothing else here.
                let _ = PostMessageW(
                    Some(HWND(hwnd_raw as *mut c_void)),
                    WM_APP_BACKDROP,
                    WPARAM(0),
                    LPARAM(0),
                );
                Ok(())
            }))?;
            let session = pool.CreateCaptureSession(&item)?;
            // Windows 10 cannot turn the capture border off: it would draw a yellow frame
            // around the whole screen for as long as Pillbox shows glass. Fail instead, so the
            // glass surfaces fall back to their tinted look without the live blur.
            session
                .SetIsBorderRequired(false)
                .context("turning off the capture border (needs Windows 11)")?;
            if let Err(e) = session.SetIsCursorCaptureEnabled(false) {
                tracing::warn!("could not leave the pointer out of the capture: {e}");
            }
            let dirty_regions = session
                .SetDirtyRegionMode(GraphicsCaptureDirtyRegionMode::ReportOnly)
                .is_ok();
            session.StartCapture().context("starting the capture")?;

            let tex_desc = D3D11_TEXTURE2D_DESC {
                Width: w,
                Height: h,
                MipLevels: 1,
                ArraySize: 1,
                Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                Usage: D3D11_USAGE_DEFAULT,
                BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
                ..Default::default()
            };
            let mut tex = None;
            self.device
                .CreateTexture2D(&tex_desc, None, Some(&mut tex))?;
            let tex: ID3D11Texture2D = tex.ok_or_else(|| anyhow!("no backdrop texture"))?;
            let staging_desc = D3D11_TEXTURE2D_DESC {
                Usage: D3D11_USAGE_STAGING,
                BindFlags: 0,
                CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                ..tex_desc
            };
            let mut staging = None;
            self.device
                .CreateTexture2D(&staging_desc, None, Some(&mut staging))?;
            let staging: ID3D11Texture2D = staging.ok_or_else(|| anyhow!("no staging texture"))?;

            let surface: IDXGISurface = tex.cast()?;
            let bitmap = self.dc.CreateBitmapFromDxgiSurface(
                &surface,
                Some(&D2D1_BITMAP_PROPERTIES1 {
                    pixelFormat: D2D1_PIXEL_FORMAT {
                        format: DXGI_FORMAT_B8G8R8A8_UNORM,
                        alphaMode: D2D1_ALPHA_MODE_IGNORE,
                    },
                    dpiX: 96.0,
                    dpiY: 96.0,
                    bitmapOptions: D2D1_BITMAP_OPTIONS_NONE,
                    ..Default::default()
                }),
            )?;
            let blur = self.dc.CreateEffect(&CLSID_D2D1GaussianBlur)?;
            blur.SetInput(0, &bitmap, true);
            blur.SetValue(
                D2D1_GAUSSIANBLUR_PROP_BORDER_MODE.0 as u32,
                D2D1_PROPERTY_TYPE_ENUM,
                &D2D1_BORDER_MODE_HARD.0.to_ne_bytes(),
            )?;

            let bend_map = self.dc.CreateBitmap(
                D2D_SIZE_U {
                    width: w,
                    height: h,
                },
                None,
                0,
                &D2D1_BITMAP_PROPERTIES1 {
                    pixelFormat: D2D1_PIXEL_FORMAT {
                        format: DXGI_FORMAT_B8G8R8A8_UNORM,
                        alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                    },
                    dpiX: 96.0,
                    dpiY: 96.0,
                    bitmapOptions: D2D1_BITMAP_OPTIONS_NONE,
                    ..Default::default()
                },
            )?;
            let bend = self.dc.CreateEffect(&CLSID_D2D1DisplacementMap)?;
            bend.SetInput(0, &blur.GetOutput()?, true);
            bend.SetInput(1, &bend_map, true);
            bend.SetValue(
                D2D1_DISPLACEMENTMAP_PROP_X_CHANNEL_SELECT.0 as u32,
                D2D1_PROPERTY_TYPE_ENUM,
                &D2D1_CHANNEL_SELECTOR_R.0.to_ne_bytes(),
            )?;
            bend.SetValue(
                D2D1_DISPLACEMENTMAP_PROP_Y_CHANNEL_SELECT.0 as u32,
                D2D1_PROPERTY_TYPE_ENUM,
                &D2D1_CHANNEL_SELECTOR_G.0.to_ne_bytes(),
            )?;

            let brush_props = D2D1_IMAGE_BRUSH_PROPERTIES {
                sourceRectangle: D2D_RECT_F {
                    left: 0.0,
                    top: 0.0,
                    right: w as f32,
                    bottom: h as f32,
                },
                extendModeX: D2D1_EXTEND_MODE_CLAMP,
                extendModeY: D2D1_EXTEND_MODE_CLAMP,
                interpolationMode: D2D1_INTERPOLATION_MODE_LINEAR,
            };
            let blur_brush = self
                .dc
                .CreateImageBrush(&blur.GetOutput()?, &brush_props, None)?;
            let bend_brush = self
                .dc
                .CreateImageBrush(&bend.GetOutput()?, &brush_props, None)?;

            Backdrop {
                pool,
                _session: session,
                tex,
                staging,
                bitmap,
                blur,
                blur_brush,
                bend,
                bend_map,
                bend_brush,
                origin,
                source,
                dirty_regions,
                last_hash: 0,
                last_blur: -1.0,
                ready: false,
            }
        };
        tracing::info!(
            "backdrop capture started (dirty regions: {})",
            backdrop.dirty_regions
        );
        self.backdrop = Some(backdrop);
        Ok(())
    }

    /// Take any waiting capture frame. Returns true only if the pixels behind Pillbox
    /// actually changed, i.e. a redraw is needed.
    ///
    /// Windows delivers a new frame every time Pillbox itself redraws, even though the
    /// Pillbox is excluded from the capture (observed behaviour, not documented). Those frames
    /// are pixel-identical, so comparing a hash of the strip is what stops an endless loop.
    pub fn pump_backdrop(&mut self) -> Result<bool> {
        let (w, h) = self.size;
        let Some(b) = &mut self.backdrop else {
            return Ok(false);
        };
        let mut changed = false;
        // The strip we sample, grown by the reach of the blur, in monitor pixels.
        const REACH: i32 = 96;
        let (sl, st) = (b.origin.0 as i32 - REACH, b.origin.1 as i32 - REACH);
        let (sr, sb) = (
            (b.origin.0 + w) as i32 + REACH,
            (b.origin.1 + h) as i32 + REACH,
        );
        while let Ok(frame) = b.pool.TryGetNextFrame() {
            if b.dirty_regions && b.ready {
                let touches = match frame.DirtyRegions() {
                    Ok(rects) => rects
                        .into_iter()
                        .any(|r| r.X < sr && r.X + r.Width > sl && r.Y < sb && r.Y + r.Height > st),
                    Err(_) => true,
                };
                if !touches {
                    frame.Close()?;
                    continue;
                }
            }
            let right = (b.origin.0 + w).min(b.source.0);
            let bottom = (b.origin.1 + h).min(b.source.1);
            if right <= b.origin.0 || bottom <= b.origin.1 {
                frame.Close()?;
                continue;
            }
            let access: IDirect3DDxgiInterfaceAccess = frame.Surface()?.cast()?;
            // SAFETY: the frame surface is a texture on our device; the box is clamped to the
            // monitor size. The staging texture has the same size and format as `tex`, and the
            // mapped pointer is read only within RowPitch * height bytes, before Unmap.
            let hash = unsafe {
                let src: ID3D11Texture2D = access.GetInterface()?;
                let region = D3D11_BOX {
                    left: b.origin.0,
                    top: b.origin.1,
                    front: 0,
                    right,
                    bottom,
                    back: 1,
                };
                self.ctx
                    .CopySubresourceRegion(&b.tex, 0, 0, 0, 0, &src, 0, Some(&region));
                self.ctx.CopyResource(&b.staging, &b.tex);
                let mut map = D3D11_MAPPED_SUBRESOURCE::default();
                self.ctx
                    .Map(&b.staging, 0, D3D11_MAP_READ, 0, Some(&mut map))?;
                let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
                for y in 0..h as usize {
                    let row = std::slice::from_raw_parts(
                        (map.pData as *const u8).add(y * map.RowPitch as usize),
                        w as usize * 4,
                    );
                    for px in row.as_chunks::<8>().0 {
                        let v = u64::from_ne_bytes([
                            px[0], px[1], px[2], px[3], px[4], px[5], px[6], px[7],
                        ]);
                        hash = (hash ^ v).wrapping_mul(0x0000_0100_0000_01b3);
                    }
                }
                self.ctx.Unmap(&b.staging, 0);
                hash
            };
            frame.Close()?;
            if b.ready && hash == b.last_hash {
                continue;
            }
            b.last_hash = hash;
            b.ready = true;
            // Direct2D caches effect output; tell it the surface behind the bitmap changed.
            // SAFETY: both objects are live resources of our device context.
            unsafe { b.blur.SetInput(0, &b.bitmap, true) };
            changed = true;
        }
        Ok(changed)
    }

    /// Upload BGRA pixels (premultiplied, `width * height * 4` bytes) into an image slot.
    pub fn set_image(&mut self, slot: ImageSlot, bgra: &[u8], width: u32, height: u32) {
        if width == 0 || height == 0 || bgra.len() != (width * height * 4) as usize {
            self.images.remove(&slot);
            return;
        }
        // Same size as what the slot holds (live camera frames): copy into the existing
        // bitmap instead of creating a new one every frame.
        if let Some(image) = self.images.get(&slot)
            && image.width == width as f32
            && image.height == height as f32
        {
            // SAFETY: the buffer holds width * height * 4 bytes (checked above), matching the
            // bitmap's size; Direct2D copies the pixels before returning.
            let copied = unsafe {
                image
                    .bitmap
                    .CopyFromMemory(None, bgra.as_ptr().cast(), width * 4)
            };
            if copied.is_ok() {
                return;
            }
        }
        self.images.remove(&slot);
        if let ImageSlot::Thumb(key) = slot {
            self.thumb_order.retain(|k| *k != key);
            self.thumb_order.push(key);
            while self.thumb_order.len() > MAX_THUMBS {
                let old = self.thumb_order.remove(0);
                self.images.remove(&ImageSlot::Thumb(old));
            }
        }
        // SAFETY: the buffer length was checked against width * height * 4 just above, and
        // Direct2D copies the pixels before the call returns.
        let bitmap = unsafe {
            self.dc.CreateBitmap(
                D2D_SIZE_U { width, height },
                Some(bgra.as_ptr().cast()),
                width * 4,
                &D2D1_BITMAP_PROPERTIES1 {
                    pixelFormat: D2D1_PIXEL_FORMAT {
                        format: DXGI_FORMAT_B8G8R8A8_UNORM,
                        alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                    },
                    dpiX: 96.0,
                    dpiY: 96.0,
                    bitmapOptions: D2D1_BITMAP_OPTIONS_NONE,
                    ..Default::default()
                },
            )
        };
        match bitmap {
            Ok(bitmap) => {
                self.images.insert(
                    slot,
                    Image {
                        bitmap,
                        width: width as f32,
                        height: height as f32,
                    },
                );
            }
            Err(e) => tracing::warn!("could not create an image: {e}"),
        }
    }

    pub fn clear_image(&mut self, slot: ImageSlot) {
        self.images.remove(&slot);
    }

    /// Width in pixels of one line of text in this style. Works outside a frame too (the
    /// resting pill sizes itself to its text before it is drawn). Cached.
    pub fn measure(&mut self, text: &str, size: f32, bold: bool) -> f32 {
        if text.is_empty() {
            return 0.0;
        }
        let key = (text.to_string(), (size * 4.0).round() as u32, bold);
        if let Some(w) = self.widths.get(&key) {
            return *w;
        }
        let Some(format) = self.format(size, bold, Align::Left, false, false) else {
            return 0.0;
        };
        let wide: Vec<u16> = text.encode_utf16().collect();
        // SAFETY: the buffer and format outlive the call; the layout is an owned COM object and
        // the metrics struct is a valid local.
        unsafe {
            let Ok(layout) = self
                .dwrite
                .CreateTextLayout(&wide, &format, 100_000.0, 1000.0)
            else {
                return 0.0;
            };
            let mut m = DWRITE_TEXT_METRICS::default();
            if layout.GetMetrics(&mut m).is_err() {
                return 0.0;
            }
            if self.widths.len() > 1024 {
                self.widths.clear();
            }
            self.widths.insert(key, m.widthIncludingTrailingWhitespace);
            m.widthIncludingTrailingWhitespace
        }
    }

    fn format(
        &mut self,
        size: f32,
        bold: bool,
        align: Align,
        icon: bool,
        wrap: bool,
    ) -> Option<IDWriteTextFormat> {
        // Quarter-pixel buckets keep the cache small while sizes animate.
        let key = (
            (size * 4.0).round().max(4.0) as u32,
            bold,
            align,
            icon,
            wrap,
        );
        if let Some(f) = self.formats.get(&key) {
            return Some(f.clone());
        }
        if self.formats.len() > 256 {
            self.formats.clear();
        }
        // SAFETY: all strings are 'static wide literals; the returned format is an owned COM object.
        let format = unsafe {
            let f = self
                .dwrite
                .CreateTextFormat(
                    if icon {
                        self.fonts.icon
                    } else {
                        self.fonts.text
                    },
                    None,
                    if bold {
                        DWRITE_FONT_WEIGHT_SEMI_BOLD
                    } else {
                        DWRITE_FONT_WEIGHT_NORMAL
                    },
                    DWRITE_FONT_STYLE_NORMAL,
                    DWRITE_FONT_STRETCH_NORMAL,
                    key.0 as f32 / 4.0,
                    w!("en-us"),
                )
                .ok()?;
            let _ = f.SetTextAlignment(match align {
                Align::Left => DWRITE_TEXT_ALIGNMENT_LEADING,
                Align::Center => DWRITE_TEXT_ALIGNMENT_CENTER,
                Align::Right => DWRITE_TEXT_ALIGNMENT_TRAILING,
            });
            if wrap {
                let _ = f.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_NEAR);
                let _ = f.SetWordWrapping(DWRITE_WORD_WRAPPING_WRAP);
            } else {
                let _ = f.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
                let _ = f.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
            }
            // Text that does not fit ends in an ellipsis instead of spilling out.
            if let Ok(sign) = self.dwrite.CreateEllipsisTrimmingSign(&f) {
                let trimming = DWRITE_TRIMMING {
                    granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER,
                    delimiter: 0,
                    delimiterCount: 0,
                };
                let _ = f.SetTrimming(&trimming, &sign);
            }
            f
        };
        self.formats.insert(key, format.clone());
        Some(format)
    }

    /// Start a frame: the surface is cleared to fully transparent.
    pub fn begin(&mut self) -> Frame<'_> {
        // SAFETY: the device context and its target are alive for the lifetime of self.
        unsafe {
            self.dc.BeginDraw();
            self.dc.Clear(Some(&color(Rgba::new(0.0, 0.0, 0.0, 0.0))));
        }
        Frame { r: self }
    }

    fn dump(&self, path: &PathBuf) -> Result<()> {
        // SAFETY: the staging texture matches the back buffer description; the mapped pointer
        // is read only within RowPitch * Height bytes and before Unmap.
        unsafe {
            let back: ID3D11Texture2D = self.swapchain.GetBuffer(0)?;
            let mut desc = D3D11_TEXTURE2D_DESC::default();
            back.GetDesc(&mut desc);
            desc.Usage = D3D11_USAGE_STAGING;
            desc.BindFlags = 0;
            desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
            desc.MiscFlags = 0;
            let mut staging = None;
            self.device
                .CreateTexture2D(&desc, None, Some(&mut staging))?;
            let staging: ID3D11Texture2D = staging.ok_or_else(|| anyhow!("no staging texture"))?;
            self.ctx.CopyResource(&staging, &back);
            let mut map = D3D11_MAPPED_SUBRESOURCE::default();
            self.ctx
                .Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut map))?;
            let (w, h) = (desc.Width as usize, desc.Height as usize);
            let mut out = Vec::with_capacity(54 + w * h * 4);
            out.extend_from_slice(b"BM");
            out.extend_from_slice(&((54 + w * h * 4) as u32).to_le_bytes());
            out.extend_from_slice(&0u32.to_le_bytes());
            out.extend_from_slice(&54u32.to_le_bytes());
            out.extend_from_slice(&40u32.to_le_bytes());
            out.extend_from_slice(&(w as i32).to_le_bytes());
            out.extend_from_slice(&(-(h as i32)).to_le_bytes());
            out.extend_from_slice(&1u16.to_le_bytes());
            out.extend_from_slice(&32u16.to_le_bytes());
            out.extend_from_slice(&[0u8; 24]);
            for y in 0..h {
                let row = std::slice::from_raw_parts(
                    (map.pData as *const u8).add(y * map.RowPitch as usize),
                    w * 4,
                );
                out.extend_from_slice(row);
            }
            self.ctx.Unmap(&staging, 0);
            std::fs::write(path, out).with_context(|| format!("writing {}", path.display()))?;
        }
        Ok(())
    }
}

/// One frame being drawn. Coordinates are window pixels.
pub struct Frame<'a> {
    r: &'a mut Renderer,
}

impl Frame<'_> {
    pub fn fill_pill(&mut self, pill: &Pill, c: Rgba) {
        // SAFETY: brush and context are live; the rect struct is a local.
        unsafe {
            self.r.brush.SetColor(&color(c));
            self.r
                .dc
                .FillRoundedRectangle(&rounded(pill), &self.r.brush);
        }
    }

    /// Vertical gradient from the top of the pill to its bottom.
    pub fn fill_pill_gradient(&mut self, pill: &Pill, top: Rgba, bottom: Rgba) {
        if top == bottom {
            return self.fill_pill(pill, top);
        }
        let stops = [
            D2D1_GRADIENT_STOP {
                position: 0.0,
                color: color(top),
            },
            D2D1_GRADIENT_STOP {
                position: 1.0,
                color: color(bottom),
            },
        ];
        let props = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
            startPoint: Vector2 {
                X: 0.0,
                Y: pill.top.max(0.0),
            },
            endPoint: Vector2 {
                X: 0.0,
                Y: pill.bottom,
            },
        };
        // SAFETY: the stop array and properties are locals that outlive the calls; the created
        // brush is an owned COM object dropped at the end of this function.
        unsafe {
            let Ok(collection) = ID2D1RenderTarget::CreateGradientStopCollection(
                &self.r.dc,
                &stops,
                D2D1_GAMMA_2_2,
                D2D1_EXTEND_MODE_CLAMP,
            ) else {
                return;
            };
            if let Ok(brush) = self
                .r
                .dc
                .CreateLinearGradientBrush(&props, None, &collection)
            {
                self.r.dc.FillRoundedRectangle(&rounded(pill), &brush);
            }
        }
    }

    /// Horizontal gradient across the pill, left to right, through `stops` (position 0..1 along
    /// the pill, colour). Fewer than two stops fill one colour.
    pub fn fill_pill_hgradient(&mut self, pill: &Pill, stops: &[(f32, Rgba)]) {
        match stops {
            [] => {}
            [(_, c)] => self.fill_pill(pill, *c),
            _ => {
                if let Some(brush) = self.hgradient(pill, stops) {
                    // SAFETY: context and brush are live COM objects.
                    unsafe { self.r.dc.FillRoundedRectangle(&rounded(pill), &brush) };
                }
            }
        }
    }

    /// The outline of the pill (inside it, `width` thick), coloured like
    /// `fill_pill_hgradient`.
    pub fn stroke_pill_hgradient(&mut self, pill: &Pill, stops: &[(f32, Rgba)], width: f32) {
        let inset = Pill {
            left: pill.left + width / 2.0,
            top: pill.top + width / 2.0,
            right: pill.right - width / 2.0,
            bottom: pill.bottom - width / 2.0,
            radius: (pill.effective_radius() - width / 2.0).max(0.0),
        };
        match stops {
            [] => {}
            [(_, c)] => self.stroke_pill(pill, *c, width),
            _ => {
                if let Some(brush) = self.hgradient(pill, stops) {
                    // SAFETY: context and brush are live COM objects; the rect is a local.
                    unsafe {
                        self.r
                            .dc
                            .DrawRoundedRectangle(&rounded(&inset), &brush, width, None);
                    }
                }
            }
        }
    }

    /// A left-to-right gradient brush spanning the pill.
    fn hgradient(
        &mut self,
        pill: &Pill,
        stops: &[(f32, Rgba)],
    ) -> Option<windows::Win32::Graphics::Direct2D::ID2D1LinearGradientBrush> {
        self.linear_brush(stops, (pill.left, 0.0), (pill.right, 0.0))
    }

    /// Fill the pill with a gradient through `stops` (position 0..1, colour) running from
    /// `start` to `end` (window pixels), clamped beyond them. Fewer than two stops fill one
    /// colour.
    pub fn fill_pill_linear(
        &mut self,
        pill: &Pill,
        stops: &[(f32, Rgba)],
        start: (f32, f32),
        end: (f32, f32),
    ) {
        match stops {
            [] => {}
            [(_, c)] => self.fill_pill(pill, *c),
            _ => {
                if let Some(brush) = self.linear_brush(stops, start, end) {
                    // SAFETY: context and brush are live COM objects; the rect is a local.
                    unsafe { self.r.dc.FillRoundedRectangle(&rounded(pill), &brush) };
                }
            }
        }
    }

    /// The outline of the pill (inside it, `width` thick), coloured like `fill_pill_linear`.
    pub fn stroke_pill_linear(
        &mut self,
        pill: &Pill,
        stops: &[(f32, Rgba)],
        start: (f32, f32),
        end: (f32, f32),
        width: f32,
    ) {
        let inset = Pill {
            left: pill.left + width / 2.0,
            top: pill.top + width / 2.0,
            right: pill.right - width / 2.0,
            bottom: pill.bottom - width / 2.0,
            radius: (pill.effective_radius() - width / 2.0).max(0.0),
        };
        match stops {
            [] => {}
            [(_, c)] => self.stroke_pill(pill, *c, width),
            _ => {
                if let Some(brush) = self.linear_brush(stops, start, end) {
                    // SAFETY: context and brush are live COM objects; the rect is a local.
                    unsafe {
                        self.r
                            .dc
                            .DrawRoundedRectangle(&rounded(&inset), &brush, width, None);
                    }
                }
            }
        }
    }

    /// Clip everything drawn until `pop_fade_clip` to this rectangle, and fade it out over
    /// `fade_top` pixels at the top and `fade_bottom` at the bottom (a list that scrolls on
    /// past the edge). With no fade it is a plain clip.
    pub fn push_fade_clip(&mut self, rect: (f32, f32, f32, f32), fade_top: f32, fade_bottom: f32) {
        let (left, top, right, bottom) = rect;
        let height = (bottom - top).max(1.0);
        let (ft, fb) = (
            (fade_top / height).clamp(0.0, 0.5),
            (fade_bottom / height).clamp(0.0, 0.5),
        );
        let solid = Rgba::new(0.0, 0.0, 0.0, 1.0);
        let clear = solid.with_alpha(0.0);
        let mask = if ft <= 0.0 && fb <= 0.0 {
            None
        } else {
            let stops = [
                (0.0, if ft > 0.0 { clear } else { solid }),
                (ft, solid),
                (1.0 - fb, solid),
                (1.0, if fb > 0.0 { clear } else { solid }),
            ];
            self.linear_brush(&stops, (0.0, top), (0.0, bottom))
                .map(|b| windows::Win32::Graphics::Direct2D::ID2D1Brush::clone(&b))
        };
        let params = windows::Win32::Graphics::Direct2D::D2D1_LAYER_PARAMETERS1 {
            contentBounds: D2D_RECT_F {
                left,
                top,
                right,
                bottom,
            },
            geometricMask: std::mem::ManuallyDrop::new(None),
            maskAntialiasMode:
                windows::Win32::Graphics::Direct2D::D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
            maskTransform: Matrix3x2::identity(),
            opacity: 1.0,
            opacityBrush: std::mem::ManuallyDrop::new(mask),
            layerOptions: windows::Win32::Graphics::Direct2D::D2D1_LAYER_OPTIONS1_NONE,
        };
        // SAFETY: the context is in a BeginDraw; every push is paired with pop_fade_clip. The
        // layer holds its own reference to the brush while pushed, so ours is released after.
        unsafe {
            self.r.dc.PushLayer(&params, None);
        }
        drop(std::mem::ManuallyDrop::into_inner(params.opacityBrush));
    }

    pub fn pop_fade_clip(&mut self) {
        // SAFETY: paired with push_fade_clip by the caller.
        unsafe { self.r.dc.PopLayer() };
    }

    /// A gradient brush through `stops` from `start` to `end` (window pixels).
    fn linear_brush(
        &mut self,
        stops: &[(f32, Rgba)],
        start: (f32, f32),
        end: (f32, f32),
    ) -> Option<windows::Win32::Graphics::Direct2D::ID2D1LinearGradientBrush> {
        let stops: Vec<D2D1_GRADIENT_STOP> = stops
            .iter()
            .map(|(p, c)| D2D1_GRADIENT_STOP {
                position: p.clamp(0.0, 1.0),
                color: color(*c),
            })
            .collect();
        let props = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
            startPoint: Vector2 {
                X: start.0,
                Y: start.1,
            },
            endPoint: Vector2 { X: end.0, Y: end.1 },
        };
        // SAFETY: as in `fill_pill_gradient`: locals outlive the calls, the brush is owned.
        unsafe {
            let collection = ID2D1RenderTarget::CreateGradientStopCollection(
                &self.r.dc,
                &stops,
                D2D1_GAMMA_2_2,
                D2D1_EXTEND_MODE_CLAMP,
            )
            .ok()?;
            self.r
                .dc
                .CreateLinearGradientBrush(&props, None, &collection)
                .ok()
        }
    }

    pub fn stroke_pill(&mut self, pill: &Pill, c: Rgba, width: f32) {
        // Inset by half the stroke so the line stays inside the shape (and the window region).
        let inset = Pill {
            left: pill.left + width / 2.0,
            top: pill.top + width / 2.0,
            right: pill.right - width / 2.0,
            bottom: pill.bottom - width / 2.0,
            radius: (pill.effective_radius() - width / 2.0).max(0.0),
        };
        // SAFETY: brush and context are live; the rect struct is a local.
        unsafe {
            self.r.brush.SetColor(&color(c));
            self.r
                .dc
                .DrawRoundedRectangle(&rounded(&inset), &self.r.brush, width, None);
        }
    }

    pub fn fill_rect(&mut self, left: f32, top: f32, right: f32, bottom: f32, c: Rgba) {
        // SAFETY: brush and context are live; the rect struct is a local.
        unsafe {
            self.r.brush.SetColor(&color(c));
            self.r.dc.FillRectangle(
                &D2D_RECT_F {
                    left,
                    top,
                    right,
                    bottom,
                },
                &self.r.brush,
            );
        }
    }

    /// Whether an image is uploaded in this slot.
    pub fn has_image(&self, slot: ImageSlot) -> bool {
        self.r.images.contains_key(&slot)
    }

    /// Fill the pill with an uploaded image, scaled to cover it (cropping the overflow).
    /// Returns false if the slot is empty.
    pub fn fill_pill_image(&mut self, slot: ImageSlot, pill: &Pill, opacity: f32) -> bool {
        let Some(image) = self.r.images.get(&slot) else {
            return false;
        };
        let (w, h) = (pill.width(), pill.height());
        if w <= 0.0 || h <= 0.0 {
            return true;
        }
        let scale = (w / image.width).max(h / image.height);
        // Centre the scaled image on the pill.
        let dx = pill.left + (w - image.width * scale) / 2.0;
        let dy = pill.top + (h - image.height * scale) / 2.0;
        let brush_props = D2D1_BRUSH_PROPERTIES {
            opacity: opacity.clamp(0.0, 1.0),
            transform: Matrix3x2 {
                M11: scale,
                M12: 0.0,
                M21: 0.0,
                M22: scale,
                M31: dx,
                M32: dy,
            },
        };
        let bitmap_props = D2D1_BITMAP_BRUSH_PROPERTIES {
            extendModeX: D2D1_EXTEND_MODE_CLAMP,
            extendModeY: D2D1_EXTEND_MODE_CLAMP,
            interpolationMode: D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
        };
        // SAFETY: the bitmap is a live resource of this context; the property structs are
        // locals that outlive the call; the brush is dropped at the end of the block.
        unsafe {
            if let Ok(brush) = ID2D1RenderTarget::CreateBitmapBrush(
                &self.r.dc,
                &image.bitmap,
                Some(&bitmap_props),
                Some(&brush_props),
            ) {
                self.r.dc.FillRoundedRectangle(&rounded(pill), &brush);
            }
        }
        true
    }

    pub fn line(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, c: Rgba, width: f32) {
        // SAFETY: brush and context are live.
        unsafe {
            self.r.brush.SetColor(&color(c));
            self.r.dc.DrawLine(
                Vector2 { X: x0, Y: y0 },
                Vector2 { X: x1, Y: y1 },
                &self.r.brush,
                width,
                None,
            );
        }
    }

    pub fn fill_ellipse(&mut self, cx: f32, cy: f32, rx: f32, ry: f32, c: Rgba) {
        // SAFETY: brush and context are live; the ellipse struct is a local.
        unsafe {
            self.r.brush.SetColor(&color(c));
            self.r.dc.FillEllipse(
                &D2D1_ELLIPSE {
                    point: Vector2 { X: cx, Y: cy },
                    radiusX: rx,
                    radiusY: ry,
                },
                &self.r.brush,
            );
        }
    }

    /// Draw one line of text, vertically centred in the rectangle.
    pub fn text(
        &mut self,
        text: &str,
        left: f32,
        top: f32,
        right: f32,
        bottom: f32,
        style: TextStyle,
    ) {
        if text.is_empty() || style.color.a <= 0.003 || right <= left || bottom <= top {
            return;
        }
        self.draw_text(text, (left, top, right, bottom), style, false);
    }

    /// Text that wraps onto as many lines as fit, from the top of the rectangle; the last
    /// visible line ends in an ellipsis if there is more.
    pub fn text_wrapped(
        &mut self,
        text: &str,
        left: f32,
        top: f32,
        right: f32,
        bottom: f32,
        style: TextStyle,
    ) {
        if text.is_empty() || style.color.a <= 0.003 || right <= left || bottom <= top {
            return;
        }
        self.draw_text(text, (left, top, right, bottom), style, true);
    }

    /// Width in pixels of one line of text in this style.
    pub fn measure(&mut self, text: &str, size: f32, bold: bool) -> f32 {
        self.r.measure(text, size, bold)
    }

    /// Clip everything drawn until `pop_clip` to this rectangle.
    pub fn push_clip(&mut self, left: f32, top: f32, right: f32, bottom: f32) {
        // SAFETY: the context is in a BeginDraw; every push is paired with pop_clip.
        unsafe {
            self.r.dc.PushAxisAlignedClip(
                &D2D_RECT_F {
                    left,
                    top,
                    right,
                    bottom,
                },
                D2D1_ANTIALIAS_MODE_ALIASED,
            );
        }
    }

    pub fn pop_clip(&mut self) {
        // SAFETY: paired with push_clip by the caller.
        unsafe { self.r.dc.PopAxisAlignedClip() };
    }

    fn draw_text(&mut self, text: &str, rect: (f32, f32, f32, f32), style: TextStyle, wrap: bool) {
        let (left, top, right, bottom) = rect;
        let Some(format) = self
            .r
            .format(style.size, style.bold, style.align, style.icon, wrap)
        else {
            return;
        };
        let wide: Vec<u16> = text.encode_utf16().collect();
        // SAFETY: the UTF-16 buffer, format and rect outlive the call.
        unsafe {
            self.r.brush.SetColor(&color(style.color));
            self.r.dc.DrawText(
                &wide,
                &format,
                &D2D_RECT_F {
                    left,
                    top,
                    right,
                    bottom,
                },
                &self.r.brush,
                D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT,
                DWRITE_MEASURING_MODE_NATURAL,
            );
        }
    }

    /// Fill the pill with the blurred screen behind it. `blur` is the blur radius in pixels.
    /// `bend` is an optional displacement map (window-sized BGRA) and its strength in pixels.
    /// Returns false if there is no backdrop to draw yet; the caller then draws a fallback.
    pub fn fill_pill_backdrop(
        &mut self,
        pill: &Pill,
        blur: f32,
        bend: Option<(&[u8], f32)>,
    ) -> bool {
        let (w, h) = self.r.size;
        let Some(b) = &mut self.r.backdrop else {
            return false;
        };
        if !b.ready {
            return false;
        }
        // SAFETY: all effects, bitmaps and brushes are live resources of this device context.
        // The displacement buffer is checked to hold exactly w*h BGRA pixels before the copy.
        unsafe {
            if (blur - b.last_blur).abs() > 0.01 {
                b.last_blur = blur;
                let _ = b.blur.SetValue(
                    D2D1_GAUSSIANBLUR_PROP_STANDARD_DEVIATION.0 as u32,
                    D2D1_PROPERTY_TYPE_FLOAT,
                    &blur.max(0.0).to_ne_bytes(),
                );
            }
            let shape = rounded(pill);
            match bend {
                Some((map, strength)) if map.len() == (w * h * 4) as usize => {
                    let _ = b.bend_map.CopyFromMemory(None, map.as_ptr().cast(), w * 4);
                    b.bend.SetInput(1, &b.bend_map, true);
                    let _ = b.bend.SetValue(
                        D2D1_DISPLACEMENTMAP_PROP_SCALE.0 as u32,
                        D2D1_PROPERTY_TYPE_FLOAT,
                        &strength.to_ne_bytes(),
                    );
                    self.r.dc.FillRoundedRectangle(&shape, &b.bend_brush);
                }
                _ => self.r.dc.FillRoundedRectangle(&shape, &b.blur_brush),
            }
        }
        true
    }

    /// Finish the frame and show it. An error usually means the GPU device was lost (driver
    /// update, sleep, GPU switch); the caller rebuilds the renderer.
    pub fn finish(self) -> Result<()> {
        // SAFETY: matches the BeginDraw in Renderer::begin; the swap chain is live.
        unsafe {
            self.r.dc.EndDraw(None, None).context("Direct2D EndDraw")?;
            if let Some(path) = self.r.dump_to.take() {
                match self.r.dump(&path) {
                    Ok(()) => tracing::info!("frame dumped to {}", path.display()),
                    Err(e) => tracing::warn!("frame dump failed: {e:#}"),
                }
            }
            self.r
                .swapchain
                .Present(1, DXGI_PRESENT(0))
                .ok()
                .context("Present")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::first_installed;

    #[test]
    fn the_best_installed_font_wins_and_the_last_is_the_fallback() {
        let fonts = ["Segoe Fluent Icons", "Segoe MDL2 Assets"];
        // Windows 11: both installed, the first wins.
        assert_eq!(
            first_installed(&fonts, |_| true),
            Some("Segoe Fluent Icons")
        );
        // Windows 10: only the second.
        assert_eq!(
            first_installed(&fonts, |f| f == "Segoe MDL2 Assets"),
            Some("Segoe MDL2 Assets")
        );
        // Neither found: the last one, for DirectWrite to substitute.
        assert_eq!(
            first_installed(&fonts, |_| false),
            Some("Segoe MDL2 Assets")
        );
        assert_eq!(first_installed::<&str>(&[], |_| true), None);
    }
}
