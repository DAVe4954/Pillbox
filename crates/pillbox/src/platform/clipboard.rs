//! Reading and writing the Windows clipboard: text, file lists and pictures.
//!
//! Change notifications come from `AddClipboardFormatListener` (see `PillboxWindow`); this
//! module only opens the clipboard, reads what the app wants and writes items back.

use std::path::PathBuf;

use anyhow::{Result, anyhow};
use pillbox_core::image::Bgra;
use windows::{
    Win32::{
        Foundation::{GlobalFree, HANDLE, HGLOBAL, HWND},
        Graphics::Gdi::{BI_BITFIELDS, BI_RGB, BITMAPINFOHEADER},
        System::{
            DataExchange::{
                CloseClipboard, EmptyClipboard, GetClipboardData, GetClipboardOwner,
                IsClipboardFormatAvailable, OpenClipboard, RegisterClipboardFormatW,
                SetClipboardData,
            },
            Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock},
            Ole::{CF_DIB, CF_HDROP, CF_UNICODETEXT},
        },
        UI::Shell::{DROPFILES, DragQueryFileW, HDROP},
    },
    core::w,
};

use super::foreground;

/// What was on the clipboard.
#[derive(Debug, Clone, PartialEq)]
pub enum Content {
    Text(String),
    Files(Vec<PathBuf>),
    Picture(Bgra),
}

/// Why nothing was read.
#[derive(Debug)]
pub enum Skip {
    /// The app that put it there asked for it not to be recorded (password managers do).
    Excluded,
    /// From an app on the user's ignore list.
    IgnoredApp(String),
    /// Nothing we keep (empty, or only formats we do not understand).
    Nothing,
}

/// Opens the clipboard and closes it again when dropped, whatever happens in between.
struct Open;

impl Open {
    fn new(hwnd: HWND) -> Result<Self> {
        // SAFETY: hwnd is Pillbox's live window, which becomes the clipboard owner on Empty.
        unsafe { OpenClipboard(Some(hwnd)) }.map_err(|e| anyhow!("clipboard is busy: {e}"))?;
        Ok(Self)
    }
}

impl Drop for Open {
    fn drop(&mut self) {
        // SAFETY: paired with the successful OpenClipboard in `new`.
        let _ = unsafe { CloseClipboard() };
    }
}

fn format_id(name: windows::core::PCWSTR) -> u32 {
    // SAFETY: name is a 'static wide literal. Registering an existing name returns its id.
    unsafe { RegisterClipboardFormatW(name) }
}

/// Bytes of a clipboard format, copied out while the clipboard is open.
fn data(format: u32) -> Option<Vec<u8>> {
    // SAFETY: the clipboard is open (callers hold an `Open`). The global memory handle stays
    // valid until CloseClipboard; it is locked, read within GlobalSize bytes, and unlocked.
    unsafe {
        let handle = GetClipboardData(format).ok()?;
        let global = HGLOBAL(handle.0);
        let size = GlobalSize(global);
        let ptr = GlobalLock(global);
        if ptr.is_null() || size == 0 {
            return None;
        }
        let bytes = std::slice::from_raw_parts(ptr as *const u8, size).to_vec();
        let _ = GlobalUnlock(global);
        Some(bytes)
    }
}

fn available(format: u32) -> bool {
    // SAFETY: no pointers.
    unsafe { IsClipboardFormatAvailable(format) }.is_ok()
}

/// Read the clipboard, with the executable name of the app that put it there (may be empty).
/// `ignore` lists executable names whose copies are not recorded.
pub fn read(hwnd: HWND, ignore: &[String]) -> Result<std::result::Result<(Content, String), Skip>> {
    // The owner is checked first, so a copy from an ignored app is never read at all.
    // SAFETY: no arguments; may return an error when there is no owner.
    let source = unsafe { GetClipboardOwner() }
        .ok()
        .and_then(foreground::exe_of)
        .unwrap_or_default();
    if !source.is_empty() && ignore.iter().any(|i| i.eq_ignore_ascii_case(&source)) {
        return Ok(Err(Skip::IgnoredApp(source)));
    }
    read_content(hwnd).map(|r| r.map(|c| (c, source)))
}

/// Plain text on the clipboard, if any (for pasting into Pillbox's text boxes).
pub fn read_text(hwnd: HWND) -> Option<String> {
    let _open = Open::new(hwnd).ok()?;
    data(u32::from(CF_UNICODETEXT.0)).and_then(|b| wide_text(&b))
}

fn read_content(hwnd: HWND) -> Result<std::result::Result<Content, Skip>> {
    let _open = Open::new(hwnd)?;
    // Documented opt-out formats: an app sets these to keep its copy out of history and away
    // from clipboard monitors.
    if available(format_id(w!(
        "ExcludeClipboardContentFromMonitorProcessing"
    ))) || available(format_id(w!("Clipboard Viewer Ignore")))
    {
        return Ok(Err(Skip::Excluded));
    }
    if let Some(flag) = data(format_id(w!("CanIncludeInClipboardHistory")))
        && flag.len() >= 4
        && u32::from_le_bytes([flag[0], flag[1], flag[2], flag[3]]) == 0
    {
        return Ok(Err(Skip::Excluded));
    }
    if let Some(files) = read_files() {
        return Ok(Ok(Content::Files(files)));
    }
    if let Some(text) = data(u32::from(CF_UNICODETEXT.0)).and_then(|b| wide_text(&b))
        && !text.trim().is_empty()
    {
        return Ok(Ok(Content::Text(text)));
    }
    // Browsers put a "PNG" copy next to the bitmap; it keeps transparency, so prefer it.
    if let Some(img) = data(format_id(w!("PNG"))).and_then(|b| Bgra::from_png(&b).ok()) {
        return Ok(Ok(Content::Picture(img)));
    }
    if let Some(img) = data(u32::from(CF_DIB.0)).and_then(|b| parse_dib(&b)) {
        return Ok(Ok(Content::Picture(img)));
    }
    Ok(Err(Skip::Nothing))
}

fn wide_text(bytes: &[u8]) -> Option<String> {
    let units: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .take_while(|&u| u != 0)
        .collect();
    Some(String::from_utf16_lossy(&units))
}

fn read_files() -> Option<Vec<PathBuf>> {
    // SAFETY: the clipboard is open; the HDROP handle is valid until it closes. Each name is
    // read into a buffer sized from the length the first call reports.
    unsafe {
        let handle = GetClipboardData(u32::from(CF_HDROP.0)).ok()?;
        let drop = HDROP(handle.0);
        let count = DragQueryFileW(drop, u32::MAX, None);
        let mut out = Vec::with_capacity(count as usize);
        for i in 0..count {
            let len = DragQueryFileW(drop, i, None) as usize;
            let mut buf = vec![0u16; len + 1];
            let got = DragQueryFileW(drop, i, Some(&mut buf)) as usize;
            out.push(PathBuf::from(String::from_utf16_lossy(
                &buf[..got.min(len)],
            )));
        }
        (!out.is_empty()).then_some(out)
    }
}

/// A packed device-independent bitmap (CF_DIB): header, optional masks, then pixel rows.
/// 24- and 32-bit images are supported, which is what screenshots and apps produce.
fn parse_dib(bytes: &[u8]) -> Option<Bgra> {
    let le32 = |o: usize| -> Option<u32> {
        bytes
            .get(o..o + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    };
    let header_size = le32(0)? as usize;
    let width = le32(4)? as i32;
    let height = le32(8)? as i32;
    let bits = u16::from_le_bytes([*bytes.get(14)?, *bytes.get(15)?]);
    let compression = le32(16)?;
    if width <= 0 || height == 0 || !(bits == 24 || bits == 32) {
        return None;
    }
    let mut offset = header_size;
    // With a plain 40-byte header, BI_BITFIELDS puts three colour masks after it.
    if compression == BI_BITFIELDS.0 && header_size == size_of::<BITMAPINFOHEADER>() {
        offset += 12;
    } else if compression != BI_RGB.0 && compression != BI_BITFIELDS.0 {
        return None;
    }
    let (w, h) = (width as usize, height.unsigned_abs() as usize);
    let bpp = usize::from(bits / 8);
    let stride = (w * bpp).div_ceil(4) * 4;
    let pixels = bytes.get(offset..offset + stride * h)?;
    let mut out = vec![0u8; w * h * 4];
    let mut any_alpha = false;
    for y in 0..h {
        // Positive height = rows stored bottom-up.
        let src_row = if height > 0 { h - 1 - y } else { y };
        let row = &pixels[src_row * stride..src_row * stride + w * bpp];
        for x in 0..w {
            let s = &row[x * bpp..x * bpp + bpp];
            let o = (y * w + x) * 4;
            out[o..o + 3].copy_from_slice(&s[..3]);
            out[o + 3] = if bpp == 4 { s[3] } else { 255 };
            any_alpha |= out[o + 3] != 0;
        }
    }
    let mut img = Bgra::new(w as u32, h as u32, out)?;
    if !any_alpha {
        // Most apps leave the fourth byte at zero; that means opaque, not invisible.
        img.make_opaque();
    }
    Some(img)
}

/// Put one global-memory block on the clipboard under `format`. On success Windows owns it.
fn put(format: u32, bytes: &[u8]) -> Result<()> {
    // SAFETY: the block is allocated with the requested size, filled within that size while
    // locked, and either handed to the clipboard (which then owns it) or freed on failure.
    unsafe {
        let global = GlobalAlloc(GMEM_MOVEABLE, bytes.len().max(1))?;
        let ptr = GlobalLock(global);
        if ptr.is_null() {
            let _ = GlobalFree(Some(global));
            return Err(anyhow!("GlobalLock failed"));
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr as *mut u8, bytes.len());
        let _ = GlobalUnlock(global);
        if let Err(e) = SetClipboardData(format, Some(HANDLE(global.0))) {
            let _ = GlobalFree(Some(global));
            return Err(anyhow!("SetClipboardData: {e}"));
        }
    }
    Ok(())
}

fn wide_bytes(text: &str) -> Vec<u8> {
    text.encode_utf16()
        .chain(std::iter::once(0))
        .flat_map(u16::to_le_bytes)
        .collect()
}

/// Replace the clipboard contents with `content`.
pub fn write(hwnd: HWND, content: &Content) -> Result<()> {
    let _open = Open::new(hwnd)?;
    // SAFETY: the clipboard is open by us.
    unsafe { EmptyClipboard() }.map_err(|e| anyhow!("EmptyClipboard: {e}"))?;
    match content {
        Content::Text(text) => put(u32::from(CF_UNICODETEXT.0), &wide_bytes(text)),
        Content::Files(files) => {
            let header = DROPFILES {
                pFiles: size_of::<DROPFILES>() as u32,
                fWide: true.into(),
                ..Default::default()
            };
            // SAFETY: DROPFILES is a plain C struct; viewing it as bytes is sound.
            let mut bytes = unsafe {
                std::slice::from_raw_parts(
                    (&header as *const DROPFILES).cast::<u8>(),
                    size_of::<DROPFILES>(),
                )
            }
            .to_vec();
            for f in files {
                bytes.extend(wide_bytes(&f.to_string_lossy()));
            }
            bytes.extend([0, 0]);
            put(u32::from(CF_HDROP.0), &bytes)
        }
        Content::Picture(img) => {
            // A PNG copy keeps transparency for apps that read it; CF_DIB is for everyone else.
            if let Ok(png) = img.to_png() {
                let _ = put(format_id(w!("PNG")), &png);
            }
            put(u32::from(CF_DIB.0), &dib_bytes(img))
        }
    }
}

fn dib_bytes(img: &Bgra) -> Vec<u8> {
    let header = BITMAPINFOHEADER {
        biSize: size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: img.width as i32,
        // Positive height: bottom-up rows, which every reader understands.
        biHeight: img.height as i32,
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB.0,
        biSizeImage: img.width * img.height * 4,
        ..Default::default()
    };
    // SAFETY: BITMAPINFOHEADER is a plain C struct; viewing it as bytes is sound.
    let mut out = unsafe {
        std::slice::from_raw_parts(
            (&header as *const BITMAPINFOHEADER).cast::<u8>(),
            size_of::<BITMAPINFOHEADER>(),
        )
    }
    .to_vec();
    let row = img.width as usize * 4;
    for y in (0..img.height as usize).rev() {
        out.extend_from_slice(&img.pixels[y * row..(y + 1) * row]);
    }
    out
}
