//! Writes the app icon next to you: `pillbox.ico` and a 256 px `Pillbox.png` to look at.
//!
//! `cargo run -p pillbox-core --example appicon -- <folder>`

use std::{fs, path::PathBuf};

fn main() -> std::io::Result<()> {
    let dir = PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| ".".to_string()));
    fs::create_dir_all(&dir)?;
    fs::write(dir.join("pillbox.ico"), pillbox_core::appicon::ico_file())?;
    for size in pillbox_core::appicon::SIZES {
        let file = fs::File::create(dir.join(format!("pillbox-{size}.png")))?;
        let mut enc = png::Encoder::new(file, size, size);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut writer = enc.write_header().map_err(std::io::Error::other)?;
        writer
            .write_image_data(&pillbox_core::appicon::render(size))
            .map_err(std::io::Error::other)?;
    }
    println!("wrote the icon to {}", dir.display());
    Ok(())
}
