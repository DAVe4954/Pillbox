//! The app icon, drawn in code so the exe, the tray and the Settings window all show the same
//! picture at any size: a violet-to-pink rounded square with the black pill on top and a hint
//! of the opened card under it. Nothing is stored as a picture file.
//!
//! `render` gives straight-alpha RGBA. The `ico_*` and `group_*` functions pack the same
//! drawing into the formats Windows wants (an `.ico` file, and the resources inside an exe).

/// Sizes packed into the icon, smallest first. 256 is what Explorer shows for large icons.
pub const SIZES: [u32; 6] = [16, 24, 32, 48, 64, 256];

type Rgb = [f32; 3];

/// Signed distance (in unit-square units) from a point to a rounded rectangle.
fn round_rect(px: f32, py: f32, cx: f32, cy: f32, hw: f32, hh: f32, r: f32) -> f32 {
    let qx = (px - cx).abs() - (hw - r);
    let qy = (py - cy).abs() - (hh - r);
    qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - r
}

/// Coverage of a pixel from a distance already in pixels: a one pixel soft edge.
fn cover(distance_px: f32) -> f32 {
    (0.5 - distance_px).clamp(0.0, 1.0)
}

/// Lay `color` with opacity `a` over the premultiplied pixel `dst`.
fn over(dst: &mut [f32; 4], color: Rgb, a: f32) {
    for (d, c) in dst.iter_mut().zip(color) {
        *d = c * a + *d * (1.0 - a);
    }
    dst[3] = a + dst[3] * (1.0 - a);
}

fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

/// The icon at `size` × `size` pixels, straight-alpha RGBA, top row first.
pub fn render(size: u32) -> Vec<u8> {
    let s = size.max(1) as f32;
    let violet: Rgb = [0.486, 0.361, 1.0];
    let pink: Rgb = [1.0, 0.361, 0.659];
    let black: Rgb = [0.03, 0.03, 0.05];
    let white: Rgb = [1.0, 1.0, 1.0];
    let detail = size >= 32;
    let mut out = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let (u, v) = ((x as f32 + 0.5) / s, (y as f32 + 0.5) / s);
            let mut px = [0.0f32; 4];
            // The rounded square, shaded from violet (top left) to pink (bottom right).
            let square = cover(round_rect(u, v, 0.5, 0.5, 0.47, 0.47, 0.22) * s);
            over(
                &mut px,
                mix(violet, pink, ((u + v) / 2.0 - 0.1) * 1.25),
                square,
            );
            // A soft light from the top.
            let sheen = (1.0 - v * 1.6).clamp(0.0, 1.0) * 0.16;
            over(&mut px, white, sheen * square);
            // The opened card, as a pale shape under the pill.
            let card = cover(round_rect(u, v, 0.5, 0.72, 0.31, 0.13, 0.09) * s);
            over(&mut px, white, 0.26 * card * square);
            // The pill.
            let pill = cover(round_rect(u, v, 0.5, 0.36, 0.33, 0.11, 0.11) * s);
            over(&mut px, black, pill);
            if detail {
                // The pill's dot on the left and three bars of the visualizer on the right.
                let dot = cover(((u - 0.26).hypot(v - 0.36) - 0.035) * s);
                over(&mut px, white, 0.9 * dot);
                for (cx, half) in [(0.60, 0.022), (0.67, 0.045), (0.74, 0.03)] {
                    let bar = cover(round_rect(u, v, cx, 0.36, 0.012, half, 0.012) * s);
                    over(&mut px, white, 0.92 * bar);
                }
            }
            let a = px[3];
            for c in &px[..3] {
                let straight = if a > 0.0 { c / a } else { 0.0 };
                out.push((straight.clamp(0.0, 1.0) * 255.0).round() as u8);
            }
            out.push((a.clamp(0.0, 1.0) * 255.0).round() as u8);
        }
    }
    out
}

/// One image of an icon as stored in an `.ico` file or an `RT_ICON` resource: a BITMAPINFOHEADER
/// (the height counts the colour rows and the mask rows), BGRA rows bottom-up, then an all-zero
/// AND mask (the alpha channel does the cutting).
pub fn dib(size: u32) -> Vec<u8> {
    let rgba = render(size);
    let mask_row = size.div_ceil(32) * 4;
    let mut out = Vec::with_capacity(40 + rgba.len() + (mask_row * size) as usize);
    let push32 = |out: &mut Vec<u8>, v: u32| out.extend_from_slice(&v.to_le_bytes());
    push32(&mut out, 40); // biSize
    push32(&mut out, size); // biWidth
    push32(&mut out, size * 2); // biHeight: image plus mask
    out.extend_from_slice(&1u16.to_le_bytes()); // biPlanes
    out.extend_from_slice(&32u16.to_le_bytes()); // biBitCount
    push32(&mut out, 0); // biCompression
    push32(&mut out, size * size * 4 + mask_row * size); // biSizeImage
    for _ in 0..4 {
        push32(&mut out, 0); // resolution and palette counts
    }
    for row in (0..size as usize).rev() {
        let line = &rgba[row * size as usize * 4..][..size as usize * 4];
        for px in line.as_chunks::<4>().0 {
            out.extend_from_slice(&[px[2], px[1], px[0], px[3]]);
        }
    }
    out.resize(out.len() + (mask_row * size) as usize, 0);
    out
}

/// A size as an icon directory stores it: 256 is written as 0.
fn dir_size(size: u32) -> u8 {
    if size >= 256 { 0 } else { size as u8 }
}

/// All images of the icon, each as `(size, dib)`.
pub fn images() -> Vec<(u32, Vec<u8>)> {
    SIZES.iter().map(|&s| (s, dib(s))).collect()
}

/// The icon as an `.ico` file.
pub fn ico_file() -> Vec<u8> {
    let images = images();
    let mut out = Vec::new();
    out.extend_from_slice(&[0, 0, 1, 0]); // reserved, type 1 = icon
    out.extend_from_slice(&(images.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * images.len() as u32;
    for (size, data) in &images {
        out.extend_from_slice(&[dir_size(*size), dir_size(*size), 0, 0]);
        out.extend_from_slice(&1u16.to_le_bytes()); // planes
        out.extend_from_slice(&32u16.to_le_bytes()); // bits per pixel
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        offset += data.len() as u32;
    }
    for (_, data) in &images {
        out.extend_from_slice(data);
    }
    out
}

/// The `RT_GROUP_ICON` resource that names the images (`RT_ICON` resources numbered from 1).
pub fn group_resource() -> Vec<u8> {
    let images = images();
    let mut out = Vec::new();
    out.extend_from_slice(&[0, 0, 1, 0]);
    out.extend_from_slice(&(images.len() as u16).to_le_bytes());
    for (i, (size, data)) in images.iter().enumerate() {
        out.extend_from_slice(&[dir_size(*size), dir_size(*size), 0, 0]);
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&32u16.to_le_bytes());
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&(i as u16 + 1).to_le_bytes()); // the RT_ICON id
    }
    out
}

/// A compiled Windows resource file (`.res`) holding the icon as resource group 1, ready for
/// the linker: the empty marker entry, one `RT_ICON` per size, then the `RT_GROUP_ICON`.
pub fn res_file() -> Vec<u8> {
    const RT_ICON: u16 = 3;
    const RT_GROUP_ICON: u16 = 14;
    fn entry(out: &mut Vec<u8>, kind: u16, id: u16, data: &[u8]) {
        out.extend_from_slice(&(data.len() as u32).to_le_bytes()); // data size
        out.extend_from_slice(&32u32.to_le_bytes()); // header size
        for v in [0xFFFFu16, kind, 0xFFFF, id] {
            out.extend_from_slice(&v.to_le_bytes()); // type and name, both by number
        }
        out.extend_from_slice(&0u32.to_le_bytes()); // data version
        out.extend_from_slice(&0x1030u16.to_le_bytes()); // movable, pure, discardable
        out.extend_from_slice(&0u16.to_le_bytes()); // language neutral
        out.extend_from_slice(&0u32.to_le_bytes()); // version
        out.extend_from_slice(&0u32.to_le_bytes()); // characteristics
        out.extend_from_slice(data);
        while !out.len().is_multiple_of(4) {
            out.push(0);
        }
    }
    let mut out = Vec::new();
    entry(&mut out, 0, 0, &[]); // the empty entry every .res starts with
    for (i, (_, data)) in images().iter().enumerate() {
        entry(&mut out, RT_ICON, i as u16 + 1, data);
    }
    entry(&mut out, RT_GROUP_ICON, 1, &group_resource());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixel(rgba: &[u8], size: u32, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * size + x) * 4) as usize;
        [rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]]
    }

    #[test]
    fn corners_are_clear_and_the_middle_is_solid() {
        for size in SIZES {
            let img = render(size);
            assert_eq!(img.len(), (size * size * 4) as usize);
            assert_eq!(pixel(&img, size, 0, 0)[3], 0, "{size}px corner");
            assert_eq!(pixel(&img, size, size - 1, size - 1)[3], 0);
            assert_eq!(
                pixel(&img, size, size / 2, size / 2)[3],
                255,
                "{size}px middle"
            );
        }
    }

    #[test]
    fn the_pill_is_dark_on_a_coloured_square() {
        let img = render(64);
        let pill = pixel(&img, 64, 32, 23);
        assert!(
            pill[..3].iter().all(|&c| c < 40),
            "the pill is near black: {pill:?}"
        );
        let square = pixel(&img, 64, 8, 32);
        assert!(
            square[0].max(square[2]) > 150,
            "the square is vivid: {square:?}"
        );
    }

    #[test]
    fn rendering_is_deterministic() {
        assert_eq!(render(48), render(48));
    }

    #[test]
    fn a_dib_has_its_header_colour_rows_and_mask() {
        let d = dib(16);
        assert_eq!(u32::from_le_bytes(d[0..4].try_into().unwrap()), 40);
        assert_eq!(u32::from_le_bytes(d[4..8].try_into().unwrap()), 16);
        assert_eq!(u32::from_le_bytes(d[8..12].try_into().unwrap()), 32);
        // 40 header bytes, 16x16 BGRA pixels, 16 mask rows of 4 bytes.
        assert_eq!(d.len(), 40 + 16 * 16 * 4 + 16 * 4);
    }

    #[test]
    fn the_ico_file_directory_points_at_every_image() {
        let ico = ico_file();
        assert_eq!(&ico[..4], &[0, 0, 1, 0]);
        let count = u16::from_le_bytes([ico[4], ico[5]]) as usize;
        assert_eq!(count, SIZES.len());
        let mut end = 0;
        for i in 0..count {
            let e = &ico[6 + i * 16..][..16];
            let len = u32::from_le_bytes(e[8..12].try_into().unwrap()) as usize;
            let off = u32::from_le_bytes(e[12..16].try_into().unwrap()) as usize;
            assert_eq!(e[0], dir_size(SIZES[i]));
            assert!(off + len <= ico.len());
            end = end.max(off + len);
        }
        assert_eq!(end, ico.len());
    }

    #[test]
    fn the_resource_file_is_aligned_and_starts_with_the_empty_entry() {
        let res = res_file();
        assert_eq!(res.len() % 4, 0);
        // Empty entry: no data, 32 byte header, type and name 0xFFFF 0.
        assert_eq!(&res[..8], &[0, 0, 0, 0, 32, 0, 0, 0]);
        assert_eq!(&res[8..16], &[0xFF, 0xFF, 0, 0, 0xFF, 0xFF, 0, 0]);
        // Walk every entry: sizes must add up exactly to the file.
        let mut at = 0;
        let mut kinds = Vec::new();
        while at < res.len() {
            let data = u32::from_le_bytes(res[at..at + 4].try_into().unwrap()) as usize;
            let header = u32::from_le_bytes(res[at + 4..at + 8].try_into().unwrap()) as usize;
            kinds.push(u16::from_le_bytes([res[at + 10], res[at + 11]]));
            at += header + data.div_ceil(4) * 4;
        }
        assert_eq!(at, res.len());
        assert_eq!(kinds.len(), SIZES.len() + 2);
        assert_eq!(kinds.last(), Some(&14), "the group comes last");
    }
}
