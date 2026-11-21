//! Text out of a picture with the OCR engine built into Windows (`Windows.Media.Ocr`), in the
//! user's own languages. Blocking; call it from a worker thread.

use anyhow::{Context, Result, anyhow};
use pillbox_core::image::Bgra;
use windows::{
    Graphics::Imaging::{BitmapAlphaMode, BitmapPixelFormat, SoftwareBitmap},
    Media::Ocr::OcrEngine,
    Storage::Streams::DataWriter,
    Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize},
};

pub fn recognize(img: &Bgra) -> Result<String> {
    // SAFETY: initialising the WinRT apartment for this worker thread; harmless if already set.
    let _ = unsafe { RoInitialize(RO_INIT_MULTITHREADED) };
    let max = OcrEngine::MaxImageDimension().unwrap_or(10_000);
    let img = if img.width > max || img.height > max {
        img.thumbnail(max)
    } else {
        img.clone()
    };
    let writer = DataWriter::new()?;
    writer.WriteBytes(&img.pixels)?;
    let buffer = writer.DetachBuffer()?;
    let bitmap = SoftwareBitmap::CreateCopyWithAlphaFromBuffer(
        &buffer,
        BitmapPixelFormat::Bgra8,
        img.width as i32,
        img.height as i32,
        BitmapAlphaMode::Premultiplied,
    )
    .context("making a bitmap for OCR")?;
    let engine = OcrEngine::TryCreateFromUserProfileLanguages()
        .map_err(|e| anyhow!("no OCR language installed: {e}"))?;
    let result = engine.RecognizeAsync(&bitmap)?.get()?;
    let mut lines = Vec::new();
    for line in result.Lines()? {
        lines.push(line.Text()?.to_string());
    }
    Ok(lines.join("\n"))
}
