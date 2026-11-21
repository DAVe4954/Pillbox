//! Colours and per-surface looks, background fades and the readable-text rules for them.

use crate::settings::{AppTheme, Appearance, FadeSettings, Surface};

/// Straight (non-premultiplied) colour, components 0..1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgba {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Rgba {
    pub const fn new(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }

    pub const fn rgb(hex: u32) -> Self {
        Self {
            r: ((hex >> 16) & 0xFF) as f32 / 255.0,
            g: ((hex >> 8) & 0xFF) as f32 / 255.0,
            b: (hex & 0xFF) as f32 / 255.0,
            a: 1.0,
        }
    }

    pub fn with_alpha(self, a: f32) -> Self {
        Self { a, ..self }
    }

    /// Multiply the existing alpha.
    pub fn fade(self, k: f32) -> Self {
        Self {
            a: self.a * k.clamp(0.0, 1.0),
            ..self
        }
    }

    /// How light the colour looks, 0 (black) to 1 (white).
    pub fn luminance(self) -> f32 {
        0.2126 * self.r + 0.7152 * self.g + 0.0722 * self.b
    }

    /// WCAG relative luminance: the same weights on linear light, 0 (black) to 1 (white).
    pub fn rel_luminance(self) -> f32 {
        let lin = |c: f32| {
            let c = c.clamp(0.0, 1.0);
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * lin(self.r) + 0.7152 * lin(self.g) + 0.0722 * lin(self.b)
    }

    /// The colour for text and icons drawn on this fill: near black on a light fill (a white
    /// accent), white on a dark or vivid one.
    pub fn readable_on(self) -> Rgba {
        if self.luminance() > 0.62 {
            INK_DARK
        } else {
            INK_LIGHT
        }
    }

    /// Scale the brightness (below 1 darkens).
    pub fn shade(self, k: f32) -> Self {
        Self {
            r: (self.r * k).clamp(0.0, 1.0),
            g: (self.g * k).clamp(0.0, 1.0),
            b: (self.b * k).clamp(0.0, 1.0),
            a: self.a,
        }
    }

    /// Straight mix towards `other`, alpha included: 0 = self, 1 = other. Mixing the stored
    /// (gamma-encoded) values is what Direct2D's `D2D1_GAMMA_2_2` gradients do too.
    pub fn lerp(self, other: Rgba, t: f32) -> Rgba {
        let t = t.clamp(0.0, 1.0);
        let m = |a: f32, b: f32| a + (b - a) * t;
        Rgba::new(
            m(self.r, other.r),
            m(self.g, other.g),
            m(self.b, other.b),
            m(self.a, other.a),
        )
    }

    /// This colour (with its alpha) drawn over an opaque `under`; the result is opaque.
    pub fn over(self, under: Rgba) -> Rgba {
        let a = self.a.clamp(0.0, 1.0);
        let m = |top: f32, bottom: f32| top * a + bottom * (1.0 - a);
        Rgba::new(
            m(self.r, under.r),
            m(self.g, under.g),
            m(self.b, under.b),
            1.0,
        )
    }
}

/// WCAG contrast ratio of two opaque colours, 1 (none) to 21 (black on white).
pub fn contrast(a: Rgba, b: Rgba) -> f32 {
    let (la, lb) = (a.rel_luminance(), b.rel_luminance());
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// The two text colours Pillbox picks between on a fade.
pub const INK_LIGHT: Rgba = Rgba::rgb(0xFFFFFF);
pub const INK_DARK: Rgba = Rgba::rgb(0x14141A);
/// Body text needs this contrast with what is behind it (WCAG AA for normal text)...
pub const TEXT_CONTRAST: f32 = 4.5;
/// ...secondary (dim) text, bars and other shapes this much (WCAG AA for large text and UI).
pub const UI_CONTRAST: f32 = 3.0;
/// Secondary text is the text colour at this alpha.
pub const DIM_ALPHA: f32 = 0.62;
/// What a glass surface is assumed to sit on when its text colour is chosen: the blurred
/// screen behind it is unknown, so a middle grey stands in for it.
pub const GLASS_BACKDROP: Rgba = Rgba::rgb(0x5C5C66);
/// How strongly a fade tints a glass surface at full opacity.
pub const GLASS_FADE_ALPHA: f32 = 0.62;

pub const DEFAULT_ACCENT: u32 = 0x7C5CFF;

/// Parse `#RRGGBB` (the `#` is optional).
pub fn parse_hex(text: &str) -> Option<Rgba> {
    let hex = text.trim().trim_start_matches('#');
    if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    u32::from_str_radix(hex, 16).ok().map(Rgba::rgb)
}

pub fn to_hex(c: Rgba) -> String {
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{:02X}{:02X}{:02X}", byte(c.r), byte(c.g), byte(c.b))
}

fn smoothstep(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

// ---- fades ------------------------------------------------------------------------------------

/// One colour of a fade and where along it that colour is at full strength (0..1).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stop {
    pub at: f32,
    pub color: Rgba,
}

/// A background fade: two or three colours along a straight line across the body.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fade {
    stops: [Stop; 3],
    count: usize,
    /// Direction in degrees: 0 = left to right, 90 = top to bottom (y grows downwards).
    pub angle: f32,
    /// 0 = hard edges between the colours, 1 = each melts all the way into the next.
    pub softness: f32,
}

impl Fade {
    /// A fade through the first three of `stops` (sorted by position). `None` for fewer than
    /// two. Positions are clamped to 0..1, the angle wrapped to 0..360.
    pub fn new(stops: &[Stop], angle: f32, softness: f32) -> Option<Fade> {
        let first = *stops.first()?;
        if stops.len() < 2 {
            return None;
        }
        let count = stops.len().min(3);
        let mut all = [first; 3];
        all[..count].copy_from_slice(&stops[..count]);
        for s in &mut all[..count] {
            s.at = if s.at.is_finite() {
                s.at.clamp(0.0, 1.0)
            } else {
                0.0
            };
        }
        all[..count].sort_by(|a, b| a.at.total_cmp(&b.at));
        Some(Fade {
            stops: all,
            count,
            angle: if angle.is_finite() {
                angle.rem_euclid(360.0)
            } else {
                0.0
            },
            softness: if softness.is_finite() {
                softness.clamp(0.0, 1.0)
            } else {
                1.0
            },
        })
    }

    /// The user's fade from the settings; colours that do not parse fall back to the accent.
    pub fn from_settings(s: &FadeSettings, fallback: Rgba) -> Option<Fade> {
        let stops: Vec<Stop> = s
            .stops
            .iter()
            .map(|st| Stop {
                at: st.at,
                color: parse_hex(&st.color).unwrap_or(fallback),
            })
            .collect();
        Fade::new(&stops, s.angle, s.softness)
    }

    pub fn stops(&self) -> &[Stop] {
        &self.stops[..self.count]
    }

    pub fn first(&self) -> Rgba {
        self.stops[0].color
    }

    pub fn last(&self) -> Rgba {
        self.stops[self.count - 1].color
    }

    /// The same fade with every colour changed by `f`.
    pub fn map_colors(mut self, f: impl Fn(Rgba) -> Rgba) -> Fade {
        for s in &mut self.stops[..self.count] {
            s.color = f(s.color);
        }
        self
    }

    /// The stops a linear gradient brush needs, as (position 0..1, colour), with the softness
    /// applied: between two colours the change happens in a band around their midpoint, as
    /// wide as `softness` allows, and eases in and out (extra stops on a smoothstep curve).
    pub fn brush_stops(&self) -> Vec<(f32, Rgba)> {
        const EASE: [(f32, f32); 5] = [
            (0.0, 0.0),
            (0.25, 0.156_25),
            (0.5, 0.5),
            (0.75, 0.843_75),
            (1.0, 1.0),
        ];
        let s = self.stops();
        let mut out = Vec::with_capacity(2 + s.len() * 6);
        out.push((s[0].at, s[0].color));
        for pair in s.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            let mid = (a.at + b.at) / 2.0;
            let half = self.softness * (b.at - a.at) / 2.0;
            if half < 1e-4 {
                out.push((mid, a.color));
                out.push((mid, b.color));
            } else {
                for (f, k) in EASE {
                    out.push((mid - half + 2.0 * half * f, a.color.lerp(b.color, k)));
                }
            }
            out.push((b.at, b.color));
        }
        out
    }

    /// The colour at position `t` (0..1) along the fade, exactly as the brush draws it.
    pub fn sample(&self, t: f32) -> Rgba {
        sample_stops(&self.brush_stops(), t)
    }

    /// Where the fade runs across the rectangle `(left, top, right, bottom)`: its start and
    /// end points. Like a CSS linear gradient, the line goes through the centre at `angle` and
    /// is just long enough that the corners get the first and the last colour.
    pub fn line(&self, (l, t, r, b): (f32, f32, f32, f32)) -> ((f32, f32), (f32, f32)) {
        let (cx, cy) = ((l + r) / 2.0, (t + b) / 2.0);
        let (dx, dy) = (self.angle.to_radians().cos(), self.angle.to_radians().sin());
        let half = ((r - l) / 2.0 * dx).abs() + ((b - t) / 2.0 * dy).abs();
        (
            (cx - dx * half, cy - dy * half),
            (cx + dx * half, cy + dy * half),
        )
    }

    /// Position along the fade (0..1) of the point `(x, y)` in `rect`.
    pub fn t_at(&self, rect: (f32, f32, f32, f32), x: f32, y: f32) -> f32 {
        let ((x0, y0), (x1, y1)) = self.line(rect);
        let (vx, vy) = (x1 - x0, y1 - y0);
        let len2 = vx * vx + vy * vy;
        if len2 < 1e-6 {
            return 0.0;
        }
        (((x - x0) * vx + (y - y0) * vy) / len2).clamp(0.0, 1.0)
    }
}

/// Piecewise-linear lookup in sorted (position, colour) stops, clamped at both ends.
fn sample_stops(stops: &[(f32, Rgba)], t: f32) -> Rgba {
    let Some(&(first_at, first)) = stops.first() else {
        return Rgba::new(0.0, 0.0, 0.0, 0.0);
    };
    if t <= first_at {
        return first;
    }
    for pair in stops.windows(2) {
        let ((a_at, a), (b_at, b)) = (pair[0], pair[1]);
        if t <= b_at {
            let span = b_at - a_at;
            return if span < 1e-6 {
                b
            } else {
                a.lerp(b, (t - a_at) / span)
            };
        }
    }
    stops.last().map_or(first, |s| s.1)
}

// ---- motion -----------------------------------------------------------------------------------

/// How an animated theme moves. Both are slow and cheap: a drift redraws `DRIFT_FPS` times a
/// second, a shimmer redraws only while its light crosses the body.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Motion {
    /// The colours flow slowly along the fade, one round every `period` seconds, and the
    /// direction sways a little.
    Drift { period: f32 },
    /// A soft light sweeps across the body once every `period` seconds.
    Shimmer { period: f32 },
}

pub const DRIFT_FPS: f32 = 5.0;
pub const SHIMMER_FPS: f32 = 30.0;
/// How long one shimmer sweep takes, seconds.
pub const SHIMMER_SWEEP: f32 = 2.4;
/// Half the width of the shimmer's light, as a fraction of the body's width.
pub const SHIMMER_HALF_WIDTH: f32 = 0.22;
/// How far a drift sways the direction either way, degrees.
const DRIFT_SWAY: f32 = 12.0;
/// The shimmer's light leans like a reflection: it runs along the line from
/// `(left + lean, top)` to `(right + lean, bottom)`, `lean` being this times the height.
pub const SHIMMER_LEAN: f32 = 0.6;

/// The shimmer's light around its centre `x` (a position along its line): (position, alpha of
/// white). Positions past 0..1 are clamped where it is drawn.
pub fn shimmer_band(x: f32) -> [(f32, f32); 5] {
    let hw = SHIMMER_HALF_WIDTH;
    [
        (x - hw, 0.0),
        (x - hw * 0.35, 0.09),
        (x, 0.17),
        (x + hw * 0.35, 0.09),
        (x + hw, 0.0),
    ]
}

impl Motion {
    fn period(self) -> f32 {
        match self {
            Motion::Drift { period } | Motion::Shimmer { period } => period.max(1.0),
        }
    }

    /// The fade as it looks `clock` seconds in. A shimmer leaves the colours alone.
    pub fn apply(self, fade: Fade, clock: f32) -> Fade {
        let Motion::Drift { .. } = self else {
            return fade;
        };
        let phase = (clock / self.period()).rem_euclid(1.0);
        let n = fade.count;
        let base = fade.stops;
        let mut out = fade;
        for (i, stop) in out.stops[..n].iter_mut().enumerate() {
            // Each stop walks round the ring of colours, a third (or a half) apart.
            let u = (i as f32 / n as f32 + phase) * n as f32;
            let from = (u.floor() as usize) % n;
            let to = (from + 1) % n;
            stop.color = base[from].color.lerp(base[to].color, smoothstep(u.fract()));
        }
        out.angle =
            (fade.angle + DRIFT_SWAY * (phase * std::f32::consts::TAU).sin()).rem_euclid(360.0);
        out
    }

    /// Where the shimmer's light is `clock` seconds in, as a fraction of the body's width
    /// (it starts and ends fully outside), or `None` between sweeps.
    pub fn shimmer(self, clock: f32) -> Option<f32> {
        let Motion::Shimmer { .. } = self else {
            return None;
        };
        let local = clock.rem_euclid(self.period());
        (local < SHIMMER_SWEEP).then(|| {
            let from = -SHIMMER_HALF_WIDTH;
            let to = 1.0 + SHIMMER_HALF_WIDTH;
            from + (to - from) * smoothstep(local / SHIMMER_SWEEP)
        })
    }

    /// Seconds until the next frame worth drawing, `clock` seconds in.
    pub fn next_frame_in(self, clock: f32) -> f32 {
        match self {
            Motion::Drift { .. } => 1.0 / DRIFT_FPS,
            Motion::Shimmer { .. } => {
                let period = self.period();
                let local = clock.rem_euclid(period);
                if local < SHIMMER_SWEEP {
                    // One frame lands just after the sweep, so the light is drawn away.
                    (1.0 / SHIMMER_FPS).min(SHIMMER_SWEEP - local + 0.001)
                } else {
                    (period - local).max(1.0 / SHIMMER_FPS)
                }
            }
        }
    }
}

// ---- readable text ----------------------------------------------------------------------------

/// The backgrounds text meets along a fade drawn at `alpha` over `under`: evenly spaced points
/// plus every brush stop.
fn backgrounds(fade: &Fade, alpha: f32, under: Rgba) -> Vec<Rgba> {
    const STEPS: usize = 48;
    let stops = fade.brush_stops();
    let mut ts: Vec<f32> = (0..=STEPS).map(|i| i as f32 / STEPS as f32).collect();
    ts.extend(stops.iter().map(|(at, _)| *at));
    ts.iter()
        .map(|t| sample_stops(&stops, *t).with_alpha(alpha).over(under))
        .collect()
}

/// The lowest contrast `ink` gets anywhere on these backgrounds, for full and for dim text.
fn worst(ink: Rgba, bgs: &[Rgba]) -> (f32, f32) {
    bgs.iter().fold((f32::MAX, f32::MAX), |(text, dim), bg| {
        let dim_ink = ink.with_alpha(DIM_ALPHA).over(*bg);
        (
            text.min(contrast(ink, *bg)),
            dim.min(contrast(dim_ink, *bg)),
        )
    })
}

/// Does `ink` read everywhere on the fade: text at `TEXT_CONTRAST`, dim text at `UI_CONTRAST`?
pub fn fade_reads(fade: &Fade, alpha: f32, under: Rgba, ink: Rgba) -> bool {
    let (text, dim) = worst(ink, &backgrounds(fade, alpha, under));
    text >= TEXT_CONTRAST && dim >= UI_CONTRAST
}

/// The fade nudged until `ink` reads on all of it: each colour that is too close to the ink
/// is pushed away from it (darker under light text, lighter under dark text), just as far as
/// needed, then the whole fade if the blends between colours still fall short.
fn push_away(fade: Fade, alpha: f32, under: Rgba, ink: Rgba) -> Fade {
    if fade_reads(&fade, alpha, under, ink) {
        return fade;
    }
    let away = if ink.rel_luminance() > 0.5 {
        Rgba::rgb(0x000000)
    } else {
        Rgba::rgb(0xFFFFFF)
    };
    let mut fixed = fade;
    for i in 0..fixed.count {
        let original = fixed.stops[i].color;
        for step in 0..=20 {
            let c = original.lerp(away, step as f32 * 0.05);
            fixed.stops[i].color = c;
            let bg = c.with_alpha(alpha).over(under);
            let dim = ink.with_alpha(DIM_ALPHA).over(bg);
            // A little margin over the targets: the blends between stops come close to them.
            if contrast(ink, bg) >= TEXT_CONTRAST + 0.15 && contrast(dim, bg) >= UI_CONTRAST + 0.1 {
                break;
            }
        }
    }
    for _ in 0..20 {
        if fade_reads(&fixed, alpha, under, ink) {
            break;
        }
        fixed = fixed.map_colors(|c| c.lerp(away, 0.05));
    }
    fixed
}

/// The text colour for a fade drawn at `alpha` over `under`, and the fade adjusted (if it has
/// to be) so that text reads on every part of it. With `ink` given that colour is kept and only
/// the fade changes; otherwise light or dark text is picked, whichever needs the fade changed
/// least. Text never ends up unreadable: the side the surface itself reads with always works.
pub fn readable_fade(fade: Fade, alpha: f32, under: Rgba, ink: Option<Rgba>) -> (Fade, Rgba) {
    let order = match ink {
        Some(i) => [i, i],
        None => {
            let bgs = backgrounds(&fade, alpha, under);
            let score = |ink| worst(ink, &bgs).0;
            if score(INK_LIGHT) >= score(INK_DARK) {
                [INK_LIGHT, INK_DARK]
            } else {
                [INK_DARK, INK_LIGHT]
            }
        }
    };
    let first = push_away(fade, alpha, under, order[0]);
    if fade_reads(&first, alpha, under, order[0]) || ink.is_some() {
        return (first, order[0]);
    }
    let second = push_away(fade, alpha, under, order[1]);
    if fade_reads(&second, alpha, under, order[1]) {
        (second, order[1])
    } else {
        (first, order[0])
    }
}

/// `color` moved towards `toward` until it stands out from every part of the fade by
/// `UI_CONTRAST` (an accent bar on a fade close to the accent's own colour).
pub fn stand_out(color: Rgba, fade: &Fade, alpha: f32, under: Rgba, toward: Rgba) -> Rgba {
    let bgs = backgrounds(fade, alpha, under);
    let reads = |c: Rgba| {
        bgs.iter()
            .all(|bg| contrast(c.with_alpha(1.0), *bg) >= UI_CONTRAST)
    };
    for step in 0..=20 {
        let c = color.lerp(toward, step as f32 * 0.05);
        if reads(c) {
            return c;
        }
    }
    toward
}

// ---- themes -----------------------------------------------------------------------------------

/// The small decoration each soft theme carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Touch {
    Hearts,
    Sparkles,
    Petals,
}

/// How a glass surface treats the screen behind it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Glass {
    /// Multiplier on the user's blur setting.
    pub blur: f32,
    /// Refract the backdrop near the rim.
    pub bend: bool,
    /// Tint drawn over the backdrop, top to bottom.
    pub tint_top: Rgba,
    pub tint_bottom: Rgba,
    /// Strength of the glossy highlight across the top, 0 = none.
    pub gloss: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Theme {
    /// Body fill, top to bottom. For glass this is the fallback when capture is unavailable.
    /// Under a fade this is what shows through where the fade is not fully opaque.
    pub fill_top: Rgba,
    pub fill_bottom: Rgba,
    pub rim: Option<Rgba>,
    pub text: Rgba,
    pub text_dim: Rgba,
    /// Colour for progress bars and decorations.
    pub accent: Rgba,
    /// The highlight behind the active tab icon.
    pub chip: Rgba,
    /// The colour the tiles, rows and bars of the list tabs are washed with (at a low alpha).
    pub wash: Rgba,
    pub glass: Option<Glass>,
    pub touch: Option<Touch>,
    /// A fade of several colours drawn over the fill (or, on glass, over the blurred screen
    /// in place of the tint).
    pub fade: Option<Fade>,
    /// How strongly the fade covers what is under it, 0..1.
    pub fade_alpha: f32,
    /// The fade moves (only when animation is allowed).
    pub motion: Option<Motion>,
}

impl Theme {
    pub fn for_surface(surface: Surface, accent: Rgba) -> Theme {
        let white = INK_LIGHT;
        let dark = |top: u32, bottom: u32, rim: f32| Theme {
            fill_top: Rgba::rgb(top),
            fill_bottom: Rgba::rgb(bottom),
            rim: (rim > 0.0).then(|| white.with_alpha(rim)),
            text: white,
            text_dim: white.with_alpha(DIM_ALPHA),
            accent,
            chip: white,
            wash: white,
            glass: None,
            touch: None,
            fade: None,
            fade_alpha: 1.0,
            motion: None,
        };
        let soft = |top: u32, bottom: u32, text: u32, own_accent: u32, touch: Touch| Theme {
            fill_top: Rgba::rgb(top),
            fill_bottom: Rgba::rgb(bottom),
            rim: Some(white.with_alpha(0.7)),
            text: Rgba::rgb(text),
            text_dim: Rgba::rgb(text).with_alpha(DIM_ALPHA),
            accent: Rgba::rgb(own_accent),
            chip: Rgba::rgb(text),
            wash: Rgba::rgb(text),
            glass: None,
            touch: Some(touch),
            fade: None,
            fade_alpha: 1.0,
            motion: None,
        };
        match surface {
            Surface::Black => dark(0x0B0B0C, 0x0B0B0C, 0.08),
            Surface::PureBlack => dark(0x000000, 0x000000, 0.0),
            Surface::Graphite => dark(0x36373C, 0x1D1E22, 0.12),
            Surface::Acrylic => Theme {
                glass: Some(Glass {
                    blur: 1.7,
                    bend: false,
                    tint_top: Rgba::new(0.11, 0.11, 0.13, 0.46),
                    tint_bottom: Rgba::new(0.09, 0.09, 0.11, 0.52),
                    gloss: 0.0,
                }),
                ..dark(0x1C1C20, 0x18181B, 0.14)
            },
            Surface::LiquidGlass => Theme {
                glass: Some(Glass {
                    blur: 0.55,
                    bend: true,
                    tint_top: Rgba::new(1.0, 1.0, 1.0, 0.07),
                    tint_bottom: Rgba::new(0.04, 0.04, 0.07, 0.22),
                    gloss: 0.22,
                }),
                ..dark(0x2A2B30, 0x1B1C20, 0.38)
            },
            Surface::CandyGlass => Theme {
                glass: Some(Glass {
                    blur: 0.9,
                    bend: true,
                    tint_top: accent.with_alpha(0.38),
                    tint_bottom: accent.shade(0.6).with_alpha(0.58),
                    gloss: 0.42,
                }),
                fill_top: accent.shade(0.8),
                fill_bottom: accent.shade(0.45),
                ..dark(0, 0, 0.5)
            },
            Surface::Rose => soft(0xFFDDE6, 0xF8B9CA, 0x5A2A3A, 0xE8588A, Touch::Hearts),
            Surface::Lavender => soft(0xE9E0FF, 0xCBBBF6, 0x3A2E5C, 0x8F6BE8, Touch::Sparkles),
            Surface::Peach => soft(0xFFE6D3, 0xFFC7A4, 0x5C3A26, 0xF0834A, Touch::Sparkles),
            Surface::Sakura => soft(0xFFEEF3, 0xFFCFDF, 0x5A3040, 0xF078A0, Touch::Petals),
        }
    }

    /// The whole look from the Appearance settings: the surface, recoloured by the app theme
    /// (a flat one, a fade, or the user's own fade), with motion only when it is allowed.
    pub fn from_appearance(a: &Appearance) -> Theme {
        let accent = parse_hex(&a.accent).unwrap_or(Rgba::rgb(DEFAULT_ACCENT));
        let base = Theme::for_surface(a.surface, accent);
        let theme = match a.app_theme {
            AppTheme::Custom => base.with_custom_fade(&a.fade, accent),
            app => base.with_app_theme(app),
        };
        if a.animate { theme } else { theme.still() }
    }

    /// This theme without motion.
    pub fn still(self) -> Theme {
        Theme {
            motion: None,
            ..self
        }
    }

    /// This theme recoloured by an app theme: the card's fill, rim, text, accent, the active
    /// tab's chip and the wash of the list tabs. A glass surface keeps its material (blur,
    /// bend, gloss) and takes the app theme's colour as its tint; a solid one takes the fill.
    /// `AppTheme::Default` changes nothing; `AppTheme::Custom` needs the user's fade and goes
    /// through `with_custom_fade` (here it changes nothing).
    pub fn with_app_theme(self, app: AppTheme) -> Theme {
        if let Some(p) = fade_palette(app) {
            return self.with_fade_palette(&p);
        }
        let Some(p) = app_palette(app) else {
            return self;
        };
        let (top, bottom) = (Rgba::rgb(p.top), Rgba::rgb(p.bottom));
        let (text, accent) = (Rgba::rgb(p.text), Rgba::rgb(p.accent));
        Theme {
            fill_top: top,
            fill_bottom: bottom,
            rim: Some(accent.with_alpha(0.22)),
            text,
            text_dim: text.with_alpha(DIM_ALPHA),
            accent,
            chip: accent,
            wash: Rgba::rgb(p.wash),
            glass: self.glass.map(|g| Glass {
                tint_top: top.with_alpha(0.42),
                tint_bottom: bottom.with_alpha(0.58),
                ..g
            }),
            touch: None,
            fade: None,
            fade_alpha: 1.0,
            motion: None,
        }
    }

    /// What a fade is drawn over when the text colour is chosen: the surface's own fill, or
    /// for glass a stand-in for the blurred screen.
    fn under(&self) -> Rgba {
        match self.glass {
            Some(_) => GLASS_BACKDROP,
            None => self.fill_top.lerp(self.fill_bottom, 0.5).with_alpha(1.0),
        }
    }

    /// How strongly a fade at `opacity` covers this surface.
    fn fade_alpha_for(&self, opacity: f32) -> f32 {
        let opacity = opacity.clamp(0.0, 1.0);
        if self.glass.is_some() {
            GLASS_FADE_ALPHA * opacity
        } else {
            opacity
        }
    }

    /// A built-in fade theme over this surface. Its text colour is kept; on glass (where the
    /// screen shows through) the fade is darkened or lightened if the text needs it.
    fn with_fade_palette(self, p: &FadePalette) -> Theme {
        let stops: Vec<Stop> = p
            .stops
            .iter()
            .map(|(at, hex)| Stop {
                at: *at,
                color: Rgba::rgb(*hex),
            })
            .collect();
        let Some(fade) = Fade::new(&stops, p.angle, 1.0) else {
            return self;
        };
        let text = Rgba::rgb(p.text);
        let alpha = self.fade_alpha_for(1.0);
        let (fade, text) = readable_fade(fade, alpha, self.under(), Some(text));
        let accent = Rgba::rgb(p.accent);
        self.with_fade(
            fade,
            alpha,
            FadeLook {
                text,
                accent,
                chip: accent,
                wash: Rgba::rgb(p.wash),
                rim: Rgba::rgb(p.rim.0).with_alpha(p.rim.1),
                motion: p.motion,
            },
        )
    }

    /// The user's own fade over this surface: the text colour is chosen for it (and the fade
    /// adjusted if neither light nor dark text would read), and the accent is lifted or
    /// deepened until bars and highlights stand out from it.
    pub fn with_custom_fade(self, s: &FadeSettings, accent: Rgba) -> Theme {
        let Some(fade) = Fade::from_settings(s, accent) else {
            return self;
        };
        let alpha = self.fade_alpha_for(s.opacity);
        let under = self.under();
        let (fade, ink) = readable_fade(fade, alpha, under, None);
        let accent = stand_out(accent, &fade, alpha, under, ink);
        self.with_fade(
            fade,
            alpha,
            FadeLook {
                text: ink,
                accent,
                chip: accent,
                wash: ink,
                rim: ink.with_alpha(0.14),
                motion: None,
            },
        )
    }

    fn with_fade(self, fade: Fade, alpha: f32, look: FadeLook) -> Theme {
        Theme {
            // Where the fade is not fully opaque the surface shows through; glass keeps its
            // tint colours only as the fallback before the screen capture starts.
            fill_top: if alpha >= 0.999 || self.glass.is_some() {
                fade.first()
            } else {
                self.fill_top
            },
            fill_bottom: if alpha >= 0.999 || self.glass.is_some() {
                fade.last()
            } else {
                self.fill_bottom
            },
            rim: Some(look.rim),
            text: look.text,
            text_dim: look.text.with_alpha(DIM_ALPHA),
            accent: look.accent,
            chip: look.chip,
            wash: look.wash,
            glass: self.glass.map(|g| Glass {
                tint_top: fade.first().with_alpha(alpha),
                tint_bottom: fade.last().with_alpha(alpha),
                ..g
            }),
            touch: None,
            fade: Some(fade),
            fade_alpha: alpha,
            motion: look.motion,
        }
    }

    /// The fade as it looks `clock` seconds in (moved, when the theme moves).
    pub fn fade_at(&self, clock: f32) -> Option<Fade> {
        let fade = self.fade?;
        Some(match self.motion {
            Some(m) => m.apply(fade, clock),
            None => fade,
        })
    }

    /// The body colour at `(x, y)` of a `w`×`h` body, as Pillbox paints it before the rim:
    /// the fill (or, on glass, `backdrop` under the tint), the fade over it, and the soft
    /// sheen across the top. The Settings previews are drawn from this.
    pub fn body_at(&self, w: f32, h: f32, x: f32, y: f32, backdrop: Rgba, clock: f32) -> Rgba {
        self.body_painter(w, h, clock).at(x, y, backdrop)
    }

    /// `body_at` for many points of one `w`×`h` body at one moment (a preview picture): the
    /// fade is worked out once.
    pub fn body_painter(&self, w: f32, h: f32, clock: f32) -> BodyPainter<'_> {
        BodyPainter {
            theme: self,
            w,
            h,
            clock,
            fade: self.fade_at(clock).map(|f| {
                let stops = f.brush_stops();
                (f, stops)
            }),
        }
    }

    /// How much white light a shimmer puts on `(x, y)` of a `w`×`h` body, `clock` seconds in
    /// (0 between sweeps and for every other theme).
    pub fn shimmer_light(&self, w: f32, h: f32, x: f32, y: f32, clock: f32) -> f32 {
        let Some(centre) = self.motion.and_then(|m| m.shimmer(clock)) else {
            return 0.0;
        };
        let lean = h * SHIMMER_LEAN;
        // Position along the line from (lean, 0) to (w + lean, h), clamped like the brush.
        let len2 = w * w + h * h;
        if len2 < 1e-6 {
            return 0.0;
        }
        let t = (((x - lean) * w + y * h) / len2).clamp(0.0, 1.0);
        let band = shimmer_band(centre).map(|(at, a)| (at.clamp(0.0, 1.0), a));
        let Some(&(first_at, first)) = band.first() else {
            return 0.0;
        };
        if t <= first_at {
            return first;
        }
        for pair in band.windows(2) {
            let ((a_at, a), (b_at, b)) = (pair[0], pair[1]);
            if t <= b_at {
                let span = b_at - a_at;
                return if span < 1e-6 {
                    b
                } else {
                    a + (b - a) * (t - a_at) / span
                };
            }
        }
        band[4].1
    }

    /// Strength of the soft light across the top of the body (glass has its own gloss).
    pub fn sheen(&self) -> f32 {
        match self.glass {
            Some(_) => 0.0,
            // Light from above reads on dark and coloured bodies; on light ones it would only
            // wash them out.
            None if self.text.rel_luminance() < 0.5 => 0.0,
            None => 0.045,
        }
    }
}

/// How far down the sheen reaches, as a fraction of the body's height.
pub const SHEEN_DEPTH: f32 = 0.5;

/// A theme's body at one moment, ready to be looked up point by point (`Theme::body_painter`).
pub struct BodyPainter<'a> {
    theme: &'a Theme,
    w: f32,
    h: f32,
    clock: f32,
    fade: Option<(Fade, Vec<(f32, Rgba)>)>,
}

impl BodyPainter<'_> {
    /// The body colour at `(x, y)`, over `backdrop` where glass lets the screen through.
    pub fn at(&self, x: f32, y: f32, backdrop: Rgba) -> Rgba {
        let t = self.theme;
        let v = if self.h > 0.0 {
            (y / self.h).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let mut c = match t.glass {
            Some(g) if self.fade.is_none() => g.tint_top.lerp(g.tint_bottom, v).over(backdrop),
            Some(_) => backdrop.with_alpha(1.0),
            None => t.fill_top.lerp(t.fill_bottom, v).with_alpha(1.0),
        };
        if let Some((f, stops)) = &self.fade {
            let at = f.t_at((0.0, 0.0, self.w, self.h), x, y);
            c = sample_stops(stops, at).with_alpha(t.fade_alpha).over(c);
        }
        let light = t.shimmer_light(self.w, self.h, x, y, self.clock);
        if light > 0.0 {
            c = INK_LIGHT.with_alpha(light).over(c);
        }
        let sheen = t.sheen();
        if sheen > 0.0 && v < SHEEN_DEPTH {
            c = INK_LIGHT
                .with_alpha(sheen * (1.0 - v / SHEEN_DEPTH))
                .over(c);
        }
        c
    }
}

struct FadeLook {
    text: Rgba,
    accent: Rgba,
    chip: Rgba,
    wash: Rgba,
    rim: Rgba,
    motion: Option<Motion>,
}

/// The colours of one app theme, as `0xRRGGBB`.
struct AppPalette {
    top: u32,
    bottom: u32,
    text: u32,
    accent: u32,
    wash: u32,
}

fn app_palette(app: AppTheme) -> Option<AppPalette> {
    let p = |top, bottom, text, accent, wash| AppPalette {
        top,
        bottom,
        text,
        accent,
        wash,
    };
    Some(match app {
        AppTheme::Midnight => p(0x0E1220, 0x090B14, 0xEAF0FF, 0x6C8CFF, 0xC9D6FF),
        AppTheme::Ocean => p(0x0A1C24, 0x061218, 0xE6FAFA, 0x2DD4D0, 0xBFF0EE),
        AppTheme::Forest => p(0x0F1C14, 0x08110B, 0xEAF8EE, 0x4ADE80, 0xC6EBD0),
        AppTheme::Sunset => p(0x2A1220, 0x1A0A12, 0xFFEFE6, 0xFF8A4C, 0xFFD3BA),
        AppTheme::Violet => p(0x1A1130, 0x100A1F, 0xF1ECFF, 0xA78BFA, 0xD9CCFF),
        AppTheme::Ember => p(0x2A0E0E, 0x180808, 0xFFECEA, 0xFF5A4F, 0xFFC9C4),
        AppTheme::Mono => p(0x2A2A2A, 0x141414, 0xFFFFFF, 0xFFFFFF, 0xFFFFFF),
        _ => return None,
    })
}

/// The colours of one fade theme: three stops (position, `0xRRGGBB`), the direction, and the
/// text, accent, wash and rim (colour, alpha) that go with them.
struct FadePalette {
    stops: [(f32, u32); 3],
    angle: f32,
    text: u32,
    accent: u32,
    wash: u32,
    rim: (u32, f32),
    motion: Option<Motion>,
}

fn fade_palette(app: AppTheme) -> Option<FadePalette> {
    let p = |stops, angle, text, accent, wash, rim, motion| FadePalette {
        stops,
        angle,
        text,
        accent,
        wash,
        rim,
        motion,
    };
    Some(match app {
        AppTheme::Aurora => p(
            [(0.0, 0x0A2A3C), (0.5, 0x0C5A4C), (1.0, 0x3A1E6A)],
            15.0,
            0xEFFFF8,
            0x5EF2B0,
            0xCFFFEA,
            (0x7DF5C9, 0.30),
            Some(Motion::Drift { period: 36.0 }),
        ),
        AppTheme::GoldenHour => p(
            [(0.0, 0x2B1240), (0.55, 0x7E2A4C), (1.0, 0xA8432F)],
            20.0,
            0xFFF3EA,
            0xFFB067,
            0xFFE2CC,
            (0xFFC08A, 0.28),
            None,
        ),
        AppTheme::DeepSea => p(
            [(0.0, 0x04203A), (0.5, 0x08456A), (1.0, 0x0B6A78)],
            160.0,
            0xEAFBFF,
            0x5BE3E0,
            0xC8F4FF,
            (0x7FE9F0, 0.26),
            None,
        ),
        AppTheme::MidnightNeon => p(
            [(0.0, 0x140A33), (0.5, 0x3A0E5E), (1.0, 0x0C2A5E)],
            0.0,
            0xF6EEFF,
            0xFF5CDB,
            0xE6D6FF,
            (0x3DE6FF, 0.40),
            Some(Motion::Shimmer { period: 12.0 }),
        ),
        AppTheme::ForestMist => p(
            [(0.0, 0xE4EDE6), (0.55, 0xC4D8CB), (1.0, 0xA9C4B4)],
            100.0,
            0x15241B,
            0x23603F,
            0x1E3A2A,
            (0xFFFFFF, 0.55),
            None,
        ),
        AppTheme::Lava => p(
            [(0.0, 0x1C0606), (0.5, 0x5C120C), (1.0, 0x9A2C0E)],
            25.0,
            0xFFF1EA,
            0xFFB347,
            0xFFD9C2,
            (0xFF8A4A, 0.32),
            Some(Motion::Drift { period: 28.0 }),
        ),
        AppTheme::MonoFade => p(
            [(0.0, 0x3C3C42), (0.5, 0x1E1E22), (1.0, 0x08080A)],
            90.0,
            0xFFFFFF,
            0xE8E8EE,
            0xFFFFFF,
            (0xFFFFFF, 0.16),
            None,
        ),
        AppTheme::CottonCandy => p(
            [(0.0, 0xFFD3EA), (0.5, 0xE6D6FF), (1.0, 0xC9E8FF)],
            0.0,
            0x2A1F3D,
            0xB0367F,
            0x3A2B55,
            (0xFFFFFF, 0.70),
            None,
        ),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_on_an_accent_stays_readable() {
        let on = |hex| Rgba::rgb(hex).readable_on();
        // A white accent (the Mono theme) must not get white text.
        assert_eq!(on(0xFFFFFF), Rgba::rgb(0x14141A));
        assert_eq!(on(0x2DD4D0), Rgba::rgb(0x14141A));
        // Violet and pink keep white text.
        assert_eq!(on(0x7C5CFF), Rgba::rgb(0xFFFFFF));
        assert_eq!(on(0xFF5CA8), Rgba::rgb(0xFFFFFF));
        assert!(Rgba::rgb(0xFFFFFF).luminance() > Rgba::rgb(0x000000).luminance());
    }

    #[test]
    fn hex_parses_with_and_without_hash() {
        assert_eq!(parse_hex("#FF8000"), Some(Rgba::rgb(0xFF8000)));
        assert_eq!(parse_hex("ff8000"), Some(Rgba::rgb(0xFF8000)));
        assert_eq!(parse_hex(" #000000 "), Some(Rgba::rgb(0)));
    }

    #[test]
    fn bad_hex_is_rejected() {
        for bad in [
            "", "#FFF", "#GGGGGG", "#12345", "#1234567", "red", "#+12345",
        ] {
            assert_eq!(parse_hex(bad), None, "{bad}");
        }
    }

    #[test]
    fn hex_round_trips() {
        for hex in ["#7C5CFF", "#000000", "#FFFFFF", "#E8588A"] {
            let c = parse_hex(hex);
            assert_eq!(c.map(to_hex).as_deref(), Some(hex));
        }
    }

    #[test]
    fn every_surface_has_a_theme_with_readable_text() {
        let accent = Rgba::rgb(0x7C5CFF);
        for s in Surface::ALL {
            let t = Theme::for_surface(s, accent);
            assert_eq!(t.glass.is_some(), s.is_glass(), "{s:?}");
            // Text must contrast with the body: light on dark or dark on light.
            let lum = |c: Rgba| 0.2126 * c.r + 0.7152 * c.g + 0.0722 * c.b;
            let contrast = (lum(t.text) - lum(t.fill_bottom)).abs();
            assert!(contrast > 0.35, "{s:?} contrast {contrast}");
        }
    }

    #[test]
    fn app_themes_recolour_the_card_and_keep_the_material() {
        let accent = Rgba::rgb(0x7C5CFF);
        for s in Surface::ALL {
            let base = Theme::for_surface(s, accent);
            // The default app theme is the surface as it is, and so is a custom one without
            // its colours (`with_custom_fade` brings those).
            assert_eq!(base.with_app_theme(AppTheme::Default), base, "{s:?}");
            assert_eq!(base.with_app_theme(AppTheme::Custom), base, "{s:?}");
            for app in AppTheme::ALL
                .into_iter()
                .filter(|a| !matches!(a, AppTheme::Default | AppTheme::Custom))
            {
                let t = base.with_app_theme(app);
                assert_ne!(t.accent, accent, "{app:?} has its own accent");
                assert!(t.touch.is_none(), "{app:?} drops the soft decoration");
                assert_eq!(t.fade.is_some(), app.is_fade(), "{app:?}");
                // Glass stays glass (same blur and bend), solid stays solid.
                assert_eq!(t.glass.is_some(), base.glass.is_some(), "{s:?} {app:?}");
                if let (Some(a), Some(b)) = (t.glass, base.glass) {
                    assert_eq!((a.blur, a.bend, a.gloss), (b.blur, b.bend, b.gloss));
                }
                // Readable: the text and the accent against the body.
                let body = t.fill_top.lerp(t.fill_bottom, 0.5).with_alpha(1.0);
                let body = match t.fade {
                    Some(f) => f.sample(0.5).with_alpha(t.fade_alpha).over(t.under()),
                    None => body,
                };
                assert!(
                    contrast(t.text, body) >= TEXT_CONTRAST,
                    "{s:?} {app:?} text contrast {}",
                    contrast(t.text, body)
                );
            }
        }
    }

    #[test]
    fn soft_themes_each_have_a_touch() {
        let accent = Rgba::rgb(0x7C5CFF);
        let touch = |s| Theme::for_surface(s, accent).touch;
        assert_eq!(touch(Surface::Rose), Some(Touch::Hearts));
        assert_eq!(touch(Surface::Lavender), Some(Touch::Sparkles));
        assert_eq!(touch(Surface::Peach), Some(Touch::Sparkles));
        assert_eq!(touch(Surface::Sakura), Some(Touch::Petals));
        assert_eq!(touch(Surface::Black), None);
    }

    // ---- fades --------------------------------------------------------------------------------

    fn stop(at: f32, hex: u32) -> Stop {
        Stop {
            at,
            color: Rgba::rgb(hex),
        }
    }

    fn fade(stops: &[Stop], angle: f32, softness: f32) -> Fade {
        match Fade::new(stops, angle, softness) {
            Some(f) => f,
            None => panic!("a fade of {} stops", stops.len()),
        }
    }

    fn close(a: Rgba, b: Rgba) -> bool {
        (a.r - b.r).abs() < 1e-3 && (a.g - b.g).abs() < 1e-3 && (a.b - b.b).abs() < 1e-3
    }

    #[test]
    fn a_fade_needs_two_stops_and_keeps_three() {
        assert!(Fade::new(&[], 0.0, 1.0).is_none());
        assert!(Fade::new(&[stop(0.0, 0xFF0000)], 0.0, 1.0).is_none());
        let f = fade(
            &[
                stop(1.0, 0x0000FF),
                stop(0.0, 0xFF0000),
                stop(0.5, 0x00FF00),
                stop(0.7, 0xFFFFFF),
            ],
            -90.0,
            7.0,
        );
        // Only three are kept, sorted by position; the angle wraps and softness clamps.
        let at: Vec<f32> = f.stops().iter().map(|s| s.at).collect();
        assert_eq!(at, vec![0.0, 0.5, 1.0]);
        assert_eq!(f.angle, 270.0);
        assert_eq!(f.softness, 1.0);
        let f = fade(
            &[stop(f32::NAN, 0), stop(3.0, 0xFFFFFF)],
            f32::NAN,
            f32::NAN,
        );
        assert_eq!((f.stops()[0].at, f.stops()[1].at), (0.0, 1.0));
        assert_eq!((f.angle, f.softness), (0.0, 1.0));
    }

    #[test]
    fn soft_fades_blend_and_hard_ones_cut() {
        let soft = fade(&[stop(0.0, 0x000000), stop(1.0, 0xFFFFFF)], 0.0, 1.0);
        assert!(close(soft.sample(0.0), Rgba::rgb(0x000000)));
        assert!(close(soft.sample(1.0), Rgba::rgb(0xFFFFFF)));
        assert!(close(soft.sample(0.5), Rgba::new(0.5, 0.5, 0.5, 1.0)));
        // Eased: a quarter of the way it has changed less than a quarter.
        assert!(soft.sample(0.25).r < 0.25 && soft.sample(0.25).r > 0.05);
        // Positions are monotone and the samples never go backwards.
        let stops = soft.brush_stops();
        assert!(stops.windows(2).all(|w| w[0].0 <= w[1].0));
        let mut last = -1.0;
        for i in 0..=100 {
            let v = soft.sample(i as f32 / 100.0).r;
            assert!(v >= last - 1e-6);
            last = v;
        }
        // Softness 0: one colour up to the middle, the other after it.
        let hard = fade(&[stop(0.0, 0x000000), stop(1.0, 0xFFFFFF)], 0.0, 0.0);
        assert!(close(hard.sample(0.49), Rgba::rgb(0x000000)));
        assert!(close(hard.sample(0.51), Rgba::rgb(0xFFFFFF)));
        // Half soft: the change happens within the middle half.
        let half = fade(&[stop(0.0, 0x000000), stop(1.0, 0xFFFFFF)], 0.0, 0.5);
        assert!(close(half.sample(0.24), Rgba::rgb(0x000000)));
        assert!(close(half.sample(0.76), Rgba::rgb(0xFFFFFF)));
        // Before the first stop and after the last the end colours hold.
        let inset = fade(&[stop(0.3, 0xFF0000), stop(0.6, 0x0000FF)], 0.0, 1.0);
        assert!(close(inset.sample(0.1), Rgba::rgb(0xFF0000)));
        assert!(close(inset.sample(0.9), Rgba::rgb(0x0000FF)));
        // Three stops pass through the middle colour.
        let three = fade(
            &[
                stop(0.0, 0xFF0000),
                stop(0.5, 0x00FF00),
                stop(1.0, 0x0000FF),
            ],
            0.0,
            1.0,
        );
        assert!(close(three.sample(0.5), Rgba::rgb(0x00FF00)));
    }

    #[test]
    fn the_fade_line_spans_the_body_at_its_angle() {
        let rect = (0.0, 0.0, 200.0, 40.0);
        let f = |angle| fade(&[stop(0.0, 0), stop(1.0, 0xFFFFFF)], angle, 1.0);
        // Left to right: from the left edge to the right edge, through the middle.
        let ((x0, y0), (x1, y1)) = f(0.0).line(rect);
        assert!((x0 - 0.0).abs() < 1e-3 && (x1 - 200.0).abs() < 1e-3);
        assert!((y0 - 20.0).abs() < 1e-3 && (y1 - 20.0).abs() < 1e-3);
        // Top to bottom.
        let ((x0, y0), (x1, y1)) = f(90.0).line(rect);
        assert!((x0 - 100.0).abs() < 1e-3 && (x1 - 100.0).abs() < 1e-3);
        assert!((y0 - 0.0).abs() < 1e-3 && (y1 - 40.0).abs() < 1e-3);
        // Diagonal: the corners get the end colours exactly.
        let d = f(45.0);
        assert!(d.t_at(rect, 0.0, 0.0) < 1e-4);
        assert!((d.t_at(rect, 200.0, 40.0) - 1.0).abs() < 1e-4);
        assert!((d.t_at(rect, 100.0, 20.0) - 0.5).abs() < 1e-4);
        // Right to left reverses it.
        assert!((f(180.0).t_at(rect, 0.0, 20.0) - 1.0).abs() < 1e-4);
        // Everything inside maps into 0..1.
        for angle in [0.0, 30.0, 77.0, 135.0, 200.0, 333.0] {
            for (x, y) in [
                (0.0, 0.0),
                (200.0, 0.0),
                (0.0, 40.0),
                (200.0, 40.0),
                (57.0, 13.0),
            ] {
                let t = f(angle).t_at(rect, x, y);
                assert!((0.0..=1.0).contains(&t), "{angle} {x},{y}: {t}");
            }
        }
    }

    #[test]
    fn contrast_follows_wcag() {
        let c = contrast(Rgba::rgb(0x000000), Rgba::rgb(0xFFFFFF));
        assert!((c - 21.0).abs() < 0.01, "{c}");
        assert!((contrast(Rgba::rgb(0x777777), Rgba::rgb(0x777777)) - 1.0).abs() < 1e-4);
        // #767676 on white is the classic 4.54:1.
        let grey = contrast(Rgba::rgb(0x767676), Rgba::rgb(0xFFFFFF));
        assert!((grey - 4.54).abs() < 0.02, "{grey}");
        // Over: alpha 0 leaves what is under, 1 covers it.
        let red = Rgba::rgb(0xFF0000);
        assert_eq!(red.with_alpha(0.0).over(INK_LIGHT), INK_LIGHT);
        assert_eq!(red.over(INK_LIGHT), red);
    }

    #[test]
    fn light_fades_get_dark_text_and_dark_fades_light_text() {
        let under = Rgba::rgb(0x0B0B0C);
        let pastel = fade(&[stop(0.0, 0xFFE0F0), stop(1.0, 0xD8F0FF)], 0.0, 1.0);
        let (kept, ink) = readable_fade(pastel, 1.0, under, None);
        assert_eq!(ink, INK_DARK);
        assert_eq!(kept, pastel, "already readable: unchanged");
        let night = fade(&[stop(0.0, 0x101030), stop(1.0, 0x301010)], 0.0, 1.0);
        let (kept, ink) = readable_fade(night, 1.0, under, None);
        assert_eq!(ink, INK_LIGHT);
        assert_eq!(kept, night);
    }

    #[test]
    fn no_fade_leaves_text_unreadable() {
        // Every pairing of these, at several opacities, over dark and light surfaces: the
        // chosen text reads on every part of the (possibly adjusted) fade.
        let colours = [
            0x000000, 0xFFFFFF, 0x808080, 0xFF0000, 0x00FF00, 0x0000FF, 0xFFFF00, 0x7C5CFF,
            0x777777,
        ];
        for under in [Rgba::rgb(0x0B0B0C), Rgba::rgb(0xF8B9CA), GLASS_BACKDROP] {
            for &a in &colours {
                for &b in &colours {
                    for &c in &[None, Some(0x888888)] {
                        let mut stops = vec![stop(0.0, a), stop(1.0, b)];
                        if let Some(c) = c {
                            stops.insert(1, stop(0.5, c));
                        }
                        for alpha in [1.0, 0.3] {
                            for softness in [0.0, 1.0] {
                                let f = fade(&stops, 30.0, softness);
                                let (fixed, ink) = readable_fade(f, alpha, under, None);
                                assert!(
                                    fade_reads(&fixed, alpha, under, ink),
                                    "{a:06X} {c:?} {b:06X} alpha {alpha} under {under:?}"
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn a_hard_fade_from_white_to_black_is_toned_down_to_read() {
        let f = fade(&[stop(0.0, 0xFFFFFF), stop(1.0, 0x000000)], 0.0, 1.0);
        let (fixed, ink) = readable_fade(f, 1.0, Rgba::rgb(0x0B0B0C), None);
        assert_ne!(fixed, f, "one end had to move");
        assert!(fade_reads(&fixed, 1.0, Rgba::rgb(0x0B0B0C), ink));
        // Only as far as needed: the end that was fine is untouched.
        let untouched = if ink == INK_LIGHT {
            fixed.last()
        } else {
            fixed.first()
        };
        assert!(untouched == Rgba::rgb(0x000000) || untouched == Rgba::rgb(0xFFFFFF));
    }

    #[test]
    fn the_accent_stands_out_from_a_fade_like_it() {
        let lilac = fade(&[stop(0.0, 0xD8CCFF), stop(1.0, 0xC9B8FF)], 0.0, 1.0);
        let under = Rgba::rgb(0x0B0B0C);
        let (lilac, ink) = readable_fade(lilac, 1.0, under, None);
        let accent = stand_out(Rgba::rgb(0xB8A6FF), &lilac, 1.0, under, ink);
        for i in 0..=10 {
            let bg = lilac.sample(i as f32 / 10.0);
            assert!(contrast(accent, bg) >= UI_CONTRAST, "{i}");
        }
    }

    #[test]
    fn every_fade_theme_reads_as_designed_on_solid_surfaces() {
        // On a solid surface a built-in fade is shown exactly as designed (not adjusted), and
        // its text, dim text and accent all read on every part of it.
        let accent = Rgba::rgb(0x7C5CFF);
        let apps: Vec<AppTheme> = AppTheme::ALL
            .into_iter()
            .filter(|a| a.is_fade() && *a != AppTheme::Custom)
            .collect();
        assert!(apps.len() >= 8, "at least eight fade themes");
        let mut moving = 0;
        for app in apps {
            let Some(p) = fade_palette(app) else {
                panic!("{app:?} has a palette");
            };
            moving += usize::from(p.motion.is_some());
            let distinct = p.stops.iter().map(|s| s.1).collect::<Vec<_>>();
            assert!(
                distinct.windows(2).all(|w| w[0] != w[1]),
                "{app:?} is not one flat colour"
            );
            for s in Surface::ALL.into_iter().filter(|s| !s.is_glass()) {
                let t = Theme::for_surface(s, accent).with_app_theme(app);
                let Some(f) = t.fade else {
                    panic!("{app:?} draws a fade");
                };
                let designed: Vec<Rgba> = p.stops.iter().map(|s| Rgba::rgb(s.1)).collect();
                let got: Vec<Rgba> = f.stops().iter().map(|s| s.color).collect();
                assert_eq!(got, designed, "{app:?} on {s:?} shown as designed");
                assert!(fade_reads(&f, 1.0, t.under(), t.text), "{app:?} text");
                for i in 0..=20 {
                    let bg = f.sample(i as f32 / 20.0);
                    assert!(
                        contrast(t.accent, bg) >= UI_CONTRAST,
                        "{app:?} accent at {i}: {}",
                        contrast(t.accent, bg)
                    );
                }
            }
            // On glass the text still reads (the fade may be toned for it).
            for s in Surface::ALL.into_iter().filter(|s| s.is_glass()) {
                let t = Theme::for_surface(s, accent).with_app_theme(app);
                let Some(f) = t.fade else {
                    panic!("{app:?} draws a fade on glass");
                };
                assert!(
                    fade_reads(&f, t.fade_alpha, GLASS_BACKDROP, t.text),
                    "{app:?} {s:?}"
                );
            }
        }
        assert!(moving >= 2, "at least two fade themes move");
    }

    #[test]
    fn a_drift_cycles_and_returns() {
        let f = fade(
            &[
                stop(0.0, 0xFF0000),
                stop(0.5, 0x00FF00),
                stop(1.0, 0x0000FF),
            ],
            10.0,
            1.0,
        );
        let m = Motion::Drift { period: 30.0 };
        // At the start of a round it is the fade itself; a whole round later too.
        assert_eq!(m.apply(f, 0.0).stops(), f.stops());
        let later = m.apply(f, 30.0);
        for (a, b) in later.stops().iter().zip(f.stops()) {
            assert!(close(a.color, b.color));
        }
        // A third of the way round each stop has the next one's colour.
        let third = m.apply(f, 10.0);
        assert!(close(third.stops()[0].color, Rgba::rgb(0x00FF00)));
        assert!(close(third.stops()[1].color, Rgba::rgb(0x0000FF)));
        assert!(close(third.stops()[2].color, Rgba::rgb(0xFF0000)));
        // Positions stay; the direction sways but only a little.
        for t in [3.0, 7.5, 12.0, 22.0] {
            let d = m.apply(f, t);
            assert_eq!(d.stops()[1].at, 0.5);
            let sway = (d.angle - 10.0 + 180.0).rem_euclid(360.0) - 180.0;
            assert!(sway.abs() <= DRIFT_SWAY + 1e-3, "{t}: {sway}");
        }
        // Slow: a drift frame moves the colours only a hair.
        let step = m.apply(f, 1.0 / DRIFT_FPS);
        assert!((step.stops()[0].color.r - 1.0).abs() < 0.02);
        assert_eq!(m.shimmer(5.0), None);
        assert!((m.next_frame_in(4.0) - 1.0 / DRIFT_FPS).abs() < 1e-6);
    }

    #[test]
    fn a_shimmer_sweeps_then_rests() {
        let m = Motion::Shimmer { period: 9.0 };
        let f = fade(&[stop(0.0, 0x101030), stop(1.0, 0x301010)], 0.0, 1.0);
        assert_eq!(m.apply(f, 4.0), f, "a shimmer leaves the colours alone");
        // The light enters from outside the left edge and leaves past the right one.
        let start = m.shimmer(0.0).unwrap_or(f32::NAN);
        assert!(start < 0.0 && start + SHIMMER_HALF_WIDTH <= 1e-4);
        let end = m.shimmer(SHIMMER_SWEEP - 1e-4).unwrap_or(f32::NAN);
        assert!(end - SHIMMER_HALF_WIDTH >= 1.0 - 1e-3, "{end}");
        let mid = m.shimmer(SHIMMER_SWEEP / 2.0).unwrap_or(f32::NAN);
        assert!((mid - 0.5).abs() < 1e-3);
        // Between sweeps: nothing, and the next frame is the next sweep (no idle frames).
        assert_eq!(m.shimmer(5.0), None);
        assert!((m.next_frame_in(5.0) - 4.0).abs() < 1e-3);
        assert!((m.next_frame_in(9.0 + 0.5) - 1.0 / SHIMMER_FPS).abs() < 1e-6);
        // The last frame of a sweep comes just after it, to draw the light away.
        let last = m.next_frame_in(SHIMMER_SWEEP - 0.01);
        assert!(last > 0.01 && last < 1.0 / SHIMMER_FPS);
        assert_eq!(m.shimmer(SHIMMER_SWEEP - 0.01 + last), None);
    }

    #[test]
    fn a_custom_fade_comes_from_the_settings() {
        let a = Appearance {
            app_theme: AppTheme::Custom,
            ..Appearance::default()
        };
        let t = Theme::from_appearance(&a);
        let Some(f) = t.fade else {
            panic!("the custom fade is drawn");
        };
        assert_eq!(f.stops().len(), 3);
        assert!(t.motion.is_none());
        assert!(fade_reads(&f, t.fade_alpha, t.under(), t.text));
        // Light colours get dark text, and the fade at half opacity lets the surface through.
        let mut light = a.clone();
        light.fade.stops[0].color = "#FFF4E0".into();
        light.fade.stops[1].color = "#FFE6F2".into();
        light.fade.stops[2].color = "#E6F4FF".into();
        let t = Theme::from_appearance(&light);
        assert_eq!(t.text, INK_DARK);
        light.fade.opacity = 0.5;
        let t = Theme::from_appearance(&light);
        assert_eq!(t.fill_top, Rgba::rgb(0x0B0B0C), "Black shows through");
        assert!((t.fade_alpha - 0.5).abs() < 1e-6);
        let Some(f) = t.fade else {
            panic!("still a fade");
        };
        assert!(fade_reads(&f, 0.5, t.under(), t.text));
        // On glass the fade is the tint.
        light.surface = Surface::Acrylic;
        let t = Theme::from_appearance(&light);
        assert!(t.glass.is_some());
        assert!((t.fade_alpha - GLASS_FADE_ALPHA * 0.5).abs() < 1e-6);
    }

    #[test]
    fn animation_can_be_switched_off() {
        let mut a = Appearance {
            app_theme: AppTheme::Aurora,
            ..Appearance::default()
        };
        assert!(Theme::from_appearance(&a).motion.is_some());
        a.animate = false;
        let t = Theme::from_appearance(&a);
        assert!(t.motion.is_none());
        assert_eq!(
            t.fade_at(12.0),
            t.fade,
            "a still fade does not change with time"
        );
    }

    #[test]
    fn the_body_colour_matches_the_layers() {
        let a = Appearance {
            app_theme: AppTheme::MonoFade,
            ..Appearance::default()
        };
        let t = Theme::from_appearance(&a);
        let backdrop = Rgba::rgb(0x336699);
        // Top to bottom (90°): lighter at the top, darker at the bottom, any x.
        let top = t.body_at(200.0, 40.0, 50.0, 0.0, backdrop, 0.0);
        let bottom = t.body_at(200.0, 40.0, 150.0, 40.0, backdrop, 0.0);
        assert!(top.rel_luminance() > bottom.rel_luminance());
        assert!(close(bottom, Rgba::rgb(0x08080A)), "{bottom:?}");
        // The sheen only lifts the upper half.
        let below = t.body_at(200.0, 40.0, 50.0, 30.0, backdrop, 0.0);
        assert!(close(
            below,
            t.fade_at(0.0).map_or(below, |f| f.sample(0.75))
        ));
        // Glass shows the backdrop through the tint.
        let glass = Theme::from_appearance(&Appearance {
            surface: Surface::Acrylic,
            ..a
        });
        let c = glass.body_at(200.0, 40.0, 100.0, 30.0, backdrop, 0.0);
        assert!(c.b > c.r, "the blue backdrop shows: {c:?}");
    }
}
