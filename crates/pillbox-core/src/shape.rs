//! Pill geometry, per-pixel hit-testing and window placement. All values are physical pixels
//! unless a name says `dip`.

use crate::settings::{Layout, SIZE_MAX, SIZE_MIN, Side, Style};

/// A rounded rectangle in window pixel coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pill {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub radius: f32,
}

/// Arguments for `CreateRoundRectRgn`: the interactive area of the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegionKey {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
    pub ellipse: i32,
}

impl Pill {
    pub fn width(&self) -> f32 {
        self.right - self.left
    }

    pub fn height(&self) -> f32 {
        self.bottom - self.top
    }

    /// Corner radius limited to what fits in the rectangle.
    pub fn effective_radius(&self) -> f32 {
        self.radius
            .min(self.width() / 2.0)
            .min(self.height() / 2.0)
            .max(0.0)
    }

    /// True if the point is inside the rounded shape (corner cut-outs are outside).
    pub fn contains(&self, x: f32, y: f32) -> bool {
        if x < self.left || x >= self.right || y < self.top || y >= self.bottom {
            return false;
        }
        let r = self.effective_radius();
        // Nearest corner-circle centre; points between the centres are in the straight part.
        let cx = x.clamp(self.left + r, self.right - r);
        let cy = y.clamp(self.top + r, self.bottom - r);
        let (dx, dy) = (x - cx, y - cy);
        dx * dx + dy * dy <= r * r
    }

    /// Signed distance to the outline (negative inside) and the outward unit normal.
    pub fn distance_and_normal(&self, x: f32, y: f32) -> (f32, (f32, f32)) {
        let r = self.effective_radius();
        let (cx, cy) = (
            (self.left + self.right) / 2.0,
            (self.top + self.bottom) / 2.0,
        );
        let (px, py) = (x - cx, y - cy);
        let (sx, sy) = (
            if px < 0.0 { -1.0 } else { 1.0 },
            if py < 0.0 { -1.0 } else { 1.0 },
        );
        let ax = px.abs() - (self.width() / 2.0 - r);
        let ay = py.abs() - (self.height() / 2.0 - r);
        let outside = (ax.max(0.0).powi(2) + ay.max(0.0).powi(2)).sqrt();
        let d = outside + ax.max(ay).min(0.0) - r;
        let normal = if ax > 0.0 && ay > 0.0 {
            let len = outside.max(1e-6);
            (sx * ax / len, sy * ay / len)
        } else if ax > ay {
            (sx, 0.0)
        } else {
            (0.0, sy)
        };
        (d, normal)
    }

    /// The window region for this shape, grown by `pad` pixels so the hard region edge never
    /// cuts into the anti-aliased outline.
    pub fn region(&self, pad: i32) -> RegionKey {
        RegionKey {
            left: self.left.floor() as i32 - pad,
            top: self.top.floor() as i32 - pad,
            // CreateRoundRectRgn excludes the right and bottom edge, hence the +1.
            right: self.right.ceil() as i32 + pad + 1,
            bottom: self.bottom.ceil() as i32 + pad + 1,
            ellipse: (2.0 * self.effective_radius()).round() as i32 + 2 * pad,
        }
    }
}

/// Integer rectangle in virtual-screen pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RectI {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl RectI {
    pub fn width(&self) -> i32 {
        self.right - self.left
    }

    pub fn height(&self) -> i32 {
        self.bottom - self.top
    }
}

/// Width, height and corner radius in DIPs, in screen orientation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Dims {
    pub w: f32,
    pub h: f32,
    pub r: f32,
}

/// Margin between the largest shape and the window edge, in DIPs.
const MARGIN: f32 = 8.0;
/// Thickness and length of the stats strip on the top edge, in DIPs.
const STRIP_THICKNESS: f32 = 24.0;
const STRIP_LENGTH: f32 = 400.0;
/// Width and height of the stats column on a side edge, in DIPs.
const STRIP_COLUMN: (f32, f32) = (66.0, 186.0);
/// How far a notch reaches past the screen edge so its near corners are never visible.
const NOTCH_OVERHANG: f32 = 4.0;

/// Where Pillbox sits and what its three shapes are, derived from the layout settings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    pub side: Side,
    pub notch: bool,
    /// Gap between the screen edge and Pillbox, in DIPs (0 for a notch).
    pub distance: f32,
    pub closed: Dims,
    pub open: Dims,
    /// The open card on the list tabs (taller).
    pub tall: Dims,
    /// The open card on the System tab: wide enough for a row of component cards. Only on the
    /// top edge; on the side edges it is the same as `open`.
    pub wide: Dims,
    /// The open card on the Music tab when it shows lyrics beside the player. Only on the top
    /// edge; on the side edges it is the same as `open` (no room for two columns).
    pub lyrics: Dims,
    /// The open card on the Controls tab while a Wi-Fi or Bluetooth list is open: the list
    /// tabs' card, taller.
    pub links: Dims,
    pub strip: Dims,
}

/// The Music tab with lyrics: the player on the left, the words on the right. DIPs.
pub const LYRICS_CARD_W: f32 = 580.0;
pub const LYRICS_CARD_H: f32 = 214.0;

/// The System tab lays its component cards out in two rows. Sizes in DIPs.
pub const SYS_TAB_CARD_W: f32 = 176.0;
pub const SYS_TAB_CARD_H: f32 = 88.0;
pub const SYS_TAB_GAP: f32 = 8.0;
/// Side padding of the card content, and the space above (the tab row) and below the cards.
pub const SYS_TAB_PAD: f32 = 16.0;
const SYS_TAB_HEADER: f32 = 42.0;
const SYS_TAB_FOOT: f32 = 12.0;
/// The most cards the tab can show (CPU, GPU, RAM, disk, network, battery).
pub const SYS_TAB_MAX_CARDS: usize = 6;

/// What the resting pill shows; on the top edge its width follows it. Text widths are in DIPs,
/// measured in the pill's own font sizes (see `REST_TEXT` and `MUSIC_TEXT`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Resting {
    /// The clock (or another short face, or nothing): as small as the text allows.
    Quiet { text: f32 },
    /// A notification, flash, notice or call: `NOTICE_W`, or wider when the text (its width
    /// with the icon and the space around it, 0 if it does not matter) needs it.
    Notice { text: f32 },
    /// A song playing: cover, title (if shown) and bars (unless the visualizer is off); as wide
    /// as the title needs.
    Music {
        title: Option<f32>,
        art: bool,
        bars: bool,
    },
}

/// Text size on the resting pill, as a share of its height: the clock face and the song title.
pub const REST_TEXT: f32 = 0.44;
pub const MUSIC_TEXT: f32 = 0.40;
/// A notice with an icon: its text size, and the room the icon and the ends of the pill take,
/// as shares of the pill's height.
pub const NOTICE_TEXT: f32 = 0.38;
pub const NOTICE_LEAD: f32 = 0.85;
pub const NOTICE_PAD: f32 = 0.45;
/// The resting pill is never narrower than this (it still reads as a pill with the clock).
pub const RESTING_MIN_W: f32 = 88.0;
/// A playing song's pill: at least this wide with a title, at most this wide in any case.
pub const MUSIC_MIN_W: f32 = 196.0;
pub const MUSIC_MAX_W: f32 = 360.0;
/// The music pill's six bars (3 px each, 2 px apart), in DIPs.
pub const MUSIC_BARS_W: f32 = 6.0 * 3.0 + 5.0 * 2.0;

/// Where things sit in the music pill of height `h` (DIPs): (inset, cover side, title start,
/// space right of the title up to the pill's end). Drawing and sizing share it, so a title the
/// pill was widened for fits exactly. `bars` = false (visualizer off) leaves no room for them.
pub fn music_pill_metrics(h: f32, art: bool, bars: bool) -> (f32, f32, f32, f32) {
    let inset = (h * 0.16).max(3.0);
    let art_side = h - 2.0 * inset;
    let title_left = if art {
        inset + 2.0 + art_side + 9.0
    } else {
        inset + 8.0
    };
    let right = if bars {
        // Gap before the bars, the bars, and the pad after them.
        12.0 + MUSIC_BARS_W + inset + 6.0
    } else {
        // The same pad as a title with no cover has on its left.
        inset + 8.0
    };
    (inset, art_side, title_left, right)
}

/// `layout.size`, kept in range (a hand-edited file is clamped on load, this guards the rest).
fn draw_size(l: &Layout) -> f32 {
    if l.size.is_finite() {
        l.size.clamp(SIZE_MIN, SIZE_MAX)
    } else {
        1.0
    }
}

/// The scale Pillbox is drawn at: the monitor's DPI scale times `layout.size`.
pub fn draw_scale(dpi_scale: f32, l: &Layout) -> f32 {
    dpi_scale * draw_size(l)
}

/// Cards in the first of the System tab's two rows (the second row gets the rest).
pub fn system_tab_columns(cards: usize) -> usize {
    cards.div_ceil(2).max(1)
}

/// Pillbox's shapes at `layout.size` = 1, in DIPs: the resting pill with a notification
/// (its height is the pill height everywhere), the open card, and the taller card of the list
/// tabs (home widgets, clipboard, shelf, reminders, controls).
pub const NOTICE_W: f32 = 200.0;
pub const PILL_H: f32 = 36.0;
pub const OPEN_W: f32 = 400.0;
pub const OPEN_H: f32 = 160.0;
pub const TALL_H: f32 = 330.0;
/// The Controls tab with a Wi-Fi or Bluetooth list open.
pub const LINKS_H: f32 = 426.0;
/// Corner radius of the open cards, in DIPs.
const CARD_RADIUS: f32 = 34.0;

impl Placement {
    /// Pillbox's place and shapes for the layout settings. Shapes are in DIPs at
    /// `layout.size` = 1: the size is applied by drawing at `dpi scale × size` (see
    /// `draw_scale`), so text, icons and shapes all grow together. The gap to the screen edge is
    /// divided by the size here, so it stays the distance that was set.
    pub fn from_layout(l: &Layout) -> Self {
        let notch = l.style == Style::Notch;
        // On the left and right edges the resting pill stands upright.
        let (cw, ch) = match l.side {
            Side::Top => (NOTICE_W, PILL_H),
            Side::Left | Side::Right => (PILL_H, NOTICE_W),
        };
        // The stats strip: one row of numbers along the top edge, or a short column of them
        // on a side edge.
        let (sw, sh, sr) = match l.side {
            Side::Top => (
                STRIP_LENGTH.min(OPEN_W),
                STRIP_THICKNESS,
                STRIP_THICKNESS / 2.0,
            ),
            Side::Left | Side::Right => (STRIP_COLUMN.0, STRIP_COLUMN.1, 12.0),
        };
        let size = draw_size(l);
        let mut placement = Self {
            side: l.side,
            notch,
            distance: if notch { 0.0 } else { l.distance / size },
            closed: Dims {
                w: cw,
                h: ch,
                r: cw.min(ch) / 2.0,
            },
            open: Dims {
                w: OPEN_W,
                h: OPEN_H,
                r: CARD_RADIUS.min(OPEN_W.min(OPEN_H) / 2.0),
            },
            tall: Dims {
                w: OPEN_W,
                h: TALL_H,
                r: CARD_RADIUS.min(OPEN_W.min(TALL_H) / 2.0),
            },
            links: Dims {
                w: OPEN_W,
                h: LINKS_H,
                r: CARD_RADIUS.min(OPEN_W.min(LINKS_H) / 2.0),
            },
            lyrics: if l.side == Side::Top {
                Dims {
                    w: LYRICS_CARD_W,
                    h: LYRICS_CARD_H,
                    r: CARD_RADIUS,
                }
            } else {
                Dims {
                    w: OPEN_W,
                    h: OPEN_H,
                    r: CARD_RADIUS.min(OPEN_W.min(OPEN_H) / 2.0),
                }
            },
            strip: Dims {
                w: sw,
                h: sh,
                r: sr,
            },
            // Filled in just below, from the open card.
            wide: Dims {
                w: 0.0,
                h: 0.0,
                r: 0.0,
            },
        };
        placement.wide = placement.system_tab(SYS_TAB_MAX_CARDS);
        placement
    }

    /// The open card on the System tab for `cards` component cards in two rows: just big enough
    /// for them (at least the normal open card). On the side edges the width stays the normal
    /// one and the cards get narrower.
    pub fn system_tab(&self, cards: usize) -> Dims {
        let cols = system_tab_columns(cards) as f32;
        let w = cols * SYS_TAB_CARD_W + (cols - 1.0) * SYS_TAB_GAP + 2.0 * SYS_TAB_PAD;
        let h = SYS_TAB_HEADER + 2.0 * SYS_TAB_CARD_H + SYS_TAB_GAP + SYS_TAB_FOOT;
        let w = match self.side {
            Side::Top => self.open.w.max(w),
            Side::Left | Side::Right => self.open.w,
        };
        let h = self.open.h.max(h);
        Dims {
            w,
            h,
            r: self.open.r.min(w.min(h) / 2.0),
        }
    }

    /// The resting pill for what it shows. On the top edge the width follows the content: the
    /// clock alone is small, a notification takes `NOTICE_W` or more, a song widens to
    /// its title. On the side edges the pill stands upright and keeps its size.
    pub fn resting(&self, what: Resting) -> Dims {
        if self.side != Side::Top {
            return self.closed;
        }
        // Always the configured height: only the width follows the content.
        let h = self.closed.h;
        let w = match what {
            Resting::Quiet { text } => (text + h).clamp(RESTING_MIN_W, MUSIC_MAX_W),
            Resting::Notice { text } => {
                let least = self.closed.w.max(RESTING_MIN_W);
                text.clamp(least, least.max(MUSIC_MAX_W))
            }
            Resting::Music { title, art, bars } => {
                let (_, _, left, right) = music_pill_metrics(h, art, bars);
                match title {
                    Some(t) => (left + t + right).clamp(MUSIC_MIN_W, MUSIC_MAX_W),
                    None => (left + right).clamp(RESTING_MIN_W, MUSIC_MAX_W),
                }
            }
        };
        Dims {
            w,
            h,
            r: self.closed.r,
        }
    }

    /// Window size in DIPs: large enough for the biggest shape plus a margin.
    pub fn window_size_dip(&self) -> (f32, f32) {
        let widest_rest = if self.side == Side::Top {
            MUSIC_MAX_W
        } else {
            0.0
        };
        let w = self
            .closed
            .w
            .max(widest_rest)
            .max(self.open.w)
            .max(self.tall.w)
            .max(self.wide.w)
            .max(self.lyrics.w)
            .max(self.links.w);
        let h = self
            .closed
            .h
            .max(self.open.h)
            .max(self.tall.h)
            .max(self.wide.h)
            .max(self.lyrics.h)
            .max(self.links.h);
        match self.side {
            Side::Top => (w + 2.0 * MARGIN, self.distance + h + MARGIN),
            Side::Left | Side::Right => (self.distance + w + MARGIN, h + 2.0 * MARGIN),
        }
    }

    /// Window rectangle on the monitor: (x, y, width, height) in pixels.
    pub fn window_rect(&self, monitor: RectI, scale: f32) -> (i32, i32, u32, u32) {
        let (w_dip, h_dip) = self.window_size_dip();
        let w = ((w_dip * scale).round().max(1.0) as i32).min(monitor.width().max(1));
        let h = ((h_dip * scale).round().max(1.0) as i32).min(monitor.height().max(1));
        let (x, y) = match self.side {
            Side::Top => (monitor.left + (monitor.width() - w) / 2, monitor.top),
            Side::Left => (monitor.left, monitor.top + (monitor.height() - h) / 2),
            Side::Right => (monitor.right - w, monitor.top + (monitor.height() - h) / 2),
        };
        (x, y, w as u32, h as u32)
    }

    /// The shape in window pixels. `hide` slides it off the screen edge: 0 = in place,
    /// 1 = fully out of sight.
    pub fn pill(&self, win: (u32, u32), scale: f32, d: Dims, hide: f32) -> Pill {
        let (w, h) = ((d.w * scale).max(0.0), (d.h * scale).max(0.0));
        let radius = (d.r * scale).max(0.0);
        let gap = self.distance * scale;
        // A notch starts beyond the edge, so the corners on that side are cut off by it.
        let over = if self.notch {
            radius + NOTCH_OVERHANG * scale
        } else {
            0.0
        };
        let (win_w, win_h) = (win.0 as f32, win.1 as f32);
        match self.side {
            Side::Top => {
                let shift = hide * (gap + h + 2.0);
                let cx = win_w / 2.0;
                Pill {
                    left: cx - w / 2.0,
                    top: gap - over - shift,
                    right: cx + w / 2.0,
                    bottom: gap + h - shift,
                    radius,
                }
            }
            Side::Left => {
                let shift = hide * (gap + w + 2.0);
                let cy = win_h / 2.0;
                Pill {
                    left: gap - over - shift,
                    top: cy - h / 2.0,
                    right: gap + w - shift,
                    bottom: cy + h / 2.0,
                    radius,
                }
            }
            Side::Right => {
                let shift = hide * (gap + w + 2.0);
                let cy = win_h / 2.0;
                Pill {
                    left: win_w - gap - w + shift,
                    top: cy - h / 2.0,
                    right: win_w - gap + over + shift,
                    bottom: cy + h / 2.0,
                    radius,
                }
            }
        }
    }

    /// A thin band on the screen edge that brings a hidden Pillbox back when the pointer
    /// touches it. Same length as the resting pill.
    pub fn trigger_region(&self, win: (u32, u32), scale: f32) -> RegionKey {
        let t = (3.0 * scale).ceil() as i32;
        let (win_w, win_h) = (win.0 as i32, win.1 as i32);
        let (len_w, len_h) = (
            (self.closed.w * scale).round() as i32,
            (self.closed.h * scale).round() as i32,
        );
        let (left, top, right, bottom) = match self.side {
            Side::Top => ((win_w - len_w) / 2, 0, (win_w + len_w) / 2, t),
            Side::Left => (0, (win_h - len_h) / 2, t, (win_h + len_h) / 2),
            Side::Right => (win_w - t, (win_h - len_h) / 2, win_w, (win_h + len_h) / 2),
        };
        RegionKey {
            left,
            top,
            right: right + 1,
            bottom: bottom + 1,
            ellipse: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pill() -> Pill {
        Pill {
            left: 100.0,
            top: 8.0,
            right: 300.0,
            bottom: 44.0,
            radius: 18.0,
        }
    }

    fn top() -> Placement {
        Placement::from_layout(&Layout::default())
    }

    fn placed(side: Side, style: Style) -> Placement {
        Placement::from_layout(&Layout {
            side,
            style,
            ..Layout::default()
        })
    }

    #[test]
    fn centre_and_straight_edges_are_inside() {
        let p = pill();
        assert!(p.contains(200.0, 26.0));
        assert!(p.contains(100.5, 26.0));
        assert!(p.contains(299.0, 26.0));
        assert!(p.contains(200.0, 8.5));
        assert!(p.contains(200.0, 43.5));
    }

    #[test]
    fn outside_the_bounding_box_is_outside() {
        let p = pill();
        assert!(!p.contains(99.0, 26.0));
        assert!(!p.contains(300.0, 26.0));
        assert!(!p.contains(200.0, 7.0));
        assert!(!p.contains(200.0, 44.0));
    }

    #[test]
    fn corner_cutouts_are_outside() {
        let p = pill();
        for (x, y) in [(102.0, 10.0), (298.0, 10.0), (102.0, 42.0), (298.0, 42.0)] {
            assert!(!p.contains(x, y), "({x},{y}) should be in a corner cut-out");
        }
    }

    #[test]
    fn just_inside_the_corner_arc_is_inside() {
        let p = pill();
        // Arc centre is (118, 26); a point 17 px away on the diagonal is inside, 19 px is not.
        let d = 17.0 / std::f32::consts::SQRT_2;
        assert!(p.contains(118.0 - d, 26.0 - d));
        let d = 19.0 / std::f32::consts::SQRT_2;
        assert!(!p.contains(118.0 - d, 26.0 - d));
    }

    #[test]
    fn oversized_radius_is_clamped() {
        let p = Pill {
            radius: 500.0,
            ..pill()
        };
        assert_eq!(p.effective_radius(), 18.0);
        assert!(p.contains(200.0, 26.0));
        assert!(!p.contains(101.0, 9.0));
    }

    #[test]
    fn zero_radius_is_a_plain_rectangle() {
        let p = Pill {
            radius: 0.0,
            ..pill()
        };
        assert!(p.contains(100.0, 8.0));
        assert!(p.contains(299.9, 43.9));
    }

    #[test]
    fn degenerate_shapes_contain_nothing() {
        let p = Pill {
            left: 10.0,
            top: 10.0,
            right: 10.0,
            bottom: 10.0,
            radius: 5.0,
        };
        assert!(!p.contains(10.0, 10.0));
        let p = Pill {
            radius: -3.0,
            ..pill()
        };
        assert_eq!(p.effective_radius(), 0.0);
    }

    #[test]
    fn signed_distance_agrees_with_contains() {
        let p = pill();
        for y in 0..52 {
            for x in 90..310 {
                let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
                let (d, n) = p.distance_and_normal(fx, fy);
                // Skip the half-pixel band right on the outline.
                if d.abs() > 0.75 {
                    assert_eq!(d < 0.0, p.contains(fx, fy), "({fx},{fy}) d={d}");
                }
                let len = (n.0 * n.0 + n.1 * n.1).sqrt();
                assert!((len - 1.0).abs() < 1e-3, "normal not unit at ({fx},{fy})");
            }
        }
        // Normals point away from the centre.
        assert_eq!(p.distance_and_normal(101.0, 26.0).1, (-1.0, 0.0));
        assert_eq!(p.distance_and_normal(200.0, 43.0).1, (0.0, 1.0));
    }

    #[test]
    fn region_covers_the_shape_with_padding() {
        let r = pill().region(1);
        assert_eq!(
            r,
            RegionKey {
                left: 99,
                top: 7,
                right: 302,
                bottom: 46,
                ellipse: 38
            }
        );
    }

    #[test]
    fn default_placement_matches_m1_geometry() {
        let p = top();
        let monitor = RectI {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1080,
        };
        // The window is as wide as the widest card (the Music tab with lyrics) and as tall as
        // the tallest (Controls with a Wi-Fi or Bluetooth list open), plus the margins.
        assert_eq!(p.window_rect(monitor, 1.0), (662, 0, 596, 442));
        let closed = p.pill((592, 346), 1.0, p.closed, 0.0);
        assert_eq!(
            (closed.left, closed.top, closed.right, closed.bottom),
            (196.0, 8.0, 396.0, 44.0)
        );
        assert_eq!(closed.radius, 18.0);
        let open = p.pill((592, 346), 1.0, p.open, 0.0);
        assert_eq!(
            (open.width(), open.height(), open.radius),
            (400.0, 160.0, 34.0)
        );
        let tall = p.pill((592, 346), 1.0, p.tall, 0.0);
        assert_eq!((tall.top, tall.width(), tall.height()), (8.0, 400.0, 330.0));
    }

    #[test]
    fn the_system_tab_card_fits_its_cards_in_two_rows() {
        let t = top();
        // Six cards: three columns. Five: still three (3 + 2). Four: two columns, which
        // fit in the normal open card.
        assert_eq!(t.system_tab(6).w, 3.0 * 176.0 + 2.0 * 8.0 + 32.0);
        assert_eq!(t.system_tab(5).w, t.system_tab(6).w);
        assert_eq!(t.system_tab(4).w, 400.0);
        assert_eq!(t.system_tab(6).h, 42.0 + 2.0 * 88.0 + 8.0 + 12.0);
        // The window holds the biggest version.
        assert_eq!(t.wide, t.system_tab(6));
        let l = placed(Side::Left, Style::Pill);
        assert_eq!(l.system_tab(6).w, l.open.w);
        assert_eq!(system_tab_columns(1), 1);
        assert_eq!(system_tab_columns(5), 3);
    }

    #[test]
    fn the_resting_pill_fits_what_it_shows() {
        let t = top();
        // The clock alone: small, never below the minimum.
        assert_eq!(t.resting(Resting::Quiet { text: 70.0 }).w, 70.0 + 36.0);
        assert_eq!(t.resting(Resting::Quiet { text: 10.0 }).w, RESTING_MIN_W);
        // A notification: the fixed width from the layout.
        let notice = |text| t.resting(Resting::Notice { text });
        assert_eq!(notice(0.0).w, t.closed.w);
        // A long notice widens the pill, up to a limit.
        assert_eq!(notice(260.0).w, 260.0);
        assert_eq!(notice(900.0).w, MUSIC_MAX_W);
        // A song: cover + title + bars, between the music limits.
        let (_, _, left, right) = music_pill_metrics(36.0, true, true);
        let song = |w| {
            t.resting(Resting::Music {
                title: Some(w),
                art: true,
                bars: true,
            })
            .w
        };
        assert_eq!(song(150.0), left + 150.0 + right);
        assert_eq!(song(5.0), MUSIC_MIN_W);
        assert_eq!(song(900.0), MUSIC_MAX_W);
        assert!(
            t.resting(Resting::Music {
                title: None,
                art: false,
                bars: true,
            })
            .w >= RESTING_MIN_W
        );
        // The configured height and corners, a little taller (same roundness) for a song.
        assert_eq!(notice(0.0).h, t.closed.h);
        assert_eq!(t.resting(Resting::Quiet { text: 0.0 }).r, t.closed.r);
        let song_pill = t.resting(Resting::Music {
            title: None,
            art: true,
            bars: true,
        });
        assert_eq!(song_pill.h, t.closed.h, "a song does not make it taller");
        assert_eq!(song_pill.r, t.closed.r);
        // Upright pills on the side edges keep their size.
        let l = placed(Side::Left, Style::Pill);
        assert_eq!(
            l.resting(Resting::Music {
                title: Some(300.0),
                art: true,
                bars: true,
            }),
            l.closed
        );
        // With the visualizer off there is no room kept for bars: narrower by exactly that.
        let (_, _, _, no_bars) = music_pill_metrics(36.0, true, false);
        assert!(no_bars < right);
        let quiet_song = t.resting(Resting::Music {
            title: Some(250.0),
            art: true,
            bars: false,
        });
        assert_eq!(quiet_song.w, left + 250.0 + no_bars);
        assert_eq!(song(250.0) - quiet_song.w, right - no_bars);
        // The window has room for the widest song pill.
        assert!(t.window_size_dip().0 >= MUSIC_MAX_W + 16.0);
    }

    #[test]
    fn pill_scales_with_dpi() {
        let p = top();
        for scale in [1.0f32, 1.5, 2.0] {
            let win = ((592.0 * scale) as u32, (346.0 * scale) as u32);
            let pill = p.pill(win, scale, p.closed, 0.0);
            assert!((pill.width() - 200.0 * scale).abs() < 0.01);
            assert!((pill.height() - 36.0 * scale).abs() < 0.01);
            assert!((pill.top - 8.0 * scale).abs() < 0.01);
            assert!(pill.contains(296.0 * scale, 26.0 * scale));
            assert!(!pill.contains(pill.left + 2.0 * scale, pill.top + 2.0 * scale));
        }
    }

    #[test]
    fn window_follows_its_monitor_and_side() {
        let left_monitor = RectI {
            left: -2560,
            top: -200,
            right: 0,
            bottom: 1240,
        };
        assert_eq!(
            top().window_rect(left_monitor, 2.0),
            (-1876, -200, 1192, 884)
        );
        let monitor = RectI {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1080,
        };
        // Left edge: window hugs x = 0 and is centred vertically.
        let l = placed(Side::Left, Style::Pill);
        let (x, y, w, h) = l.window_rect(monitor, 1.0);
        assert_eq!((x, w, h), (0, 416, 442));
        assert_eq!(y, (1080 - 442) / 2);
        // Right edge: window ends exactly at the monitor's right edge.
        let r = placed(Side::Right, Style::Pill);
        let (x, _, w, _) = r.window_rect(monitor, 1.0);
        assert_eq!(x + w as i32, 1920);
    }

    #[test]
    fn window_never_exceeds_a_tiny_monitor() {
        let tiny = RectI {
            left: 0,
            top: 0,
            right: 300,
            bottom: 100,
        };
        assert_eq!(top().window_rect(tiny, 2.0), (0, 0, 300, 100));
    }

    #[test]
    fn side_pills_stand_upright_and_hug_their_edge() {
        let l = placed(Side::Left, Style::Pill);
        assert_eq!((l.closed.w, l.closed.h), (36.0, 200.0));
        let win = (416, 346);
        let pill = l.pill(win, 1.0, l.closed, 0.0);
        assert_eq!((pill.left, pill.right), (8.0, 44.0));
        assert_eq!((pill.top + pill.bottom) / 2.0, 173.0);
        let r = placed(Side::Right, Style::Pill);
        let pill = r.pill(win, 1.0, r.closed, 0.0);
        assert_eq!((pill.left, pill.right), (416.0 - 44.0, 416.0 - 8.0));
    }

    #[test]
    fn notch_touches_the_edge_and_hides_its_near_corners() {
        let n = placed(Side::Top, Style::Notch);
        assert_eq!(n.distance, 0.0);
        let pill = n.pill((416, 168), 1.0, n.closed, 0.0);
        assert!(
            pill.top < -pill.effective_radius(),
            "top corners must be off-screen"
        );
        assert_eq!(pill.bottom, 36.0);
        // The very top row of the window is inside the shape across its full width.
        assert!(pill.contains(pill.left + 0.5, 0.5));
        assert!(pill.contains(pill.right - 0.5, 0.5));
    }

    #[test]
    fn hiding_slides_the_pill_off_screen() {
        for side in [Side::Top, Side::Left, Side::Right] {
            let p = placed(side, Style::Pill);
            let (w, h) = p.window_size_dip();
            let win = (w as u32, h as u32);
            let hidden = p.pill(win, 1.0, p.closed, 1.0);
            let off = match side {
                Side::Top => hidden.bottom < 0.0,
                Side::Left => hidden.right < 0.0,
                Side::Right => hidden.left > win.0 as f32,
            };
            assert!(off, "{side:?}: {hidden:?}");
            let trigger = p.trigger_region(win, 1.0);
            assert!(trigger.right > trigger.left && trigger.bottom > trigger.top);
            assert!(trigger.left >= 0 && trigger.top >= 0);
            assert!(trigger.right <= win.0 as i32 + 1 && trigger.bottom <= win.1 as i32 + 1);
        }
    }

    #[test]
    fn the_music_card_with_lyrics_is_wider_only_on_the_top_edge() {
        let t = top();
        assert_eq!((t.lyrics.w, t.lyrics.h), (LYRICS_CARD_W, LYRICS_CARD_H));
        assert!(t.lyrics.w > t.open.w && t.lyrics.h > t.open.h);
        assert!(
            t.window_size_dip().0 >= LYRICS_CARD_W + 16.0,
            "the window has room"
        );
        for side in [Side::Left, Side::Right] {
            let p = placed(side, Style::Pill);
            assert_eq!(p.lyrics, p.open, "{side:?}");
        }
    }

    #[test]
    fn size_scales_everything_but_the_gap_to_the_edge() {
        let normal = top();
        assert_eq!((normal.closed.r, normal.open.r), (18.0, 34.0));
        let big_layout = Layout {
            size: 1.25,
            distance: 10.0,
            ..Layout::default()
        };
        let big = Placement::from_layout(&big_layout);
        // The shapes stay in DIPs at size 1; the drawing scale carries the size.
        assert_eq!(big.closed, normal.closed);
        assert_eq!(big.open, normal.open);
        assert!((draw_scale(1.5, &big_layout) - 1.875).abs() < 1e-6);
        // The gap on screen is still the distance that was set: 10 DIPs × 1.5 = 15 px.
        assert!((big.distance * draw_scale(1.5, &big_layout) - 15.0).abs() < 1e-4);
        // On screen the pill is 25% bigger.
        let win = big.window_size_dip();
        let k = draw_scale(1.0, &big_layout);
        let p = big.pill((win.0 as u32 * 2, win.1 as u32 * 2), k, big.closed, 0.0);
        assert!((p.height() - PILL_H * 1.25).abs() < 1e-4);
        // An out-of-range size (a hand-edited value) is held in range.
        let silly = Layout {
            size: 9.0,
            ..Layout::default()
        };
        assert!((draw_scale(1.0, &silly) - SIZE_MAX).abs() < 1e-6);
        let nan = Layout {
            size: f32::NAN,
            ..Layout::default()
        };
        assert_eq!(draw_scale(2.0, &nan), 2.0);
    }
}
