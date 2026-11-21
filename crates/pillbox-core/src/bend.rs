//! The "bend" of Liquid Glass: a displacement map that makes the backdrop refract near the rim.
//!
//! Each pixel says where to sample the blurred backdrop from, relative to itself. Red holds the
//! horizontal offset and green the vertical one; 128 means "no offset". Near the outline the
//! sample point is pushed towards the centre of the shape, like the edge of a thick lens.

use crate::shape::Pill;

/// Byte value meaning "no displacement".
pub const NEUTRAL: u8 = 128;

/// Fill `buf` with a `width` x `height` BGRA displacement map for `pill`.
/// `bezel` is how far in from the outline the bending reaches, in pixels.
pub fn displacement_map(buf: &mut Vec<u8>, width: usize, height: usize, pill: &Pill, bezel: f32) {
    buf.clear();
    buf.resize(width * height * 4, 0);
    for px in buf.as_chunks_mut::<4>().0 {
        px.copy_from_slice(&[0, NEUTRAL, NEUTRAL, 255]);
    }
    if bezel <= 0.0 || width == 0 || height == 0 {
        return;
    }
    // Only the pixels the pill covers need work.
    let x0 = pill.left.floor().max(0.0) as usize;
    let y0 = pill.top.floor().max(0.0) as usize;
    let x1 = (pill.right.ceil().max(0.0) as usize).min(width);
    let y1 = (pill.bottom.ceil().max(0.0) as usize).min(height);
    for y in y0..y1 {
        for x in x0..x1 {
            let (d, normal) = pill.distance_and_normal(x as f32 + 0.5, y as f32 + 0.5);
            let depth = -d;
            if depth <= 0.0 || depth >= bezel {
                continue;
            }
            // Strongest at the outline, fading smoothly to nothing at the bezel's inner edge.
            let t = 1.0 - depth / bezel;
            let strength = t * t;
            let i = (y * width + x) * 4;
            // Sample from further inside: opposite to the outward normal.
            buf[i + 2] = (f32::from(NEUTRAL) - normal.0 * strength * 127.0).round() as u8;
            buf[i + 1] = (f32::from(NEUTRAL) - normal.1 * strength * 127.0).round() as u8;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pill() -> Pill {
        Pill {
            left: 10.0,
            top: 10.0,
            right: 110.0,
            bottom: 70.0,
            radius: 20.0,
        }
    }

    fn at(buf: &[u8], width: usize, x: usize, y: usize) -> (u8, u8) {
        let i = (y * width + x) * 4;
        (buf[i + 2], buf[i + 1])
    }

    #[test]
    fn centre_and_outside_are_neutral() {
        let mut buf = Vec::new();
        displacement_map(&mut buf, 120, 80, &pill(), 12.0);
        assert_eq!(buf.len(), 120 * 80 * 4);
        assert_eq!(at(&buf, 120, 60, 40), (NEUTRAL, NEUTRAL));
        assert_eq!(at(&buf, 120, 2, 2), (NEUTRAL, NEUTRAL));
        assert!(
            buf.as_chunks::<4>()
                .0
                .iter()
                .all(|p| p[3] == 255 && p[0] == 0)
        );
    }

    #[test]
    fn edges_sample_towards_the_centre() {
        let mut buf = Vec::new();
        displacement_map(&mut buf, 120, 80, &pill(), 12.0);
        // Left edge: sample from the right (red above neutral), no vertical shift.
        let (r, g) = at(&buf, 120, 11, 40);
        assert!(r > NEUTRAL + 60, "r={r}");
        assert_eq!(g, NEUTRAL);
        // Right edge mirrors it.
        let (r, _) = at(&buf, 120, 108, 40);
        assert!(r < NEUTRAL - 60, "r={r}");
        // Bottom edge: sample from above (green below neutral).
        let (r, g) = at(&buf, 120, 60, 68);
        assert_eq!(r, NEUTRAL);
        assert!(g < NEUTRAL - 60, "g={g}");
    }

    #[test]
    fn bending_fades_with_depth() {
        let mut buf = Vec::new();
        displacement_map(&mut buf, 120, 80, &pill(), 12.0);
        let shift = |x| i32::from(at(&buf, 120, x, 40).0) - i32::from(NEUTRAL);
        assert!(shift(11) > shift(15));
        assert!(shift(15) > shift(20));
        assert_eq!(shift(23), 0, "beyond the bezel");
    }

    #[test]
    fn handles_pills_partly_off_the_window_and_zero_bezel() {
        let mut buf = Vec::new();
        let notch = Pill {
            top: -30.0,
            ..pill()
        };
        displacement_map(&mut buf, 120, 80, &notch, 12.0);
        assert_eq!(buf.len(), 120 * 80 * 4);
        displacement_map(&mut buf, 120, 80, &pill(), 0.0);
        assert!(
            buf.as_chunks::<4>()
                .0
                .iter()
                .all(|p| p[1] == NEUTRAL && p[2] == NEUTRAL)
        );
        displacement_map(&mut buf, 0, 0, &pill(), 5.0);
        assert!(buf.is_empty());
    }
}
