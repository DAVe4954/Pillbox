//! What Pillbox looks like: surface, rim, the resting pill, and the open card with its tabs
//! (home, music, lyrics). Pure drawing from a snapshot of state; no `unsafe`, no I/O.
//!
//! `paint` also returns the clickable areas it drew, so clicks are tested against exactly
//! what is on screen.

use pillbox_core::{
    cliphistory::ClipFilter,
    face::{PET_HEIGHT, PET_WIDTH, PetPixel, pet_pixels},
    music::{BANDS, GLOW_STOPS, Palette, format_clock},
    settings::{Music, ProgressStyle, Side, Visualizer},
    shape::Pill,
    theme::{Rgba, Theme, Touch},
};

use crate::{
    lyrics::LyricsState,
    platform::gfx::{Align, Frame, ImageSlot, TextStyle},
    tabs::{self, UiState},
    widgets,
};

/// What the resting pill shows when no music is playing.
#[derive(Debug, Clone, PartialEq)]
pub enum Face {
    Empty,
    Text(String),
    Pet { blink: bool },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tab {
    Home,
    Music,
    Clipboard,
    Shelf,
    Reminders,
    Controls,
    /// M7: coding agents and live statuses from scripts.
    Activity,
    /// CPU, GPU, RAM, disk, network and battery, one card each, on a wide card.
    System,
    /// M8: Windows toast notifications mirrored on Pillbox.
    Notifications,
}

impl Tab {
    pub fn name(self) -> &'static str {
        match self {
            Tab::Home => "Home",
            Tab::Music => "Music",
            Tab::Clipboard => "Clipboard",
            Tab::Shelf => "Shelf",
            Tab::Reminders => "Reminders",
            Tab::Controls => "Controls",
            Tab::Activity => "Activity",
            Tab::Notifications => "Notifications",
            Tab::System => "System",
        }
    }

    /// Segoe Fluent Icons glyph.
    pub fn icon(self) -> &'static str {
        match self {
            Tab::Home => "\u{E80F}",
            Tab::Music => "\u{E8D6}",
            Tab::Clipboard => "\u{E77F}",
            Tab::Shelf => "\u{E7B8}",
            Tab::Reminders => "\u{EA8F}",
            Tab::Controls => "\u{E9E9}",
            Tab::Activity => "\u{E9D9}",
            Tab::Notifications => "\u{E8BD}",
            Tab::System => "\u{E950}",
        }
    }

    /// List tabs use the taller card.
    pub fn tall(self) -> bool {
        !matches!(self, Tab::Music | Tab::System)
    }
}

/// Text boxes in the card. Only one has the keyboard at a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Field {
    ClipSearch,
    ReminderAdd,
    Note,
    Todo,
    Calc,
}

/// A line in the shelf file menu (right-click on a shelf file).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShelfCmd {
    Open,
    OpenWith,
    ShowInFolder,
    /// Save a copy somewhere (offered for files that came from the phone).
    Download,
    SendToPhone,
    Remove,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Action {
    Tab(Tab),
    PlayPause,
    Next,
    Previous,
    /// The progress bar: where a click or drag lands between `left` and `right` is the
    /// position in the song.
    SeekBar {
        left: f32,
        right: f32,
    },
    /// Jump to this many seconds (a lyrics line).
    SeekTo(f32),
    /// "Return to live": follow the line being sung again.
    LyricsLive,
    /// The lyrics column: only there so the mouse wheel over it scrolls the words.
    LyricsArea,
    /// Click into a text box.
    Focus(Field),
    ClipFilter(ClipFilter),
    ClipCopy(u64),
    ClipPin(u64),
    ClipDelete(u64),
    /// Copy the text in a picture.
    ClipOcr(u64),
    ClipClear,
    SnippetCopy(usize),
    /// Press on a shelf file: select it, or start dragging it out.
    ShelfItem(usize),
    ShelfSelectAll,
    ShelfRemove,
    ShelfCopy,
    /// Show a QR code that lets a phone download this shelf file (M6).
    ShelfPhone(usize),
    /// Show a QR code that lets a phone send photos, files and text here (M6).
    PhoneReceive,
    /// Close the QR code (and stop the phone server).
    PhoneClose,
    /// A line of the shelf file menu, for the file at this index.
    ShelfMenu(usize, ShelfCmd),
    /// A click outside the open shelf menu: just close it.
    ShelfMenuClose,
    /// Activity tab (M7). The numbers are the click numbers (`uid`) of sessions and statuses,
    /// or a permission question's id.
    AgentAllow(u64),
    AgentDeny(u64),
    /// Stop the agent's process; asks to be clicked twice.
    AgentStop(u64),
    /// Open a new chat of this agent in a terminal, in the same folder.
    AgentNewChat(u64),
    /// A notification in the list: open the app that posted it (by the notification's id).
    NoteOpen(u32),
    /// The small X at the right end of the tab row: quit the whole app (asks for a second click).
    Quit,
    /// Bring the Claude desktop app to the front (Claude Code cards).
    AgentOpen(u64),
    /// Show or hide a session's timeline.
    AgentExpand(u64),
    /// Take a finished session or a status off the card.
    ActivityDismiss(u64),
    ReminderAdd,
    ReminderDone(u64),
    ReminderSnooze(u64),
    ReminderDelete(u64),
    /// A slider: the value is where the click or drag lands between `left` and `right`.
    Volume {
        left: f32,
        right: f32,
    },
    Brightness {
        left: f32,
        right: f32,
    },
    SpeakerMute,
    MicMute,
    Wifi,
    Bluetooth,
    SoundSettings,
    /// Open or fold the Wi-Fi or Bluetooth list on the Controls tab.
    LinkPanel(LinkPanel),
    /// Join the saved network at this place in the Wi-Fi list.
    WifiJoin(usize),
    WifiLeave,
    /// Connect or disconnect the Bluetooth device with this address.
    BtConnect(u64),
    BtDisconnect(u64),
    /// Open Windows' own page for the list (new networks, pairing).
    LinkSettings(LinkPanel),
    /// Turn the camera mirror on or off.
    Mirror,
    /// Open or close the chat drawer.
    ChatDrawer,
    Launch(usize),
    OpenSettings,
    Widget(WidgetAction),
}

/// The two lists the Controls tab can open under its cards.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkPanel {
    Wifi,
    Bluetooth,
}

/// Clicks inside widgets on the Home tab.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WidgetAction {
    FocusTimer,
    FocusReset,
    TodoToggle(u64),
    TodoDelete(u64),
    /// Index into the shown widgets: pop it out onto the desktop.
    PopOut(usize),
    PhotoNext,
    /// Expand or fold the Play time card.
    PlayToggle,
    /// Show this day (0 = today) in the expanded Play time card.
    PlayDay(usize),
    /// Expand or fold the detailed Weather card (the next five days).
    WeatherToggle,
}

/// A short notice on the resting pill: lock keys, mic mute, screenshot saved, colour copied.
#[derive(Debug, Clone, PartialEq)]
pub struct Toast {
    /// Segoe Fluent Icons glyph, or empty for none.
    pub icon: &'static str,
    pub text: String,
    /// A colour dot (the colour picker's result).
    pub swatch: Option<Rgba>,
    /// Draw the icon in the warning colour (e.g. microphone muted).
    pub alert: bool,
}

impl Toast {
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            icon: "",
            text: text.into(),
            swatch: None,
            alert: false,
        }
    }

    pub fn icon(icon: &'static str, text: impl Into<String>) -> Self {
        Self {
            icon,
            ..Self::text(text)
        }
    }
}

/// The stack of recent notifications (M8) on the closed pill: up to three overlapping app
/// badges, the newest one's text, and how many more are stacked behind it.
#[derive(Debug, Clone, PartialEq)]
pub struct NoteStackView {
    /// Newest first: the app id and name of each stacked notification (at most 3); the badge is
    /// its icon, or a coloured letter.
    pub badges: Vec<(String, String)>,
    pub text: String,
    /// Notifications in the stack, and which of them is in front (0 = the newest).
    pub count: usize,
    pub index: usize,
}

/// A clickable rectangle (left, top, right, bottom) in window pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hit {
    pub rect: (f32, f32, f32, f32),
    pub action: Action,
}

impl Hit {
    pub fn contains(&self, x: f32, y: f32) -> bool {
        let (l, t, r, b) = self.rect;
        x >= l && x < r && y >= t && y < b
    }
}

pub struct MusicView<'a> {
    pub title: &'a str,
    pub artist: &'a str,
    pub playing: bool,
    /// Seconds.
    pub position: f32,
    pub duration: f32,
    pub has_art: bool,
    pub palette: Palette,
    pub bands: [f32; BANDS],
    pub lyrics: &'a LyricsState,
    /// Fraction under the pointer while the progress bar is being dragged.
    pub scrub: Option<f32>,
    /// Controls the player accepts; the rest are drawn dimmed and do nothing.
    pub can_seek: bool,
    pub can_next: bool,
    pub can_previous: bool,
    /// The Music tab's reactive glow: its stops and the lyric-line flash; None when off.
    pub glow: Option<([f32; GLOW_STOPS], f32)>,
    /// The lyrics scrolled by hand: the line (fractional) in view; None = following the song.
    pub lyrics_view: Option<f32>,
}

/// How the text on the closed pill looks (the clock and the other text faces).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextLook {
    /// Times the normal text size.
    pub size: f32,
    pub bold: bool,
    pub color: Rgba,
}

pub struct Scene<'a> {
    pub pill: Pill,
    /// Window size in pixels; content is laid out in the part of the pill that is on screen.
    pub win: (f32, f32),
    pub scale: f32,
    pub side: Side,
    pub theme: &'a Theme,
    /// Blur radius for glass surfaces, in pixels.
    pub blur: f32,
    /// Displacement map and strength for the glass "bend", when the surface has one.
    pub bend: Option<(&'a [u8], f32)>,
    /// 0 = resting pill, 1 = fully open card.
    pub openness: f32,
    /// The playing song's colour over the resting pill: top, bottom, and how much shows (0..1).
    pub tint: Option<([Rgba; 3], f32)>,
    /// Stats-strip mode (a fullscreen game or a "strip" app rule): live numbers, no card.
    pub strip: Option<&'a StripView>,
    pub face: &'a Face,
    pub text_look: TextLook,
    pub time: &'a str,
    pub date: &'a str,
    pub day_progress: f32,
    /// Seconds since start, drives decorations and the wave progress bar.
    pub clock: f32,
    pub tab: Tab,
    pub music: Option<MusicView<'a>>,
    /// The Music tab has the lyrics column (decided up front, so the layout does not flip
    /// halfway through the card's opening animation).
    pub music_wide: bool,
    pub music_settings: &'a Music,
    pub visualizer: Visualizer,
    /// A short notice shown on the resting pill in place of its usual content.
    pub toast: Option<&'a Toast>,
    /// Recent notifications stacked on the resting pill (below a toast, above a call).
    pub notes: Option<&'a NoteStackView>,
    /// A call is going on (an app is using the microphone): timer on the resting pill.
    pub call: Option<CallView<'a>>,
    /// Microphone / camera in use: the coloured dot on the pill.
    pub sensor_dot: (bool, bool),
    /// The quit button was clicked once: it shows red and the next click quits.
    pub quit_armed: bool,
    /// Left edge (window pixels) of the normal open card: where the tab icons start, whatever
    /// the width of the card that is open.
    pub tab_left: f32,
    /// Tabs to show, in order.
    pub tabs: &'a [Tab],
    /// Where the active tab's highlight is, as a (fractional) index into `tabs`: it slides
    /// from tab to tab.
    pub tab_chip: f32,
    /// How far the current tab's content has eased in after a tab switch, 0..1.
    pub tab_in: f32,
    /// Everything the list tabs draw from.
    pub ui: &'a UiState<'a>,
}

/// What a frame produced besides pixels.
#[derive(Debug, Default)]
pub struct Painted {
    pub hits: Vec<Hit>,
    /// How far the current tab's list can scroll, in pixels.
    pub scroll_max: f32,
}

/// The numbers on the stats strip, already smoothed and formatted where needed.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StripView {
    /// Whole-system CPU load, 0..100.
    pub cpu: Option<f32>,
    /// Memory in use, 0..100.
    pub ram: Option<f32>,
    /// Busiest GPU's 3D load, 0..100, and its temperature in °C.
    pub gpu: Option<f32>,
    pub gpu_temp: Option<f32>,
    /// Download and upload speed text, e.g. "4.2 Mb/s".
    pub down: Option<String>,
    pub up: Option<String>,
    pub usage_bars: bool,
    /// No surface behind the numbers.
    pub numbers_only: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct CallView<'a> {
    pub seconds: u64,
    /// The app using the microphone.
    pub app: &'a str,
    pub muted: bool,
}

/// Text colours for the current background.
#[derive(Clone, Copy)]
pub(crate) struct Ink {
    pub text: Rgba,
    pub dim: Rgba,
    pub accent: Rgba,
    /// The active tab's highlight.
    pub chip: Rgba,
    /// What the tiles and rows of the list tabs are washed with (at a low alpha).
    pub wash: Rgba,
}

pub(crate) type Rect = (f32, f32, f32, f32);

/// Cheap deterministic pseudo-random number in 0..1 for decoration `i`, channel `k`.
fn noise(i: u32, k: u32) -> f32 {
    let mut x = i.wrapping_mul(0x9E37_79B9) ^ k.wrapping_mul(0x85EB_CA6B) ^ 0x1234_5678;
    x ^= x >> 15;
    x = x.wrapping_mul(0x2C1B_3C6D);
    x ^= x >> 12;
    (x & 0xFFFF) as f32 / 65535.0
}

pub(crate) fn label(size: f32, bold: bool, color: Rgba, align: Align) -> TextStyle {
    TextStyle {
        icon: false,
        size,
        bold,
        color,
        align,
    }
}

pub(crate) fn icon(size: f32, color: Rgba) -> TextStyle {
    TextStyle {
        icon: true,
        size,
        bold: false,
        color,
        align: Align::Center,
    }
}

/// The part of `pill` on screen, as a rectangle (a notch starts beyond the screen edge, and
/// its fade and light are laid out on what shows).
fn shown_rect(pill: &Pill) -> Rect {
    (pill.left, pill.top.max(0.0), pill.right, pill.bottom)
}

/// How glass shows the screen behind it: the blur radius, and the displacement map and
/// strength of its bend (when it has one).
pub(crate) type Backdrop<'a> = (f32, Option<(&'a [u8], f32)>);

/// The body of the pill, the card or a widget window, `clock` seconds in: the surface (the
/// blurred screen on glass, when `backdrop` gives its blur and bend and a capture is ready),
/// the theme's fade over it, a shimmer's light, and the soft light across the top. The
/// Settings previews draw the same layers from `Theme::body_at`.
pub(crate) fn paint_body(
    f: &mut Frame<'_>,
    pill: &Pill,
    t: &Theme,
    clock: f32,
    scale: f32,
    backdrop: Option<Backdrop<'_>>,
) {
    let fade = t.fade_at(clock);
    let glass = match (t.glass, backdrop) {
        (Some(g), Some((blur, bend))) if f.fill_pill_backdrop(pill, blur, bend) => Some(g),
        _ => None,
    };
    let rect = shown_rect(pill);
    match glass {
        // A fade on glass is the tint itself.
        Some(_) if fade.is_some() => {}
        Some(g) => f.fill_pill_gradient(pill, g.tint_top, g.tint_bottom),
        // A fade that covers everything needs nothing under it.
        None if fade.is_some() && t.fade_alpha >= 0.999 => {}
        // Not glass, or the capture has not produced a frame yet.
        None => f.fill_pill_gradient(pill, t.fill_top, t.fill_bottom),
    }
    if let Some(fd) = fade {
        let stops: Vec<(f32, Rgba)> = fd
            .brush_stops()
            .into_iter()
            .map(|(at, c)| (at, c.fade(t.fade_alpha)))
            .collect();
        let (start, end) = fd.line(rect);
        f.fill_pill_linear(pill, &stops, start, end);
    }
    if let Some(x) = t.motion.and_then(|m| m.shimmer(clock)) {
        // A soft band of light crossing the body, leaning like a reflection.
        let light = Rgba::rgb(0xFFFFFF);
        let band = pillbox_core::theme::shimmer_band(x).map(|(at, a)| (at, light.with_alpha(a)));
        let (l, top, r, b) = rect;
        let lean = (b - top) * pillbox_core::theme::SHIMMER_LEAN;
        // Positions past the ends are clamped by the brush, so the light enters and leaves
        // cleanly.
        f.fill_pill_linear(pill, &band, (l + lean, top), (r + lean, b));
    }
    // Light from above: glass has its gloss, solid surfaces a fainter sheen.
    let (light, depth, inset) = match glass {
        Some(g) => (g.gloss, 0.52, 2.0),
        None => (t.sheen(), pillbox_core::theme::SHEEN_DEPTH, 1.0),
    };
    if light > 0.0 {
        let inset = inset * scale;
        let top = pill.top.max(0.0);
        let upper = Pill {
            left: pill.left + inset,
            top: top + inset,
            right: pill.right - inset,
            bottom: top + (pill.bottom - top) * depth,
            radius: (pill.effective_radius() - inset).max(0.0),
        };
        let white = Rgba::rgb(0xFFFFFF);
        f.fill_pill_gradient(&upper, white.with_alpha(light), white.with_alpha(0.0));
    }
}

/// The theme's rim, lit from above: brighter along the top edge, softer along the bottom.
pub(crate) fn paint_rim(f: &mut Frame<'_>, pill: &Pill, t: &Theme, scale: f32) {
    let Some(rim) = t.rim else {
        return;
    };
    let (_, top, _, bottom) = shown_rect(pill);
    let stops = [
        (0.0, rim.with_alpha((rim.a * 1.35).min(1.0))),
        (0.5, rim),
        (1.0, rim.with_alpha(rim.a * 0.55)),
    ];
    f.stroke_pill_linear(pill, &stops, (0.0, top), (0.0, bottom), scale.max(1.0));
}

pub fn paint(f: &mut Frame<'_>, s: &Scene<'_>) -> Painted {
    let pill = &s.pill;
    if pill.width() < 1.0 || pill.height() < 1.0 {
        return Painted::default();
    }
    let t = s.theme;
    if let Some(strip) = s.strip
        && strip.numbers_only
    {
        paint_strip(f, s, strip);
        return Painted::default();
    }

    // Body.
    paint_body(f, pill, t, s.clock, s.scale, Some((s.blur, s.bend)));
    // A song playing on the resting pill colours it like its cover: its (up to) three colours
    // flow slowly along the pill from end to end (one loop every 24 s; the pill redraws anyway
    // while music plays), with depth from a darker bottom, a soft sheen on the upper half and a
    // thin rim that follows the colours. It fades out as the card opens (the open card has its
    // own cover background) and when the song stops.
    let tint = s
        .tint
        .map(|(colours, amount)| (colours, amount * (1.0 - s.openness).clamp(0.0, 1.0)))
        .filter(|(_, a)| *a > 0.01);
    if let Some((colours, a)) = tint {
        let phase = s.clock / 24.0;
        let stops = pillbox_core::music::pill_stops(&colours, phase);
        let body: Vec<(f32, Rgba)> = stops
            .iter()
            .map(|(x, c)| (*x, c.with_alpha(0.95 * a)))
            .collect();
        f.fill_pill_hgradient(pill, &body);
        // Depth: the lower part sinks into shadow.
        let shade = Rgba::new(0.0, 0.0, 0.0, 1.0);
        f.fill_pill_gradient(pill, shade.with_alpha(0.0), shade.with_alpha(0.42 * a));
        let lift = |c: Rgba| {
            let up = |x: f32| (x * 2.4 + 0.16).min(1.0);
            Rgba::new(up(c.r), up(c.g), up(c.b), 1.0)
        };
        let inset = 1.5 * s.scale;
        let upper = Pill {
            left: pill.left + inset,
            top: pill.top.max(0.0) + inset,
            right: pill.right - inset,
            bottom: pill.top.max(0.0) + (pill.bottom - pill.top.max(0.0)) * 0.55,
            radius: (pill.effective_radius() - inset).max(0.0),
        };
        let sheen = Rgba::new(1.0, 1.0, 1.0, 1.0);
        f.fill_pill_gradient(&upper, sheen.with_alpha(0.10 * a), sheen.with_alpha(0.0));
        let rim: Vec<(f32, Rgba)> = stops
            .iter()
            .map(|(x, c)| (*x, lift(*c).with_alpha(0.38 * a)))
            .collect();
        f.stroke_pill_hgradient(pill, &rim, s.scale.max(1.0));
    }
    if tint.is_none() {
        paint_rim(f, pill, t, s.scale);
    }
    if let Some(strip) = s.strip {
        paint_strip(f, s, strip);
        return Painted::default();
    }

    // The part of the pill that is on screen (a notch starts beyond the screen edge).
    let visible = (
        pill.left.max(0.0),
        pill.top.max(0.0),
        pill.right.min(s.win.0),
        pill.bottom.min(s.win.1),
    );

    let closed_alpha = (1.0 - s.openness * 2.5).clamp(0.0, 1.0);
    if closed_alpha > 0.0 {
        paint_resting(f, s, visible, closed_alpha);
    }
    let open_alpha = ((s.openness - 0.4) / 0.6).clamp(0.0, 1.0);
    if open_alpha > 0.0 {
        let mut out = paint_open(f, s, visible, open_alpha);
        // Only a (nearly) fully open card is clickable.
        if open_alpha <= 0.85 {
            out.hits.clear();
        }
        return out;
    }
    Painted::default()
}

// ---- stats strip ------------------------------------------------------------------------------

/// The strip over a game: CPU, RAM, download and upload, each a dim label and a value.
fn paint_strip(f: &mut Frame<'_>, s: &Scene<'_>, v: &StripView) {
    let pill = &s.pill;
    let k = s.scale;
    let (l, t, r, b) = (
        pill.left.max(0.0),
        pill.top.max(0.0),
        pill.right.min(s.win.0),
        pill.bottom.min(s.win.1),
    );
    if r - l < 8.0 || b - t < 8.0 {
        return;
    }
    // Over a game with no background the numbers are white with a dark shadow; on the
    // Pillbox's surface they use its own colours.
    let (text, dim) = if v.numbers_only {
        let white = Rgba::rgb(0xFFFFFF);
        (white, white.with_alpha(0.75))
    } else {
        (s.theme.text, s.theme.text_dim)
    };
    let percent = |p: Option<f32>| p.map_or("–".to_string(), |p| format!("{p:.0}%"));
    // GPU load and temperature share one item: "45% · 71°".
    let gpu = match (v.gpu, v.gpu_temp) {
        (Some(load), Some(temp)) => format!("{load:.0}% · {temp:.0}°"),
        (load, _) => percent(load),
    };
    let items: [(&str, String, Option<f32>); 5] = [
        ("CPU", percent(v.cpu), v.cpu),
        ("GPU", gpu, v.gpu),
        ("RAM", percent(v.ram), v.ram),
        ("↓", v.down.clone().unwrap_or_else(|| "–".into()), None),
        ("↑", v.up.clone().unwrap_or_else(|| "–".into()), None),
    ];
    let shadowed = |f: &mut Frame<'_>, txt: &str, rect: Rect, style: TextStyle| {
        if v.numbers_only {
            let o = k.max(1.0);
            let shadow = TextStyle {
                color: Rgba::new(0.0, 0.0, 0.0, 0.8),
                ..style
            };
            f.text(txt, rect.0 + o, rect.1 + o, rect.2 + o, rect.3 + o, shadow);
        }
        f.text(txt, rect.0, rect.1, rect.2, rect.3, style);
    };
    let bar_color = |p: f32| if p >= 90.0 { ALERT } else { s.theme.accent };
    let usage_bar = |f: &mut Frame<'_>, left: f32, right: f32, y: f32, p: f32| {
        let half = 1.0 * k;
        let track = Pill {
            left,
            top: y - half,
            right,
            bottom: y + half,
            radius: half,
        };
        f.fill_pill(&track, text.with_alpha(0.18));
        let fill = Pill {
            right: left + (right - left) * (p / 100.0).clamp(0.02, 1.0),
            ..track
        };
        f.fill_pill(&fill, bar_color(p));
    };

    if s.side == Side::Top {
        let pad = (b - t) * 0.55;
        let size = (b - t) * 0.44;
        // Each item gets the width it needs; what is left is shared out as equal gaps.
        let widths: Vec<(f32, f32)> = items
            .iter()
            .map(|(name, value, _)| {
                (
                    f.measure(name, size, false) + 4.0 * k,
                    f.measure(value, size, true),
                )
            })
            .collect();
        let used: f32 = widths.iter().map(|(a, b)| a + b).sum();
        let gap = ((r - l - 2.0 * pad - used) / items.len() as f32).max(0.0);
        let mut x = l + pad + gap / 2.0;
        for ((name, value, usage), (name_w, value_w)) in items.iter().zip(widths) {
            let start = x;
            let x0 = x;
            let cell = name_w + value_w;
            x += cell + gap;
            shadowed(
                f,
                name,
                (start, t, start + name_w, b),
                label(size, false, dim, Align::Left),
            );
            shadowed(
                f,
                value,
                (start + name_w, t, x0 + cell, b),
                label(size, true, text, Align::Left),
            );
            if v.usage_bars
                && let Some(p) = usage
            {
                let y = b - (b - t) * 0.16;
                usage_bar(f, start, start + name_w + value_w, y, *p);
            }
        }
    } else {
        // A column: label above value, five times.
        let row = (b - t) / items.len() as f32;
        let (name_size, value_size) = (9.5 * k, 11.5 * k);
        for (i, (name, value, usage)) in items.iter().enumerate() {
            let y0 = t + row * i as f32 + (row - name_size * 1.3 - value_size * 1.35) / 2.0;
            let y1 = y0 + name_size * 1.3;
            shadowed(
                f,
                name,
                (l, y0, r, y1),
                label(name_size, false, dim, Align::Center),
            );
            shadowed(
                f,
                value,
                (l + 2.0 * k, y1, r - 2.0 * k, y1 + value_size * 1.35),
                label(value_size, true, text, Align::Center),
            );
            if v.usage_bars
                && let Some(p) = usage
            {
                let inset = (r - l) * 0.24;
                usage_bar(f, l + inset, r - inset, y1 + value_size * 1.45, *p);
            }
        }
    }
}

// ---- resting pill -----------------------------------------------------------------------------

fn paint_resting(f: &mut Frame<'_>, s: &Scene<'_>, rect: Rect, alpha: f32) {
    paint_sensor_dot(f, s, rect, alpha);
    if let Some(toast) = s.toast {
        return paint_toast(f, s, toast, rect, alpha);
    }
    if let Some(stack) = s.notes {
        return paint_notes(f, s, stack, rect, alpha);
    }
    if let Some(call) = s.call {
        return paint_call(f, s, call, rect, alpha);
    }
    match &s.music {
        Some(m) if m.playing && s.music_settings.on_pill => paint_music_pill(f, s, m, rect, alpha),
        _ => match s.face {
            Face::Empty => {}
            Face::Pet { blink } => paint_pet(f, s, *blink, rect, alpha),
            Face::Text(text) => paint_text_face(f, s, text, rect, alpha),
        },
    }
}

/// The privacy dot: orange for the microphone, green for the camera (camera wins), at the far
/// end of the pill like the indicator on a phone.
fn paint_sensor_dot(f: &mut Frame<'_>, s: &Scene<'_>, (l, t, r, b): Rect, alpha: f32) {
    let (mic, cam) = s.sensor_dot;
    if !mic && !cam {
        return;
    }
    let color = if cam {
        Rgba::rgb(0x34C759)
    } else {
        Rgba::rgb(0xFF9F0A)
    };
    let rad = 3.0 * s.scale;
    let (cx, cy) = if s.side == Side::Top {
        (r - (b - t) * 0.32, t + (b - t) / 2.0)
    } else {
        (l + (r - l) / 2.0, b - (r - l) * 0.32)
    };
    f.fill_ellipse(cx, cy, rad, rad, color.fade(alpha));
}

fn paint_call(f: &mut Frame<'_>, s: &Scene<'_>, call: CallView<'_>, rect: Rect, alpha: f32) {
    let (l, t, r, b) = rect;
    let h = b - t;
    let time = format_clock(call.seconds as f32);
    if s.side != Side::Top {
        return paint_text_face(f, s, &time, rect, alpha);
    }
    let (glyph, color) = if call.muted {
        ("\u{EC54}", ALERT)
    } else {
        ("\u{E720}", Rgba::rgb(0x34C759))
    };
    let cx = l + h * 0.6;
    f.text(
        glyph,
        cx - h * 0.4,
        t,
        cx + h * 0.4,
        b,
        icon(h * 0.42, color.fade(alpha)),
    );
    let text_left = cx + h * 0.45;
    let text_right = r - h * 0.65;
    f.text(
        &time,
        text_left,
        t,
        text_right,
        b,
        label(h * 0.4, true, s.theme.text.fade(alpha), Align::Left),
    );
    f.text(
        call.app,
        text_left,
        t,
        text_right,
        b,
        label(h * 0.32, false, s.theme.text_dim.fade(alpha), Align::Right),
    );
}

/// Warning red used for "muted" and failures.
const ALERT: Rgba = Rgba {
    r: 1.0,
    g: 0.36,
    b: 0.36,
    a: 1.0,
};

fn paint_toast(f: &mut Frame<'_>, s: &Scene<'_>, toast: &Toast, rect: Rect, alpha: f32) {
    let (l, t, r, b) = rect;
    let h = b - t;
    if s.side != Side::Top || (toast.icon.is_empty() && toast.swatch.is_none()) {
        return paint_text_face(f, s, &toast.text, rect, alpha);
    }
    // Icon or colour dot at the left end, text in the middle.
    let lead = h * 0.5;
    let cx = l + h * 0.55;
    let cy = t + h / 2.0;
    if let Some(c) = toast.swatch {
        // A light ring keeps dark colours visible on a dark pill.
        let ring = lead / 2.0 + 1.5 * s.scale;
        f.fill_ellipse(cx, cy, ring, ring, s.theme.text.with_alpha(0.7).fade(alpha));
        f.fill_ellipse(
            cx,
            cy,
            lead / 2.0,
            lead / 2.0,
            c.with_alpha(1.0).fade(alpha),
        );
    } else {
        let color = if toast.alert { ALERT } else { s.theme.text };
        f.text(
            toast.icon,
            cx - lead,
            t,
            cx + lead,
            b,
            icon(h * 0.42, color.fade(alpha)),
        );
    }
    let pad = h * 0.3;
    f.text(
        &toast.text,
        cx + lead * 0.6,
        t,
        r - pad,
        b,
        label(h * 0.38, true, s.theme.text.fade(alpha), Align::Center),
    );
}

/// Recent notifications: the app badges overlap like a stack of cards (newest in front, at the
/// left), then the newest one's text, and "+N" at the far end when more are stacked behind it.
fn paint_notes(f: &mut Frame<'_>, s: &Scene<'_>, n: &NoteStackView, rect: Rect, alpha: f32) {
    let (l, t, r, b) = rect;
    let h = b - t;
    if s.side != Side::Top {
        // The upright pill has no room: the newest app's letter.
        let letter = n.badges.first().map_or(String::new(), |(_, app)| {
            pillbox_core::notes::badge_letter(app).to_string()
        });
        return paint_text_face(f, s, &letter, rect, alpha);
    }
    let rad = h * 0.27;
    let step = rad * 0.9;
    let cy = t + h / 2.0;
    let x0 = l + h * 0.35 + rad;
    // Back to front, so the newest badge is on top.
    for (i, (app_id, app)) in n.badges.iter().enumerate().rev() {
        let cx = x0 + step * i as f32;
        // A ring in the pill's own colour separates the overlapping discs.
        f.fill_ellipse(
            cx,
            cy,
            rad + 1.5 * s.scale,
            rad + 1.5 * s.scale,
            Rgba::new(0.0, 0.0, 0.0, 0.85).fade(alpha),
        );
        tabs::badge(f, (cx, cy), rad, app_id, app, alpha);
    }
    let text_left = x0 + step * n.badges.len().saturating_sub(1) as f32 + rad + h * 0.25;
    let pad = h * 0.3;
    let text_right = if n.count > 1 {
        r - pad - h * 0.7
    } else {
        r - pad
    };
    f.text(
        &n.text,
        text_left,
        t,
        text_right,
        b,
        label(h * 0.38, true, s.theme.text.fade(alpha), Align::Left),
    );
    if n.count > 1 {
        f.text(
            &if n.index == 0 {
                format!("+{}", n.count - 1)
            } else {
                format!("{}/{}", n.index + 1, n.count)
            },
            text_right,
            t,
            r - pad,
            b,
            label(h * 0.34, false, s.theme.text_dim.fade(alpha), Align::Right),
        );
    }
}

fn paint_text_face(f: &mut Frame<'_>, s: &Scene<'_>, text: &str, (l, t, r, b): Rect, alpha: f32) {
    let (w, h) = (r - l, b - t);
    let look = s.text_look;
    let color = look.color.fade(alpha);
    if s.side == Side::Top {
        let pad = h * 0.3;
        f.text(
            text,
            l + pad,
            t,
            r - pad,
            b,
            label(
                h * pillbox_core::shape::REST_TEXT * look.size,
                look.bold,
                color,
                Align::Center,
            ),
        );
    } else {
        // Upright pill: stack the words. "14:05" becomes two lines.
        let words: Vec<&str> = text.split([' ', ':']).filter(|p| !p.is_empty()).collect();
        // The pill's width is fixed here, so a bigger text size only goes as far as it fits.
        let size = w * 0.36 * look.size.min(1.1);
        let line = size * 1.3;
        let top = t + (h - line * words.len() as f32) / 2.0;
        for (i, word) in words.iter().enumerate() {
            let y = top + line * i as f32;
            f.text(
                word,
                l,
                y,
                r,
                y + line,
                label(size, look.bold, color, Align::Center),
            );
        }
    }
}

fn paint_pet(f: &mut Frame<'_>, s: &Scene<'_>, blink: bool, (l, t, r, b): Rect, alpha: f32) {
    let (w, h) = (r - l, b - t);
    let px = ((w.min(h) * 0.62) / PET_HEIGHT as f32).floor().max(1.0);
    let (sw, sh) = (px * PET_WIDTH as f32, px * PET_HEIGHT as f32);
    let (ox, oy) = ((l + (w - sw) / 2.0).round(), (t + (h - sh) / 2.0).round());
    for (x, y, kind) in pet_pixels(blink) {
        let c = match kind {
            PetPixel::Body => s.theme.text,
            PetPixel::Eye => s.theme.fill_bottom.with_alpha(1.0),
            PetPixel::Blush => s.theme.accent,
        };
        let (x0, y0) = (ox + x as f32 * px, oy + y as f32 * px);
        f.fill_rect(x0, y0, x0 + px, y0 + px, c.fade(alpha));
    }
}

/// Colour for the visualizer and progress accents: from the cover when there is one and the
/// surface is dark; soft pastel themes keep their own accent so it stays readable.
fn music_accent(s: &Scene<'_>, m: &MusicView<'_>) -> Rgba {
    if m.has_art && s.theme.touch.is_none() {
        m.palette.accent
    } else {
        s.theme.accent
    }
}

/// The visualizer's colours on the resting pill: the cover's colours (lifted towards white so
/// they read on the dark pill) when there is a cover on a dark surface, else the accent.
fn vis_colors(s: &Scene<'_>, m: &MusicView<'_>, alpha: f32) -> VisColors {
    if m.has_art && s.theme.touch.is_none() {
        let lift = |c: Rgba| {
            let max = c.r.max(c.g).max(c.b).max(1e-3);
            let up = |x: f32| (x / max) * 0.62 + 0.38;
            Rgba::new(up(c.r), up(c.g), up(c.b), 1.0)
        };
        VisColors::Cover(m.palette.colors.map(lift), s.clock / 24.0)
    } else {
        VisColors::One(music_accent(s, m).fade(alpha))
    }
}

fn paint_music_pill(
    f: &mut Frame<'_>,
    s: &Scene<'_>,
    m: &MusicView<'_>,
    (l, t, r, b): Rect,
    alpha: f32,
) {
    let k = s.scale;
    let (w, h) = (r - l, b - t);
    let colors = vis_colors(s, m, alpha);
    let bars_w = BANDS as f32 * 3.0 * k + (BANDS as f32 - 1.0) * 2.0 * k;
    if s.side == Side::Top {
        // The same measurements the pill was sized with (`shape::music_pill_metrics`), in
        // pixels, so the title it widened for fits.
        let bars = s.visualizer != Visualizer::Off;
        let (inset, art_side, title_left, title_right) =
            pillbox_core::shape::music_pill_metrics(h / k, m.has_art, bars);
        let (inset, art_side) = (inset * k, art_side * k);
        let text_left = l + title_left * k;
        let text_right = r - title_right * k + 1.0;
        if m.has_art {
            let art = Pill {
                left: l + inset + 2.0 * k,
                top: t + inset,
                right: l + inset + 2.0 * k + art_side,
                bottom: b - inset,
                radius: art_side * 0.26,
            };
            f.fill_pill_image(ImageSlot::Art, &art, alpha);
        }
        let bars_left = r - inset - 6.0 * k - bars_w;
        visualizer(
            f,
            s.visualizer,
            bars_left,
            t + h / 2.0,
            h * 0.5,
            &m.bands,
            colors,
            alpha,
            k,
        );
        if s.music_settings.title_in_pill {
            // At most 30 characters; the pill is as wide as that needs (up to its cap), and a
            // short title sits in the middle of the room it has.
            let title = pillbox_core::music::pill_title(m.title);
            let size = h * pillbox_core::shape::MUSIC_TEXT;
            let style = label(size, true, s.theme.text.fade(alpha), Align::Center);
            f.text(&title, text_left, t, text_right, b, style);
        }
    } else {
        // Upright pill: cover on top, bars at the bottom, no room for a title.
        let inset = (w * 0.16).max(3.0 * k);
        let art_side = w - 2.0 * inset;
        if m.has_art {
            let art = Pill {
                left: l + inset,
                top: t + inset + 2.0 * k,
                right: r - inset,
                bottom: t + inset + 2.0 * k + art_side,
                radius: art_side * 0.26,
            };
            f.fill_pill_image(ImageSlot::Art, &art, alpha);
        }
        let bars_h = w * 0.5;
        visualizer(
            f,
            s.visualizer,
            l + (w - bars_w) / 2.0,
            b - inset - 4.0 * k - bars_h / 2.0,
            bars_h,
            &m.bands,
            colors,
            alpha,
            k,
        );
    }
}

/// The visualizer's colours: one accent, or the cover's colours spread across the bars (light
/// enough to read on the dark, tinted pill), drifting slowly with the pill's gradient.
#[derive(Clone, Copy)]
enum VisColors {
    One(Rgba),
    Cover([Rgba; 3], f32),
}

impl VisColors {
    /// Colour at `x` across the visualizer, 0 = left, 1 = right.
    fn at(self, x: f32, alpha: f32) -> Rgba {
        match self {
            VisColors::One(c) => c,
            VisColors::Cover(colors, phase) => {
                let c = pillbox_core::music::cycle_color(&colors, phase + x * 0.55);
                c.fade(alpha)
            }
        }
    }
}

/// Six bands, `height` tall at full level, vertically centred on `cy`, starting at `left`, in
/// the box six 3 px bars with 2 px gaps take (`shape::MUSIC_BARS_W`).
#[allow(clippy::too_many_arguments)]
fn visualizer(
    f: &mut Frame<'_>,
    style: Visualizer,
    left: f32,
    cy: f32,
    height: f32,
    bands: &[f32; BANDS],
    colors: VisColors,
    alpha: f32,
    k: f32,
) {
    let (bar, gap) = (3.0 * k, 2.0 * k);
    let width = BANDS as f32 * bar + (BANDS as f32 - 1.0) * gap;
    let level_of = |i: usize| bands[i].clamp(0.0, 1.0);
    let x_of = |i: usize| left + i as f32 * (bar + gap);
    let shade = |i: usize| colors.at(i as f32 / (BANDS - 1) as f32, alpha);
    let rounded_bar = |f: &mut Frame<'_>, i: usize, top: f32, bottom: f32| {
        let x = x_of(i);
        f.fill_pill(
            &Pill {
                left: x,
                top,
                right: x + bar,
                bottom,
                radius: bar / 2.0,
            },
            shade(i),
        );
    };
    match style {
        Visualizer::RoundedBars => {
            for i in 0..BANDS {
                let h = (level_of(i) * height).max(bar);
                rounded_bar(f, i, cy - h / 2.0, cy + h / 2.0);
            }
        }
        Visualizer::Equalizer => {
            let floor = cy + height / 2.0;
            for i in 0..BANDS {
                let h = (level_of(i) * height).max(bar);
                rounded_bar(f, i, floor - h, floor);
            }
        }
        Visualizer::Burst => {
            for (i, band) in pillbox_core::music::BURST_ORDER.iter().enumerate() {
                let h = (level_of(*band) * height).max(bar);
                rounded_bar(f, i, cy - h / 2.0, cy + h / 2.0);
            }
        }
        Visualizer::PixelBars => {
            // Square cells stacked from the bottom, like an old level meter.
            let cell = bar;
            let step = cell + k;
            let rows = ((height + k) / step).floor().max(1.0) as u32;
            let bottom = cy + (rows as f32 * step - k) / 2.0;
            for i in 0..BANDS {
                let lit = 1 + (level_of(i) * (rows - 1) as f32).round() as u32;
                for row in 0..lit.min(rows) {
                    let y1 = bottom - row as f32 * step;
                    f.fill_rect(x_of(i), y1 - cell, x_of(i) + cell, y1, shade(i));
                }
            }
        }
        Visualizer::Wave => {
            // A smooth curve through the bands, mirrored round the middle and filled, one
            // pixel column at a time; the ends taper so it reads as a sound wave.
            let columns = width.round().max(2.0) as usize;
            for c in 0..columns {
                let x = c as f32 / (columns - 1) as f32;
                let taper = (x * std::f32::consts::PI).sin().max(0.0).sqrt();
                let amp = (pillbox_core::music::smooth_level(bands, x) * taper * height / 2.0)
                    .max(0.75 * k);
                let px = left + c as f32;
                f.fill_rect(px, cy - amp, px + 1.0, cy + amp, colors.at(x, alpha));
            }
        }
        Visualizer::Dots => {
            // Each dot rises with its band and swells a little.
            for i in 0..BANDS {
                let level = level_of(i);
                let rad = bar * (0.45 + 0.35 * level);
                let cx = x_of(i) + bar / 2.0;
                let y = cy + (height / 2.0 - rad) * (1.0 - 2.0 * level);
                f.fill_ellipse(cx, y, rad, rad, shade(i));
            }
        }
        // Switched off: nothing is drawn (and no room was kept for it on the pill).
        Visualizer::Off => {}
    }
}

// ---- open card --------------------------------------------------------------------------------

fn paint_open(f: &mut Frame<'_>, s: &Scene<'_>, (l, t, r, b): Rect, alpha: f32) -> Painted {
    let k = s.scale;
    let pad = 18.0 * k;
    let mut hits = Vec::new();
    let mut scroll_max = 0.0;

    // The Music tab can sit on the blurred cover; then the text is always light.
    let on_art = s.tab == Tab::Music
        && s.music_settings.art_background
        && s.music.as_ref().is_some_and(|m| m.has_art)
        && f.fill_pill_image(ImageSlot::ArtSoft, &s.pill, alpha);
    let ink = if on_art {
        f.fill_pill(&s.pill, Rgba::new(0.0, 0.0, 0.0, 0.26 * alpha));
        if let Some((glow, pulse)) = s.music.as_ref().and_then(|m| m.glow) {
            paint_glow(f, s, glow, pulse, alpha);
        }
        f.stroke_pill(
            &s.pill,
            Rgba::rgb(0xFFFFFF).with_alpha(0.16 * alpha),
            k.max(1.0),
        );
        let white = Rgba::rgb(0xFFFFFF);
        Ink {
            text: white,
            dim: white.with_alpha(0.66),
            accent: s.music.as_ref().map_or(white, |m| m.palette.accent),
            chip: white,
            wash: white,
        }
    } else {
        Ink {
            text: s.theme.text,
            dim: s.theme.text_dim,
            // The home tab keeps the theme's own accent; music views take theirs from the cover.
            accent: match (&s.music, s.tab) {
                (Some(m), Tab::Music) => music_accent(s, m),
                _ => s.theme.accent,
            },
            chip: s.theme.chip,
            wash: s.theme.wash,
        }
    };

    // Tab row: an icon per tab, always on the same pixels: the ones they have on the Home tab,
    // where Home's slot is as wide as its highlight with the name. On another tab only that
    // tab's icon gets the highlight.
    // Anchored to the normal card's left edge, not this card's: the System tab's card is wider
    // (and centred), and the icons must not move when it opens.
    let mut x = s.tab_left + pad - 8.0 * k;
    let (tab_top, tab_h) = (t + 9.0 * k, 24.0 * k);
    // Ten tabs (agents and notifications on) get narrower icons so the row clears the buttons.
    let icon_w = if s.tabs.len() > 8 { 26.0 } else { 30.0 } * k;
    let home_w = icon_w + f.measure(Tab::Home.name(), 12.5 * k, true) + 8.0 * k;
    // Every slot's left edge and width; the highlight slides between them.
    let mut slots = Vec::with_capacity(s.tabs.len());
    {
        let mut sx = x;
        for &tab in s.tabs {
            let w = if tab == Tab::Home { home_w } else { icon_w };
            slots.push((sx, w));
            sx += w + 2.0 * k;
        }
    }
    if let (Some(&first), Some(&last)) = (slots.first(), slots.last()) {
        // The highlight: the whole slot for Home (with its name), the icon for the rest. While
        // it slides its two edges move from one slot's to the other's.
        let at = s.tab_chip.clamp(0.0, (slots.len() - 1) as f32);
        let (from, to) = (at.floor() as usize, at.ceil() as usize);
        let (a, b) = (
            slots.get(from).copied().unwrap_or(first),
            slots.get(to).copied().unwrap_or(last),
        );
        let frac = at - at.floor();
        let left = a.0 + (b.0 - a.0) * frac;
        let right = (a.0 + a.1) + ((b.0 + b.1) - (a.0 + a.1)) * frac;
        let chip = Pill {
            left,
            top: tab_top,
            right,
            bottom: tab_top + tab_h,
            radius: tab_h / 2.0,
        };
        // The surface's own white chip is faint; a coloured one (an app theme) needs more.
        let strength = if ink.chip == ink.text { 0.15 } else { 0.28 };
        f.fill_pill(&chip, ink.chip.with_alpha(strength * alpha));
    }
    let pointer_on = |rect: Rect| {
        s.ui.pointer
            .is_some_and(|(px, py)| px >= rect.0 && px < rect.2 && py >= rect.1 && py < rect.3)
    };
    for &tab in s.tabs {
        let active = tab == s.tab;
        let slot_w = if tab == Tab::Home { home_w } else { icon_w };
        let hit = (x, tab_top - 4.0 * k, x + slot_w, tab_top + tab_h + 4.0 * k);
        let hovered = !active && pointer_on(hit);
        if hovered {
            // Hover: a faint version of the highlight under the icon.
            let under = Pill {
                left: x,
                top: tab_top,
                right: x + icon_w,
                bottom: tab_top + tab_h,
                radius: tab_h / 2.0,
            };
            f.fill_pill(&under, ink.wash.with_alpha(0.08 * alpha));
        }
        let color = if active {
            ink.text
        } else if hovered {
            ink.dim.lerp(ink.text, 0.6)
        } else {
            ink.dim
        };
        f.text(
            tab.icon(),
            x,
            tab_top,
            x + icon_w,
            tab_top + tab_h,
            icon(13.5 * k, color.fade(alpha)),
        );
        if active && tab == Tab::Home {
            f.text(
                tab.name(),
                x + icon_w - 4.0 * k,
                tab_top,
                x + home_w,
                tab_top + tab_h,
                label(12.5 * k, true, color.fade(alpha), Align::Left),
            );
        }
        hits.push(Hit {
            rect: hit,
            action: Action::Tab(tab),
        });
        x += slot_w + 2.0 * k;
    }

    // The quit button at the right end of the tab row: a small X; after one click it turns red
    // and the next click quits the app.
    {
        let rad = 10.0 * k;
        let (qx, qy) = (r - 16.0 * k - rad, tab_top + tab_h / 2.0);
        // The settings gear sits just left of the X.
        let gx = qx - 2.0 * rad - 6.0 * k;
        let gear_hit = (
            gx - rad - 2.0 * k,
            qy - rad - 4.0 * k,
            gx + rad + 2.0 * k,
            qy + rad + 4.0 * k,
        );
        let gear_hot = pointer_on(gear_hit);
        if gear_hot {
            f.fill_ellipse(
                gx,
                qy,
                rad + 1.0 * k,
                rad + 1.0 * k,
                ink.wash.with_alpha(0.1 * alpha),
            );
        }
        f.text(
            "\u{E713}",
            gx - rad,
            qy - rad,
            gx + rad,
            qy + rad,
            icon(
                11.5 * k,
                if gear_hot { ink.text } else { ink.dim }.fade(alpha),
            ),
        );
        hits.push(Hit {
            rect: gear_hit,
            action: Action::OpenSettings,
        });
        let quit_hit = (
            qx - rad - 2.0 * k,
            qy - rad - 4.0 * k,
            qx + rad + 2.0 * k,
            qy + rad + 4.0 * k,
        );
        let quit_hot = pointer_on(quit_hit);
        if s.quit_armed {
            f.fill_ellipse(qx, qy, rad, rad, ALERT.with_alpha(0.9 * alpha));
        } else if quit_hot {
            f.fill_ellipse(
                qx,
                qy,
                rad + 1.0 * k,
                rad + 1.0 * k,
                ink.wash.with_alpha(0.1 * alpha),
            );
        }
        let color = if s.quit_armed {
            Rgba::rgb(0xFFFFFF)
        } else if quit_hot {
            ink.text
        } else {
            ink.dim
        };
        f.text(
            "\u{E711}",
            qx - rad,
            qy - rad,
            qx + rad,
            qy + rad,
            icon(10.5 * k, color.fade(alpha)),
        );
        hits.push(Hit {
            rect: quit_hit,
            action: Action::Quit,
        });
    }
    let tab_row_hits = hits.len();

    let content = (l + pad, t + 42.0 * k, r - pad, b - 12.0 * k);
    let card = (l, t, r, b);
    // After a tab switch the new content eases in (the tab row stays put).
    let alpha = alpha * s.tab_in.clamp(0.0, 1.0);
    let cx = tabs::Ctx {
        k,
        alpha,
        ink,
        ui: s.ui,
    };
    match s.tab {
        Tab::Home if s.ui.widgets.shown.is_empty() => paint_home(f, s, card, content, alpha, ink),
        Tab::Home => scroll_max = tabs::home(f, &cx, content, &mut hits),
        Tab::Music => paint_music(f, s, content, alpha, ink, &mut hits),
        Tab::Clipboard => scroll_max = tabs::clipboard(f, &cx, content, &mut hits),
        Tab::Shelf => scroll_max = tabs::shelf(f, &cx, content, &mut hits),
        Tab::Reminders => scroll_max = tabs::reminders(f, &cx, content, &mut hits),
        Tab::Controls => scroll_max = tabs::controls(f, &cx, content, &mut hits),
        Tab::Activity => scroll_max = tabs::activity(f, &cx, content, &mut hits),
        Tab::Notifications => scroll_max = tabs::notifications(f, &cx, content, &mut hits),
        Tab::System => widgets::paint_system_cards(f, &cx, content, 2),
    }
    paint_pressed(f, &cx, &hits, tab_row_hits);
    Painted { hits, scroll_max }
}

/// Pressed: while the mouse button is held on a button, row or tile, a faint layer of the
/// wash darkens or lightens it (the same for every one, on top of its own hover look).
fn paint_pressed(f: &mut Frame<'_>, cx: &tabs::Ctx<'_>, hits: &[Hit], tab_row: usize) {
    let Some((px, py)) = cx.ui.pressed else {
        return;
    };
    // The last hit drawn wins, as for clicks. The tab row has its own highlight.
    let Some((i, hit)) = hits
        .iter()
        .enumerate()
        .rev()
        .find(|(_, h)| h.contains(px, py))
    else {
        return;
    };
    if i < tab_row {
        return;
    }
    let (l, t, r, b) = hit.rect;
    let radius = ((b - t) / 2.0).min(10.0 * cx.k);
    f.fill_pill(&tabs::rounded((l, t, r, b), radius), cx.wash(0.1));
}

fn paint_home(
    f: &mut Frame<'_>,
    s: &Scene<'_>,
    card: Rect,
    (cl, ct, cr, cb): Rect,
    alpha: f32,
    ink: Ink,
) {
    let k = s.scale;
    let (l, t, r, b) = card;
    let pad = 18.0 * k;

    // Decorations go behind the text.
    match s.theme.touch {
        Some(Touch::Sparkles) => {
            for i in 0..9u32 {
                let x = l + pad + noise(i, 1) * (r - l - 2.0 * pad);
                let y = t + pad * 0.6 + noise(i, 2) * (b - t - 1.2 * pad);
                let twinkle = 0.5
                    + 0.5
                        * (s.clock * (1.2 + noise(i, 3) * 1.8)
                            + noise(i, 4) * std::f32::consts::TAU)
                            .sin();
                let size = (9.0 + noise(i, 5) * 9.0) * k;
                let color = Rgba::rgb(0xFFFFFF)
                    .with_alpha(0.25 + 0.7 * twinkle)
                    .fade(alpha);
                f.text(
                    "✦",
                    x - size,
                    y - size,
                    x + size,
                    y + size,
                    label(size, false, color, Align::Center),
                );
            }
        }
        Some(Touch::Petals) => {
            let span = (b - t) + 24.0 * k;
            for i in 0..8u32 {
                let speed = (14.0 + noise(i, 1) * 16.0) * k;
                let y = t - 12.0 * k + (s.clock * speed + noise(i, 2) * span) % span;
                let sway =
                    (s.clock * (0.7 + noise(i, 3)) + noise(i, 4) * std::f32::consts::TAU).sin();
                let x = l + pad + noise(i, 5) * (r - l - 2.0 * pad) + sway * 10.0 * k;
                let size = (3.0 + noise(i, 6) * 2.5) * k;
                // A petal is a slightly flattened ellipse that tips as it sways.
                let (rx, ry) = (size * (1.25 + 0.25 * sway), size * (0.8 - 0.2 * sway));
                f.fill_ellipse(x, y, rx, ry, s.theme.accent.with_alpha(0.55).fade(alpha));
            }
        }
        Some(Touch::Hearts) | None => {}
    }

    // Clock and date.
    let time_size = ((cb - ct) * 0.38).min(42.0 * k);
    f.text(
        s.time,
        cl,
        ct,
        cr,
        ct + time_size * 1.2,
        label(time_size, true, ink.text.fade(alpha), Align::Left),
    );
    let date_top = ct + time_size * 1.2;
    f.text(
        s.date,
        cl,
        date_top,
        cr,
        date_top + 20.0 * k,
        label(14.0 * k, false, ink.dim.fade(alpha), Align::Left),
    );

    // Day progress along the bottom.
    let text = format!("Day {:.0}%", s.day_progress * 100.0);
    let bar_y = cb - 8.0 * k;
    let label_w = 62.0 * k;
    f.text(
        &text,
        cr - label_w,
        bar_y - 10.0 * k,
        cr,
        bar_y + 10.0 * k,
        label(12.0 * k, false, ink.dim.fade(alpha), Align::Right),
    );
    let look = if s.theme.touch == Some(Touch::Hearts) {
        BarLook::Hearts
    } else {
        BarLook::Style(ProgressStyle::Bar)
    };
    progress(
        f,
        look,
        cl,
        cr - label_w - 8.0 * k,
        bar_y,
        s.day_progress,
        ink,
        alpha,
        k,
        0.0,
    );
}

enum BarLook {
    Style(ProgressStyle),
    Hearts,
}

/// Draw a progress bar from `left` to `right`, centred on `y`. `phase` animates the wave.
#[allow(clippy::too_many_arguments)]
fn progress(
    f: &mut Frame<'_>,
    look: BarLook,
    left: f32,
    right: f32,
    y: f32,
    fraction: f32,
    ink: Ink,
    alpha: f32,
    k: f32,
    phase: f32,
) {
    let width = right - left;
    if width <= 4.0 {
        return;
    }
    let fraction = fraction.clamp(0.0, 1.0);
    let head = left + width * fraction;
    let fg = ink.accent.fade(alpha);
    let track = ink.text.with_alpha(0.2).fade(alpha);
    let bar = |f: &mut Frame<'_>, l: f32, r: f32, half: f32, c: Rgba| {
        f.fill_pill(
            &Pill {
                left: l,
                top: y - half,
                right: r.max(l + 2.0 * half),
                bottom: y + half,
                radius: half,
            },
            c,
        );
    };
    match look {
        BarLook::Style(ProgressStyle::Line) => {
            bar(f, left, right, 1.0 * k, track);
            bar(f, left, head, 1.0 * k, fg);
            f.fill_ellipse(head, y, 4.0 * k, 4.0 * k, fg);
        }
        BarLook::Style(ProgressStyle::Bar) => {
            bar(f, left, right, 3.0 * k, track);
            bar(f, left, head, 3.0 * k, fg);
        }
        BarLook::Style(ProgressStyle::Dots) => {
            let count = (width / (7.0 * k)).floor().max(2.0) as usize;
            let step = width / (count - 1) as f32;
            for i in 0..count {
                let x = left + step * i as f32;
                let done = x <= head;
                let near_head = (x - head).abs() < step / 2.0;
                let radius = if near_head { 3.2 * k } else { 1.7 * k };
                f.fill_ellipse(
                    x,
                    y,
                    radius,
                    radius,
                    if done || near_head { fg } else { track },
                );
            }
        }
        BarLook::Style(ProgressStyle::Wave) => {
            // The played part ripples; the rest is a plain line.
            bar(f, head, right, 1.0 * k, track);
            let (amplitude, wavelength) = (3.0 * k, 16.0 * k);
            let step = (2.0 * k).max(1.0);
            let wave = |x: f32| {
                // Ease the ripple out near the head so it meets the knob smoothly.
                let fade = ((head - x) / (10.0 * k)).clamp(0.0, 1.0);
                y + amplitude
                    * fade
                    * ((x - left) / wavelength * std::f32::consts::TAU - phase * 4.0).sin()
            };
            let mut x = left;
            while x < head {
                let next = (x + step).min(head);
                f.line(x, wave(x), next, wave(next), fg, 2.0 * k);
                x = next;
            }
            f.fill_ellipse(head, y, 4.0 * k, 4.0 * k, fg);
        }
        BarLook::Style(ProgressStyle::Segments) => {
            let count = (width / (12.0 * k)).floor().clamp(4.0, 40.0) as usize;
            let gap = 2.5 * k;
            let seg = (width - gap * (count - 1) as f32) / count as f32;
            for i in 0..count {
                let l = left + (seg + gap) * i as f32;
                let done = (i as f32 + 0.5) / count as f32 <= fraction;
                bar(f, l, l + seg, 2.5 * k, if done { fg } else { track });
            }
        }
        BarLook::Hearts => {
            const COUNT: usize = 12;
            let step = width / COUNT as f32;
            let filled = (fraction * COUNT as f32).round() as usize;
            for i in 0..COUNT {
                let cx = left + step * (i as f32 + 0.5);
                let color = if i < filled {
                    fg
                } else {
                    ink.text.with_alpha(0.22).fade(alpha)
                };
                let style = label((step * 0.9).min(18.0 * k), false, color, Align::Center);
                f.text("♥", cx - step, y - 12.0 * k, cx + step, y + 12.0 * k, style);
            }
        }
    }
}

/// The compact Music tab (no lyrics column): cover, title and controls on one row, the progress
/// bar below.
fn paint_music_compact(
    f: &mut Frame<'_>,
    s: &Scene<'_>,
    m: &MusicView<'_>,
    (cl, ct, cr, cb): Rect,
    alpha: f32,
    ink: Ink,
    hits: &mut Vec<Hit>,
) {
    let k = s.scale;

    // Top block: cover, title and artist, transport buttons.
    let top_h = (cb - 24.0 * k) - ct;
    let side = top_h.min(62.0 * k).max(8.0);
    let cy = ct + top_h / 2.0;
    let art = Pill {
        left: cl,
        top: cy - side / 2.0,
        right: cl + side,
        bottom: cy + side / 2.0,
        radius: side * 0.16,
    };
    if !(m.has_art && f.fill_pill_image(ImageSlot::Art, &art, alpha)) {
        f.fill_pill(&art, ink.text.with_alpha(0.12 * alpha));
        f.text(
            "\u{E8D6}",
            art.left,
            art.top,
            art.right,
            art.bottom,
            icon(side * 0.42, ink.dim.fade(alpha)),
        );
    }

    let button = 30.0 * k;
    let controls_left = cr - 3.0 * button;
    let buttons = [
        ("\u{E892}", Action::Previous, 15.0, m.can_previous),
        (
            if m.playing { "\u{E769}" } else { "\u{E768}" },
            Action::PlayPause,
            19.0,
            true,
        ),
        ("\u{E893}", Action::Next, 15.0, m.can_next),
    ];
    for (i, (glyph, action, size, enabled)) in buttons.into_iter().enumerate() {
        let x = controls_left + button * i as f32;
        let rect = (x, cy - button / 2.0, x + button, cy + button / 2.0);
        let color = if enabled {
            ink.text
        } else {
            ink.text.with_alpha(0.28)
        };
        f.text(
            glyph,
            rect.0,
            rect.1,
            rect.2,
            rect.3,
            icon(size * k, color.fade(alpha)),
        );
        if enabled {
            hits.push(Hit { rect, action });
        }
    }

    let (text_left, text_right) = (art.right + 12.0 * k, controls_left - 6.0 * k);
    f.text(
        m.title,
        text_left,
        cy - 21.0 * k,
        text_right,
        cy + 1.0 * k,
        label(15.5 * k, true, ink.text.fade(alpha), Align::Left),
    );
    f.text(
        m.artist,
        text_left,
        cy + 1.0 * k,
        text_right,
        cy + 20.0 * k,
        label(13.0 * k, false, ink.dim.fade(alpha), Align::Left),
    );

    // Bottom row: elapsed, bar, total.
    let bar_y = cb - 8.0 * k;
    let time_w = 38.0 * k;
    let small = |align| label(11.5 * k, false, ink.dim.fade(alpha), align);
    let known = m.duration > 0.0;
    let fraction = match m.scrub {
        Some(scrub) => scrub,
        None if known => m.position / m.duration,
        None => 0.0,
    };
    let shown = if known {
        fraction * m.duration
    } else {
        m.position
    };
    f.text(
        &format_clock(shown),
        cl,
        bar_y - 9.0 * k,
        cl + time_w,
        bar_y + 9.0 * k,
        small(Align::Left),
    );
    let total = if known {
        format_clock(m.duration)
    } else {
        "--:--".to_string()
    };
    f.text(
        &total,
        cr - time_w,
        bar_y - 9.0 * k,
        cr,
        bar_y + 9.0 * k,
        small(Align::Right),
    );
    let (bar_l, bar_r) = (cl + time_w + 4.0 * k, cr - time_w - 4.0 * k);
    let look = if s.theme.touch == Some(Touch::Hearts) {
        BarLook::Hearts
    } else {
        BarLook::Style(s.music_settings.progress_style)
    };
    let phase = if m.playing { s.clock } else { 0.0 };
    progress(f, look, bar_l, bar_r, bar_y, fraction, ink, alpha, k, phase);
    if known && m.can_seek && bar_r > bar_l {
        hits.push(Hit {
            rect: (
                bar_l - 6.0 * k,
                bar_y - 11.0 * k,
                bar_r + 6.0 * k,
                bar_y + 11.0 * k,
            ),
            action: Action::SeekBar {
                left: bar_l,
                right: bar_r,
            },
        });
    }
}

/// The Music tab when nothing is playing: a faded sketch of the player with a made-up song, and a
/// line under it that asks for some music. Nothing in it can be clicked.
fn paint_music_example(
    f: &mut Frame<'_>,
    s: &Scene<'_>,
    (cl, ct, cr, cb): Rect,
    alpha: f32,
    ink: Ink,
) {
    let k = s.scale;
    let grey = Rgba::rgb(0x808080);
    let lyrics = LyricsState::Off;
    let sample = MusicView {
        title: "Song title",
        artist: "Artist",
        playing: false,
        position: 72.0,
        duration: 214.0,
        has_art: false,
        palette: Palette {
            dominant: grey,
            accent: grey,
            colors: [grey; 3],
        },
        bands: [0.0; BANDS],
        lyrics: &lyrics,
        scrub: None,
        can_seek: false,
        can_next: true,
        can_previous: true,
        glow: None,
        lyrics_view: None,
    };
    let mut unused = Vec::new();
    let sketch = (cl, ct, cr, cb - 22.0 * k);
    paint_music_compact(f, s, &sample, sketch, alpha * 0.4, ink, &mut unused);
    f.text(
        "Play something to see it here",
        cl,
        cb - 20.0 * k,
        cr,
        cb,
        label(13.0 * k, false, ink.dim.fade(alpha), Align::Center),
    );
}

/// The Music tab. With the lyrics setting on the card is wide: the player on the left, the
/// words on the right (a bigger card, `Placement::lyrics`). Without it, or on a side edge, the
/// compact player fills the card.
fn paint_music(
    f: &mut Frame<'_>,
    s: &Scene<'_>,
    (cl, ct, cr, cb): Rect,
    alpha: f32,
    ink: Ink,
    hits: &mut Vec<Hit>,
) {
    let k = s.scale;
    let Some(m) = &s.music else {
        return paint_music_example(f, s, (cl, ct, cr, cb), alpha, ink);
    };
    if !s.music_wide {
        return paint_music_compact(f, s, m, (cl, ct, cr, cb), alpha, ink, hits);
    }
    let left_w = ((cr - cl) * 0.43).round();
    paint_player_column(f, s, m, (cl, ct, cl + left_w, cb), alpha, ink, hits);
    paint_lyrics_column(
        f,
        s,
        m,
        (cl + left_w + 22.0 * k, ct, cr, cb),
        alpha,
        ink,
        hits,
    );
}

/// The player in the left column: cover, title and artist, the progress bar with the elapsed
/// and the remaining time, and the buttons in a row below.
fn paint_player_column(
    f: &mut Frame<'_>,
    s: &Scene<'_>,
    m: &MusicView<'_>,
    (cl, ct, cr, cb): Rect,
    alpha: f32,
    ink: Ink,
    hits: &mut Vec<Hit>,
) {
    let k = s.scale;
    // Cover, with the title and the artist beside it.
    let side = 62.0 * k;
    let art = Pill {
        left: cl,
        top: ct,
        right: cl + side,
        bottom: ct + side,
        radius: side * 0.16,
    };
    if !(m.has_art && f.fill_pill_image(ImageSlot::Art, &art, alpha)) {
        f.fill_pill(&art, ink.text.with_alpha(0.12 * alpha));
        f.text(
            "\u{E8D6}",
            art.left,
            art.top,
            art.right,
            art.bottom,
            icon(side * 0.42, ink.dim.fade(alpha)),
        );
    }
    let (text_left, text_right) = (art.right + 12.0 * k, cr);
    let mid = ct + side / 2.0;
    f.text(
        m.title,
        text_left,
        mid - 22.0 * k,
        text_right,
        mid,
        label(15.5 * k, true, ink.text.fade(alpha), Align::Left),
    );
    f.text(
        m.artist,
        text_left,
        mid + 1.0 * k,
        text_right,
        mid + 22.0 * k,
        label(13.0 * k, false, ink.dim.fade(alpha), Align::Left),
    );

    // Progress: elapsed on the left, the time still to go on the right.
    let bar_y = ct + side + 24.0 * k;
    let time_w = 36.0 * k;
    let small = |align| label(11.5 * k, false, ink.dim.fade(alpha), align);
    let known = m.duration > 0.0;
    let fraction = match m.scrub {
        Some(scrub) => scrub,
        None if known => m.position / m.duration,
        None => 0.0,
    };
    let shown = if known {
        fraction * m.duration
    } else {
        m.position
    };
    f.text(
        &format_clock(shown),
        cl,
        bar_y + 8.0 * k,
        cl + time_w,
        bar_y + 26.0 * k,
        small(Align::Left),
    );
    let remaining = if known {
        format!("\u{2212}{}", format_clock((m.duration - shown).max(0.0)))
    } else {
        "--:--".to_string()
    };
    f.text(
        &remaining,
        cr - time_w,
        bar_y + 8.0 * k,
        cr,
        bar_y + 26.0 * k,
        small(Align::Right),
    );
    let (bar_l, bar_r) = (cl, cr);
    let look = if s.theme.touch == Some(Touch::Hearts) {
        BarLook::Hearts
    } else {
        BarLook::Style(s.music_settings.progress_style)
    };
    let phase = if m.playing { s.clock } else { 0.0 };
    progress(f, look, bar_l, bar_r, bar_y, fraction, ink, alpha, k, phase);
    if known && m.can_seek && bar_r > bar_l {
        hits.push(Hit {
            rect: (
                bar_l - 6.0 * k,
                bar_y - 11.0 * k,
                bar_r + 6.0 * k,
                bar_y + 11.0 * k,
            ),
            action: Action::SeekBar {
                left: bar_l,
                right: bar_r,
            },
        });
    }

    // Buttons: previous, a round play / pause, next, centred under the bar.
    let cy = (bar_y + 26.0 * k + cb) / 2.0;
    let cx = (cl + cr) / 2.0;
    let radius = 20.0 * k;
    let dark = Rgba::rgb(0x101010);
    let ring = |c: f32| (c - 17.0 * k, cy - 17.0 * k, c + 17.0 * k, cy + 17.0 * k);
    let side_button = |f: &mut Frame<'_>, x: f32, glyph: &str, action: Action, enabled: bool| {
        let color = if enabled {
            ink.text
        } else {
            ink.text.with_alpha(0.28)
        };
        let rect = ring(x);
        f.text(
            glyph,
            rect.0,
            rect.1,
            rect.2,
            rect.3,
            icon(16.0 * k, color.fade(alpha)),
        );
        enabled.then_some(Hit { rect, action })
    };
    if let Some(hit) = side_button(
        f,
        cx - 52.0 * k,
        "\u{E892}",
        Action::Previous,
        m.can_previous,
    ) {
        hits.push(hit);
    }
    if let Some(hit) = side_button(f, cx + 52.0 * k, "\u{E893}", Action::Next, m.can_next) {
        hits.push(hit);
    }
    f.fill_ellipse(cx, cy, radius, radius, ink.text.fade(alpha));
    f.text(
        if m.playing { "\u{E769}" } else { "\u{E768}" },
        cx - radius,
        cy - radius,
        cx + radius,
        cy + radius,
        icon(17.0 * k, dark.fade(alpha)),
    );
    hits.push(Hit {
        rect: (cx - radius, cy - radius, cx + radius, cy + radius),
        action: Action::PlayPause,
    });
}

/// The Music tab's background glow: the cover's colours spread across the card, each part
/// brighter with the bass, middle or treble of the music at that place, and all of it flashing
/// when a new lyric line starts. The colours are the pill's (darkened enough for white text).
fn paint_glow(f: &mut Frame<'_>, s: &Scene<'_>, glow: [f32; GLOW_STOPS], pulse: f32, alpha: f32) {
    let Some(m) = &s.music else {
        return;
    };
    let colors = m.palette.colors.map(pillbox_core::music::pill_tint);
    let stops = pillbox_core::music::pill_stops(&colors, s.clock / 24.0);
    let body: Vec<(f32, Rgba)> = stops
        .iter()
        .zip(glow)
        .map(|((x, c), g)| {
            let strength = (0.14 + 0.5 * g + 0.22 * pulse).min(0.88);
            (*x, c.with_alpha(strength * alpha))
        })
        .collect();
    f.fill_pill_hgradient(&s.pill, &body);
    // A little shade at the bottom keeps the controls readable.
    f.fill_pill_gradient(
        &s.pill,
        Rgba::new(0.0, 0.0, 0.0, 0.0),
        Rgba::new(0.0, 0.0, 0.0, 0.3 * alpha),
    );
}

/// Split `text` into lines no wider than `max_w` at this text size (bold).
fn wrap_text(f: &mut Frame<'_>, text: &str, size: f32, max_w: f32) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        let trial = if current.is_empty() {
            word.to_string()
        } else {
            format!("{current} {word}")
        };
        if current.is_empty() || f.measure(&trial, size, true) <= max_w {
            current = trial;
        } else {
            lines.push(std::mem::take(&mut current));
            current = word.to_string();
        }
    }
    if !current.is_empty() || lines.is_empty() {
        lines.push(current);
    }
    lines
}

/// The words, on the right. The line being sung is white and the others grey; long lines wrap;
/// a pause in the singing is three dots that fill as the pause runs out. The wheel over the
/// column moves the words, and "Return to live" brings them back to the line being sung.
fn paint_lyrics_column(
    f: &mut Frame<'_>,
    s: &Scene<'_>,
    m: &MusicView<'_>,
    (l, t, r, b): Rect,
    alpha: f32,
    ink: Ink,
    hits: &mut Vec<Hit>,
) {
    let k = s.scale;
    let note = |f: &mut Frame<'_>, text: &str| {
        f.text(
            text,
            l,
            t,
            r,
            b,
            label(14.0 * k, false, ink.dim.fade(alpha), Align::Left),
        );
    };
    let lyrics = match m.lyrics {
        LyricsState::Found(lyrics) => lyrics,
        LyricsState::Loading => return note(f, "Looking for lyrics\u{2026}"),
        LyricsState::NotFound => return note(f, "No synced lyrics for this song"),
        LyricsState::SkippedVideo => return note(f, "Lyrics are skipped for videos"),
        LyricsState::Off => return note(f, "Lyrics are off"),
    };
    let last = lyrics.lines.len().saturating_sub(1);
    // The wheel over the column scrolls the words; the lines and the button below sit on top.
    hits.push(Hit {
        rect: (l - 6.0 * k, t - 6.0 * k, r + 6.0 * k, b + 6.0 * k),
        action: Action::LyricsArea,
    });

    let size = 17.0 * k;
    let line_h = size * 1.3;
    let gap = 11.0 * k;
    let live = lyrics.smooth_index(m.position, 0.35).max(0.0);
    let at = m.lyrics_view.unwrap_or(live).clamp(0.0, last as f32);
    // Wrap the lines around the one in view (the rest is out of sight).
    let base = at.floor() as usize;
    let (first, end) = (base.saturating_sub(7), (base + 9).min(last));
    let wrapped: Vec<Vec<String>> = (first..=end)
        .map(|i| {
            let text = lyrics.lines.get(i).map_or("", |line| line.text.as_str());
            if text.is_empty() {
                Vec::new()
            } else {
                wrap_text(f, text, size, r - l)
            }
        })
        .collect();
    let height_of = |i: usize| {
        let rows = i
            .checked_sub(first)
            .and_then(|j| wrapped.get(j))
            .map_or(1, |w| w.len().max(1));
        rows as f32 * line_h + gap
    };
    // Where the line being viewed starts, a little above the middle.
    let anchor = t + (b - t) * 0.36;

    f.push_clip(l - 4.0 * k, t - 6.0 * k, r + 4.0 * k, b + 4.0 * k);
    for i in first..=end {
        let top = anchor + pillbox_core::lrc::line_offset(at, i, height_of);
        let h = height_of(i) - gap;
        let centre = top + h / 2.0;
        // Fade towards the top and the bottom of the column.
        let room = (centre - t).min(b - centre);
        let fade = (room / (line_h * 1.1)).clamp(0.0, 1.0);
        if fade <= 0.0 || top + h < t - line_h || top > b + line_h {
            continue;
        }
        let near = (1.0 - (i as f32 - at).abs()).clamp(0.0, 1.0);
        let strength = (0.34 + 0.66 * near) * fade * alpha;
        let color = ink.text.fade(strength);
        let rows = &wrapped[i - first];
        if rows.is_empty() {
            // A pause: three dots, filling one by one while it is the line being sung.
            let progress = lyrics.gap_progress(i, m.position).unwrap_or(0.0);
            let singing = i == lyrics.index_at(m.position).unwrap_or(usize::MAX);
            for j in 0..3 {
                let lit = if singing {
                    (progress * 3.0 - j as f32).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let c = ink.text.fade((0.28 + 0.72 * lit) * fade * alpha);
                let rad = (3.2 + 0.9 * lit) * k;
                f.fill_ellipse(l + 5.0 * k + j as f32 * 13.0 * k, centre, rad, rad, c);
            }
        } else {
            for (j, row) in rows.iter().enumerate() {
                let y = top + j as f32 * line_h;
                f.text(
                    row,
                    l,
                    y,
                    r,
                    y + line_h,
                    label(size, true, color, Align::Left),
                );
            }
        }
        if fade > 0.5
            && m.can_seek
            && let Some(line) = lyrics.lines.get(i)
        {
            hits.push(Hit {
                rect: (l, top, r, top + h),
                action: Action::SeekTo(line.time),
            });
        }
    }
    f.pop_clip();

    // After a scroll by hand: a button that goes back to the line being sung.
    if m.lyrics_view.is_some() && (at - live).abs() > 0.6 {
        let text = "Return to live";
        let w = f.measure(text, 12.5 * k, true) + 44.0 * k;
        let h = 28.0 * k;
        let (bl, bb) = (l, b + 2.0 * k);
        let rect = (bl, bb - h, bl + w, bb);
        let pill = Pill {
            left: rect.0,
            top: rect.1,
            right: rect.2,
            bottom: rect.3,
            radius: h / 2.0,
        };
        f.fill_pill(&pill, Rgba::rgb(0xFFFFFF).with_alpha(0.94 * alpha));
        let dark = Rgba::rgb(0x111111).fade(alpha);
        // The arrow points to where the live line is: down when the view is above it.
        let arrow = if at < live { "\u{E74B}" } else { "\u{E74A}" };
        f.text(
            arrow,
            rect.0 + 8.0 * k,
            rect.1,
            rect.0 + 8.0 * k + 22.0 * k,
            rect.3,
            icon(11.0 * k, dark),
        );
        f.text(
            text,
            rect.0 + 28.0 * k,
            rect.1,
            rect.2 - 8.0 * k,
            rect.3,
            label(12.5 * k, true, dark, Align::Left),
        );
        hits.push(Hit {
            rect,
            action: Action::LyricsLive,
        });
    }
}
