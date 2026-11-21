//! Small image helpers on plain BGRA buffers: PNG encode/decode, thumbnails, premultiplying.
//! Used for screenshots, clipboard pictures and file thumbnails.

/// An image as 32-bit BGRA rows, top to bottom, `width * height * 4` bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bgra {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl Bgra {
    pub fn new(width: u32, height: u32, pixels: Vec<u8>) -> Option<Self> {
        (width > 0 && height > 0 && pixels.len() == width as usize * height as usize * 4).then_some(
            Self {
                width,
                height,
                pixels,
            },
        )
    }

    /// Set every alpha byte to 255 (screen captures and DIBs often leave it at 0).
    pub fn make_opaque(&mut self) {
        for px in self.pixels.as_chunks_mut::<4>().0 {
            px[3] = 255;
        }
    }

    /// Straight alpha to premultiplied, which is what Direct2D bitmaps expect.
    pub fn premultiply(&mut self) {
        for px in self.pixels.as_chunks_mut::<4>().0 {
            let a = u32::from(px[3]);
            if a < 255 {
                for c in &mut px[..3] {
                    *c = ((u32::from(*c) * a + 127) / 255) as u8;
                }
            }
        }
    }

    /// A copy that fits inside `max` × `max`, averaging the pixels it shrinks (box filter).
    /// Images already small enough are returned unchanged.
    pub fn thumbnail(&self, max: u32) -> Bgra {
        let max = max.max(1);
        if self.width <= max && self.height <= max {
            return self.clone();
        }
        let scale = (f64::from(max) / f64::from(self.width.max(self.height))).min(1.0);
        let tw = ((f64::from(self.width) * scale).round() as u32).max(1);
        let th = ((f64::from(self.height) * scale).round() as u32).max(1);
        let mut out = vec![0u8; tw as usize * th as usize * 4];
        let (sw, sh) = (self.width as usize, self.height as usize);
        for ty in 0..th as usize {
            let y0 = ty * sh / th as usize;
            let y1 = ((ty + 1) * sh / th as usize).max(y0 + 1).min(sh);
            for tx in 0..tw as usize {
                let x0 = tx * sw / tw as usize;
                let x1 = ((tx + 1) * sw / tw as usize).max(x0 + 1).min(sw);
                let mut sum = [0u32; 4];
                for y in y0..y1 {
                    let row = &self.pixels[(y * sw + x0) * 4..(y * sw + x1) * 4];
                    for px in row.as_chunks::<4>().0 {
                        for (s, v) in sum.iter_mut().zip(px) {
                            *s += u32::from(*v);
                        }
                    }
                }
                let n = ((y1 - y0) * (x1 - x0)) as u32;
                let o = (ty * tw as usize + tx) * 4;
                for (c, s) in sum.iter().enumerate() {
                    out[o + c] = (s / n) as u8;
                }
            }
        }
        Bgra {
            width: tw,
            height: th,
            pixels: out,
        }
    }

    /// Encode as PNG (straight alpha, as PNG requires).
    pub fn to_png(&self) -> Result<Vec<u8>, String> {
        let mut rgba = self.pixels.clone();
        for px in rgba.as_chunks_mut::<4>().0 {
            px.swap(0, 2);
        }
        let mut out = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut out, self.width, self.height);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder.set_compression(png::Compression::Fast);
            let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
            writer.write_image_data(&rgba).map_err(|e| e.to_string())?;
        }
        Ok(out)
    }

    /// Decode a PNG into straight-alpha BGRA. Only 8-bit images are supported, which covers
    /// everything this app writes itself.
    pub fn from_png(bytes: &[u8]) -> Result<Bgra, String> {
        let mut decoder = png::Decoder::new(bytes);
        decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
        let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
        let mut buf = vec![0u8; reader.output_buffer_size()];
        let info = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
        let (w, h) = (info.width, info.height);
        let n = w as usize * h as usize;
        let mut out = Vec::with_capacity(n * 4);
        match info.color_type {
            png::ColorType::Rgba => {
                for px in buf[..n * 4].as_chunks::<4>().0 {
                    out.extend_from_slice(&[px[2], px[1], px[0], px[3]]);
                }
            }
            png::ColorType::Rgb => {
                for px in buf[..n * 3].as_chunks::<3>().0 {
                    out.extend_from_slice(&[px[2], px[1], px[0], 255]);
                }
            }
            png::ColorType::GrayscaleAlpha => {
                for px in buf[..n * 2].as_chunks::<2>().0 {
                    out.extend_from_slice(&[px[0], px[0], px[0], px[1]]);
                }
            }
            png::ColorType::Grayscale => {
                for v in &buf[..n] {
                    out.extend_from_slice(&[*v, *v, *v, 255]);
                }
            }
            png::ColorType::Indexed => return Err("indexed PNG was not expanded".into()),
        }
        Bgra::new(w, h, out).ok_or_else(|| "empty PNG".to_string())
    }
}

/// `#RRGGBB` for a colour given as red, green, blue.
pub fn hex(r: u8, g: u8, b: u8) -> String {
    format!("#{r:02X}{g:02X}{b:02X}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn checker(w: u32, h: u32) -> Bgra {
        let mut px = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let v = if (x + y) % 2 == 0 { 255 } else { 0 };
                px.extend_from_slice(&[v, 0, 255 - v, 255]);
            }
        }
        match Bgra::new(w, h, px) {
            Some(b) => b,
            None => panic!("bad test image"),
        }
    }

    #[test]
    fn rejects_wrong_sizes() {
        assert!(Bgra::new(2, 2, vec![0; 15]).is_none());
        assert!(Bgra::new(0, 2, vec![]).is_none());
        assert!(Bgra::new(2, 2, vec![0; 16]).is_some());
    }

    #[test]
    fn png_round_trips() {
        let img = checker(7, 5);
        let png = match img.to_png() {
            Ok(p) => p,
            Err(e) => panic!("{e}"),
        };
        assert_eq!(&png[1..4], b"PNG");
        let back = match Bgra::from_png(&png) {
            Ok(b) => b,
            Err(e) => panic!("{e}"),
        };
        assert_eq!(back, img);
    }

    #[test]
    fn garbage_is_not_a_png() {
        assert!(Bgra::from_png(b"definitely not a png").is_err());
    }

    #[test]
    fn thumbnail_keeps_aspect_and_averages() {
        let img = checker(400, 200);
        let t = img.thumbnail(100);
        assert_eq!((t.width, t.height), (100, 50));
        // A fine checkerboard averages to mid grey-ish purple.
        let p = &t.pixels[0..4];
        assert!((120..=135).contains(&p[0]), "{p:?}");
        assert_eq!(p[3], 255);
        // Small images are left alone.
        assert_eq!(checker(10, 10).thumbnail(100), checker(10, 10));
    }

    #[test]
    fn premultiply_and_opaque() {
        let mut img = match Bgra::new(1, 1, vec![200, 100, 50, 128]) {
            Some(b) => b,
            None => panic!(),
        };
        img.premultiply();
        assert_eq!(img.pixels, vec![100, 50, 25, 128]);
        img.make_opaque();
        assert_eq!(img.pixels[3], 255);
    }

    #[test]
    fn hex_is_upper_case() {
        assert_eq!(hex(0x7c, 0x5c, 0xff), "#7C5CFF");
    }
}
