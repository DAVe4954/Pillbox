//! Pictures of a theme's body for the Settings previews: drawn pixel by pixel from
//! `Theme::body_painter`, the same layers (fill or blurred screen, fade, shimmer, sheen) Pillbox
//! paints with Direct2D, so what the preview shows is what the pill and the card will show.

use std::hash::{DefaultHasher, Hash, Hasher};

use eframe::egui::{
    self, Color32, ColorImage, Painter, Pos2, Rect, Stroke, StrokeKind, TextureHandle,
};
use pillbox_core::theme::{Rgba, Theme};

/// The pretend desktop behind the previews: a blue to purple wash, like the screenshots.
pub fn wallpaper(x: f32, y: f32) -> Rgba {
    let t = (0.35 * x + 0.65 * y).clamp(0.0, 1.0);
    Rgba::rgb(0x3A4B7A).lerp(Rgba::rgb(0x6B4A7E), t)
}

/// Paint `rect` with a picture of the theme's body `clock` seconds in, corners rounded by
/// `radius`, and the theme's rim. Glass shows the pretend desktop that fills `desk`. `slot`
/// names the picture so it is redrawn in place (one GPU texture per slot) rather than piling
/// up while colours are dragged.
pub fn body(p: &Painter, slot: &str, rect: Rect, radius: f32, t: &Theme, clock: f32, desk: Rect) {
    let ctx = p.ctx();
    let ppp = ctx.pixels_per_point();
    let (w, h) = (
        (rect.width() * ppp).round().max(1.0) as usize,
        (rect.height() * ppp).round().max(1.0) as usize,
    );
    // Where the picture sits on the desktop, as fractions of it.
    let place = (
        (rect.left() - desk.left()) / desk.width().max(1.0),
        (rect.top() - desk.top()) / desk.height().max(1.0),
        rect.width() / desk.width().max(1.0),
        rect.height() / desk.height().max(1.0),
    );
    let mut hasher = DefaultHasher::new();
    format!("{t:?}{place:?}").hash(&mut hasher);
    (w, h, (radius * 100.0) as i64, (clock * 1000.0) as i64).hash(&mut hasher);
    let key = hasher.finish();
    let id = egui::Id::new(("theme-preview", slot));
    let cached: Option<(u64, TextureHandle)> = ctx.data(|d| d.get_temp(id));
    let texture = match cached {
        Some((k, tex)) if k == key => tex,
        cached => {
            let image = picture(t, w, h, radius * ppp, clock, place);
            let tex = match cached {
                Some((_, mut tex)) => {
                    tex.set(image, egui::TextureOptions::LINEAR);
                    tex
                }
                None => ctx.load_texture(
                    format!("theme-preview-{slot}"),
                    image,
                    egui::TextureOptions::LINEAR,
                ),
            };
            ctx.data_mut(|d| d.insert_temp(id, (key, tex.clone())));
            tex
        }
    };
    p.image(
        texture.id(),
        rect,
        Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
        Color32::WHITE,
    );
    if let Some(rim) = t.rim {
        p.rect_stroke(
            rect,
            radius,
            Stroke::new(1.0_f32, color(rim)),
            StrokeKind::Inside,
        );
    }
}

/// The body as an image of `w`×`h` pixels with corners of `radius` pixels (anti-aliased).
fn picture(
    t: &Theme,
    w: usize,
    h: usize,
    radius: f32,
    clock: f32,
    (dx, dy, dw, dh): (f32, f32, f32, f32),
) -> ColorImage {
    let (wf, hf) = (w as f32, h as f32);
    let painter = t.body_painter(wf, hf, clock);
    let radius = radius.min(wf / 2.0).min(hf / 2.0);
    let mut pixels = Vec::with_capacity(w * h);
    for y in 0..h {
        for x in 0..w {
            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
            let desk = wallpaper(dx + dw * px / wf, dy + dh * py / hf);
            let c = painter.at(px, py, desk);
            let a = coverage(px, py, wf, hf, radius);
            pixels.push(Color32::from_rgba_unmultiplied(
                byte(c.r),
                byte(c.g),
                byte(c.b),
                byte(a),
            ));
        }
    }
    ColorImage {
        size: [w, h],
        pixels,
    }
}

/// How much of the pixel at `(x, y)` lies inside the rounded rectangle.
fn coverage(x: f32, y: f32, w: f32, h: f32, r: f32) -> f32 {
    let cx = x.clamp(r, w - r);
    let cy = y.clamp(r, h - r);
    let d = ((x - cx).powi(2) + (y - cy).powi(2)).sqrt();
    (r - d + 0.5).clamp(0.0, 1.0)
}

fn byte(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

pub fn color(c: Rgba) -> Color32 {
    Color32::from_rgba_unmultiplied(byte(c.r), byte(c.g), byte(c.b), byte(c.a))
}
