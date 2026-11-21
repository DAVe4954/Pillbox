//! File icons and thumbnails (shelf, launcher) and pictures for the photo widget, made on a
//! worker thread so Pillbox never waits for the disk or the shell.

use std::{
    path::PathBuf,
    sync::mpsc::{self, Receiver, Sender},
};

use anyhow::{Result, anyhow};
use pillbox_core::image::Bgra;
use windows::{
    Win32::{
        Foundation::SIZE,
        Graphics::Gdi::{
            BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, DIB_RGB_COLORS,
            DeleteDC, DeleteObject, GetDIBits, GetObjectW, HBITMAP, HGDIOBJ,
        },
        System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx},
        UI::Shell::{IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_RESIZETOFIT},
    },
    core::HSTRING,
};

use super::{media, window::Waker};

pub enum Job {
    /// Shell icon or thumbnail of a file, `size` pixels square.
    File { key: u64, path: PathBuf, size: u32 },
    /// Decode a picture file, longest side `max` pixels.
    Picture { key: u64, path: PathBuf, max: u32 },
    /// Decode picture bytes already in memory (an app's logo), longest side `max` pixels.
    Bytes { key: u64, data: Vec<u8>, max: u32 },
}

pub struct Done {
    pub key: u64,
    pub image: Option<Bgra>,
}

pub struct ThumbLoader {
    tx: Option<Sender<Job>>,
    rx: Receiver<Done>,
}

impl ThumbLoader {
    pub fn start(waker: Waker) -> Self {
        let (tx, jobs) = mpsc::channel::<Job>();
        let (done_tx, rx) = mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("thumbnails".into())
            .spawn(move || {
                // SAFETY: initialises COM for this worker thread only; the shell wants an STA.
                let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
                for job in jobs {
                    let (key, image) = match job {
                        Job::File { key, path, size } => (key, file_image(&path, size).ok()),
                        Job::Picture { key, path, max } => (
                            key,
                            std::fs::read(&path)
                                .ok()
                                .and_then(|b| media::decode_image(&b, max).ok())
                                .and_then(|a| Bgra::new(a.width, a.height, a.bgra)),
                        ),
                        Job::Bytes { key, data, max } => (
                            key,
                            media::decode_image(&data, max)
                                .ok()
                                .and_then(|a| Bgra::new(a.width, a.height, a.bgra)),
                        ),
                    };
                    if done_tx.send(Done { key, image }).is_err() {
                        return;
                    }
                    waker.wake();
                }
            });
        let tx = match spawned {
            Ok(_) => Some(tx),
            Err(e) => {
                tracing::warn!("no thumbnails: {e}");
                None
            }
        };
        Self { tx, rx }
    }

    pub fn request(&self, job: Job) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(job);
        }
    }

    pub fn poll(&self) -> Vec<Done> {
        self.rx.try_iter().collect()
    }
}

/// What Explorer would show for this file: a thumbnail for pictures and videos, else its icon.
fn file_image(path: &std::path::Path, size: u32) -> Result<Bgra> {
    let wide = HSTRING::from(path.as_os_str());
    // SAFETY: COM is initialised on this thread; the HBITMAP returned by GetImage is ours and is
    // deleted below after its pixels were copied into a buffer of exactly w*h*4 bytes.
    unsafe {
        let factory: IShellItemImageFactory = SHCreateItemFromParsingName(&wide, None)?;
        let hbmp: HBITMAP = factory.GetImage(
            SIZE {
                cx: size as i32,
                cy: size as i32,
            },
            SIIGBF_RESIZETOFIT,
        )?;
        let result = bitmap_pixels(hbmp);
        let _ = DeleteObject(HGDIOBJ(hbmp.0));
        result
    }
}

/// Copy a 32-bit HBITMAP's pixels, top-down. Shell bitmaps carry premultiplied alpha; ones
/// with no alpha at all (most photo thumbnails) are made opaque.
unsafe fn bitmap_pixels(hbmp: HBITMAP) -> Result<Bgra> {
    let mut bm = BITMAP::default();
    // SAFETY (caller): hbmp is a live bitmap. `bm` is a valid out buffer of its exact size.
    let got = unsafe {
        GetObjectW(
            HGDIOBJ(hbmp.0),
            size_of::<BITMAP>() as i32,
            Some((&mut bm as *mut BITMAP).cast()),
        )
    };
    if got == 0 || bm.bmWidth <= 0 || bm.bmHeight == 0 {
        return Err(anyhow!("not a bitmap"));
    }
    let (w, h) = (bm.bmWidth, bm.bmHeight.abs());
    let mut pixels = vec![0u8; (w * h * 4) as usize];
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
    // SAFETY: a memory DC is created and deleted here; the buffer holds w*h 32-bit pixels.
    let rows = unsafe {
        let dc = CreateCompatibleDC(None);
        let rows = GetDIBits(
            dc,
            hbmp,
            0,
            h as u32,
            Some(pixels.as_mut_ptr().cast()),
            &mut bmi,
            DIB_RGB_COLORS,
        );
        let _ = DeleteDC(dc);
        rows
    };
    if rows != h {
        return Err(anyhow!("GetDIBits copied {rows} of {h} rows"));
    }
    let mut img = Bgra::new(w as u32, h as u32, pixels).ok_or_else(|| anyhow!("bad size"))?;
    if img.pixels.as_chunks::<4>().0.iter().all(|p| p[3] == 0) {
        img.make_opaque();
    }
    Ok(img)
}
