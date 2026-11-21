//! Screen pixels: a screenshot of the monitor under the pointer, the colour under the pointer,
//! and where screenshots are saved.

use std::path::PathBuf;

use anyhow::{Context, Result, anyhow};
use pillbox_core::image::Bgra;
use windows::Win32::{
    Foundation::POINT,
    Graphics::Gdi::{
        BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CAPTUREBLT, CLR_INVALID,
        CreateCompatibleBitmap, CreateCompatibleDC, DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC,
        GetDIBits, GetMonitorInfoW, GetPixel, MONITOR_DEFAULTTONEAREST, MONITORINFO,
        MonitorFromPoint, ReleaseDC, SRCCOPY, SelectObject,
    },
    UI::{
        Shell::{FOLDERID_Pictures, KF_FLAG_DEFAULT, SHGetKnownFolderPath},
        WindowsAndMessaging::GetCursorPos,
    },
};

pub fn cursor_pos() -> Option<(i32, i32)> {
    let mut pt = POINT::default();
    // SAFETY: pt is a valid local.
    unsafe { GetCursorPos(&mut pt) }.ok()?;
    Some((pt.x, pt.y))
}

/// The colour of the screen pixel under the pointer, as (red, green, blue).
pub fn pixel_under_cursor() -> Option<(u8, u8, u8)> {
    let (x, y) = cursor_pos()?;
    // SAFETY: the screen DC is obtained and released here; GetPixel only reads.
    let c = unsafe {
        let dc = GetDC(None);
        if dc.is_invalid() {
            return None;
        }
        let c = GetPixel(dc, x, y);
        ReleaseDC(None, dc);
        c
    };
    if c.0 == CLR_INVALID {
        return None;
    }
    // COLORREF is 0x00BBGGRR.
    Some((
        (c.0 & 0xFF) as u8,
        ((c.0 >> 8) & 0xFF) as u8,
        ((c.0 >> 16) & 0xFF) as u8,
    ))
}

/// Capture the whole monitor the pointer is on, as opaque BGRA. Windows that asked to be left
/// out of captures (including Pillbox on glass surfaces) are left out.
pub fn capture_monitor_under_cursor() -> Result<Bgra> {
    let (cx, cy) = cursor_pos().ok_or_else(|| anyhow!("no pointer position"))?;
    let mut info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    // SAFETY: MonitorFromPoint always returns a monitor with DEFAULTTONEAREST; info is a valid
    // local with its size set.
    unsafe {
        let mon = MonitorFromPoint(POINT { x: cx, y: cy }, MONITOR_DEFAULTTONEAREST);
        if !GetMonitorInfoW(mon, &mut info).as_bool() {
            return Err(anyhow!("GetMonitorInfoW failed"));
        }
    }
    let r = info.rcMonitor;
    let (w, h) = (r.right - r.left, r.bottom - r.top);
    if w <= 0 || h <= 0 {
        return Err(anyhow!("empty monitor"));
    }
    let mut pixels = vec![0u8; w as usize * h as usize * 4];
    // SAFETY: every GDI object created here is selected out and deleted before returning. The
    // pixel buffer holds exactly w*h 32-bit pixels, which is what the BITMAPINFO asks for
    // (negative height = top-down rows).
    let rows = unsafe {
        let screen = GetDC(None);
        if screen.is_invalid() {
            return Err(anyhow!("no screen DC"));
        }
        let mem = CreateCompatibleDC(Some(screen));
        let bmp = CreateCompatibleBitmap(screen, w, h);
        let old = SelectObject(mem, bmp.into());
        let blit = BitBlt(
            mem,
            0,
            0,
            w,
            h,
            Some(screen),
            r.left,
            r.top,
            SRCCOPY | CAPTUREBLT,
        );
        SelectObject(mem, old);
        let mut bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let rows = if blit.is_ok() {
            GetDIBits(
                mem,
                bmp,
                0,
                h as u32,
                Some(pixels.as_mut_ptr().cast()),
                &mut bmi,
                DIB_RGB_COLORS,
            )
        } else {
            0
        };
        let _ = DeleteObject(bmp.into());
        let _ = DeleteDC(mem);
        ReleaseDC(None, screen);
        rows
    };
    if rows != h {
        return Err(anyhow!("screen copy failed ({rows} of {h} rows)"));
    }
    let mut img = Bgra::new(w as u32, h as u32, pixels).ok_or_else(|| anyhow!("bad size"))?;
    img.make_opaque();
    Ok(img)
}

/// `Pictures\Screenshots`, created if missing (the same folder Windows' own screenshots use).
pub fn screenshots_dir() -> Result<PathBuf> {
    // SAFETY: SHGetKnownFolderPath allocates the string with CoTaskMemAlloc; it is copied into
    // a Rust String and then freed exactly once with CoTaskMemFree.
    let pictures = unsafe {
        let p = SHGetKnownFolderPath(&FOLDERID_Pictures, KF_FLAG_DEFAULT, None)
            .context("locating the Pictures folder")?;
        let s = p.to_string();
        windows::Win32::System::Com::CoTaskMemFree(Some(p.0 as *const _));
        s.context("Pictures path is not valid text")?
    };
    let dir = PathBuf::from(pictures).join("Screenshots");
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    Ok(dir)
}
