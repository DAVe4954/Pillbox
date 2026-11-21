//! Developer aid (tests only): decode every QR code in a 32-bit BMP, such as Pillbox's frame
//! dump (`PILLBOX_DUMP`), and print each one's text on its own line.
//!
//! `cargo run -p pillbox-core --example qrdecode -- frame.bmp`

use std::process::ExitCode;

fn main() -> ExitCode {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: qrdecode <file.bmp>");
        return ExitCode::FAILURE;
    };
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("{path}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let read_i32 = |at: usize| {
        bytes
            .get(at..at + 4)
            .and_then(|s| s.try_into().ok())
            .map(i32::from_le_bytes)
    };
    let (Some(offset), Some(w), Some(h)) = (read_i32(10), read_i32(18), read_i32(22)) else {
        eprintln!("not a BMP");
        return ExitCode::FAILURE;
    };
    let (w, top_down, h) = (w.unsigned_abs() as usize, h < 0, h.unsigned_abs() as usize);
    let offset = offset as usize;
    if bytes.len() < offset + w * h * 4 {
        eprintln!("not a 32-bit BMP");
        return ExitCode::FAILURE;
    }
    let mut img = rqrr::PreparedImage::prepare_from_greyscale(w, h, |x, y| {
        let row = if top_down { y } else { h - 1 - y };
        let i = offset + (row * w + x) * 4;
        let (b, g, r, a) = (bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]);
        // Premultiplied BGRA over black: transparent parts read as dark.
        let lum = (u32::from(r) * 30 + u32::from(g) * 59 + u32::from(b) * 11) / 100;
        if a == 0 { 0 } else { lum as u8 }
    });
    let grids = img.detect_grids();
    if grids.is_empty() {
        eprintln!("no QR code found");
        return ExitCode::FAILURE;
    }
    for g in grids {
        match g.decode() {
            Ok((_, text)) => println!("{text}"),
            Err(e) => eprintln!("a QR code did not decode: {e:?}"),
        }
    }
    ExitCode::SUCCESS
}
