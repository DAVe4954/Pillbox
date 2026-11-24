//! The list tabs of the open card: clipboard, shelf, reminders, controls, and the Home
//! widget grid. Pure drawing from a snapshot of state, like `scene.rs`; returns clickable areas.

use std::path::{Path, PathBuf};

use pillbox_core::{
    calendar::Minutes,
    cliphistory::{self, ClipFilter, ClipHistory, ClipKind},
    links::{BtDevice, WifiNet},
    phone::Qr,
    reminders::{self, Reminders},
    script::Board,
    settings::{LaunchSlot, Snippet, WidgetKind},
    shape::Pill,
    textedit::TextEdit,
    theme::Rgba,
};

use crate::{
    platform::gfx::{Align, Frame, ImageSlot},
    scene::{Action, CallView, Field, Hit, Ink, LinkPanel, Rect, icon, label},
    widgets::{self, WidgetData},
};

mod activity;
pub use activity::{CLAUDE_AUMID, activity, can_new_chat};
mod controls;
pub use controls::controls;
mod notifications;
pub use notifications::{badge, notifications};

// Segoe Fluent Icons glyphs used here.
const G_SEARCH: &str = "\u{E721}";
const G_PIN: &str = "\u{E718}";
const G_UNPIN: &str = "\u{E77A}";
const G_DELETE: &str = "\u{E74D}";
const G_TEXT: &str = "\u{E8D2}";
const G_LINK: &str = "\u{E71B}";
const G_PICTURE: &str = "\u{EB9F}";
const G_FILES: &str = "\u{E8B7}";
const G_FILE: &str = "\u{E8A5}";
const G_SNIPPET: &str = "\u{E70B}";
const G_OCR: &str = "\u{E8E9}";
const G_ADD: &str = "\u{E710}";
const G_CHECK: &str = "\u{E73E}";
const G_SNOOZE: &str = "\u{E916}";
const G_BELL: &str = "\u{EA8F}";
const G_REPEAT: &str = "\u{E8EE}";
const G_SELECT_ALL: &str = "\u{E8B3}";
const G_COPY: &str = "\u{E8C8}";
const G_VOLUME: &str = "\u{E767}";
const G_MUTED: &str = "\u{E74F}";
const G_BRIGHTNESS: &str = "\u{E706}";
const G_WIFI: &str = "\u{E701}";
const G_BLUETOOTH: &str = "\u{E702}";
const G_MIC: &str = "\u{E720}";
const G_MIC_OFF: &str = "\u{EC54}";
const G_SOUND: &str = "\u{E7F5}";
const G_CAMERA: &str = "\u{E714}";
const G_CHAT: &str = "\u{E8BD}";
const G_CLOSE: &str = "\u{E711}";
const G_APP: &str = "\u{ECAA}";
const G_PHONE: &str = "\u{E8EA}";

const G_STOP: &str = "\u{E71A}";
const G_SYNC: &str = "\u{E895}";
const G_INFO: &str = "\u{E946}";

const RED: Rgba = Rgba {
    r: 1.0,
    g: 0.36,
    b: 0.36,
    a: 1.0,
};
const AMBER: Rgba = Rgba {
    r: 1.0,
    g: 0.72,
    b: 0.2,
    a: 1.0,
};
const GREEN: Rgba = Rgba {
    r: 0.36,
    g: 0.82,
    b: 0.5,
    a: 1.0,
};

/// All text boxes' contents.
#[derive(Debug, Default)]
pub struct Edits {
    pub clip_search: TextEdit,
    pub reminder_add: TextEdit,
    pub note: TextEdit,
    pub todo: TextEdit,
    pub calc: TextEdit,
}

impl Edits {
    pub fn get(&self, f: Field) -> &TextEdit {
        match f {
            Field::ClipSearch => &self.clip_search,
            Field::ReminderAdd => &self.reminder_add,
            Field::Note => &self.note,
            Field::Todo => &self.todo,
            Field::Calc => &self.calc,
        }
    }

    pub fn get_mut(&mut self, f: Field) -> &mut TextEdit {
        match f {
            Field::ClipSearch => &mut self.clip_search,
            Field::ReminderAdd => &mut self.reminder_add,
            Field::Note => &mut self.note,
            Field::Todo => &mut self.todo,
            Field::Calc => &mut self.calc,
        }
    }
}

pub struct ClipUi<'a> {
    pub history: &'a ClipHistory,
    pub filter: ClipFilter,
    pub snippets: &'a [Snippet],
    pub two_columns: bool,
    pub enabled: bool,
}

pub struct ShelfUi<'a> {
    pub files: &'a [PathBuf],
    pub selected: &'a [bool],
    /// Phone buttons are shown (Settings > Phone).
    pub phone_enabled: bool,
    /// A QR code is on screen: shown in place of the files.
    pub phone: Option<PhoneUi<'a>>,
    /// The right-click menu is open on this file.
    pub menu: Option<ShelfMenuUi>,
}

/// What the right-click menu on a shelf file offers.
#[derive(Debug, Clone, Copy)]
pub struct ShelfMenuUi {
    pub index: usize,
    /// The file came from the phone: offer Download.
    pub from_phone: bool,
}

/// The QR code panel (M6).
#[derive(Clone, Copy)]
pub struct PhoneUi<'a> {
    pub qr: &'a Qr,
    pub title: &'a str,
    pub detail: &'a str,
    pub status: &'a str,
    /// "192.168.1.20:51234", without the token.
    pub address: &'a str,
    /// Seconds until the code closes by itself.
    pub seconds_left: u64,
    /// Windows Firewall will drop the phone's connection: shown in red.
    pub warning: Option<&'a str>,
}

pub struct RemindersUi<'a> {
    pub list: &'a Reminders,
    pub ringing: &'a [u64],
    pub error: Option<&'a str>,
    pub snooze_minutes: u32,
}

pub struct ControlsUi<'a> {
    /// Speaker volume 0..1 and mute.
    pub volume: Option<(f32, bool)>,
    /// 0..1, `None` when no display accepts brightness changes.
    pub brightness: Option<f32>,
    pub wifi: Option<bool>,
    pub bluetooth: Option<bool>,
    pub mic_muted: Option<bool>,
    pub call: Option<CallView<'a>>,
    pub launcher: &'a [LaunchSlot],
    /// The camera mirror is on: what to show in place of the controls.
    pub mirror: Option<MirrorUi<'a>>,
    pub links: LinksUi<'a>,
}

/// The Wi-Fi and Bluetooth cards of the Controls tab and the lists under them.
#[derive(Debug, Clone, Copy)]
pub struct LinksUi<'a> {
    /// The list that is open.
    pub panel: Option<LinkPanel>,
    /// The PC has a Wi-Fi adapter.
    pub wifi_adapter: bool,
    pub wifi: &'a [WifiNet],
    pub bluetooth: &'a [BtDevice],
    /// The first reading of each list has arrived.
    pub wifi_loaded: bool,
    pub bluetooth_loaded: bool,
    /// The network or device being switched now.
    pub busy: Option<&'a str>,
    /// What went wrong with the last switch.
    pub note: Option<&'a str>,
}

impl LinksUi<'static> {
    /// Nothing known and nothing open (desktop widgets, which have no Controls tab).
    pub const EMPTY: Self = Self {
        panel: None,
        wifi_adapter: false,
        wifi: &[],
        bluetooth: &[],
        wifi_loaded: false,
        bluetooth_loaded: false,
        busy: None,
        note: None,
    };
}

#[derive(Debug, Clone, Copy)]
pub struct MirrorUi<'a> {
    /// A camera frame is uploaded to ImageSlot::Camera.
    pub live: bool,
    /// Shown while there is no picture (starting, no camera, access blocked).
    pub message: &'a str,
}

pub struct WidgetsUi<'a> {
    pub shown: &'a [WidgetKind],
    pub data: &'a WidgetData,
    /// The Play time card is expanded, and which day it shows (0 = today).
    pub play_open: bool,
    pub play_day: usize,
    /// The detailed Weather card is expanded to show the next days.
    pub weather_open: bool,
}

/// What the Activity tab (M7) draws from.
pub struct ActivityUi<'a> {
    pub board: &'a Board,
    /// Agent kinds the user hid in Settings.
    pub hidden: &'a [String],
    /// Milliseconds since 1970.
    pub now_ms: i64,
    /// The session whose timeline is open.
    pub expanded: Option<u64>,
    /// The session whose Stop button waits for a second click.
    pub confirm_stop: Option<u64>,
    /// A `claude.exe` runs (the desktop app, also in the tray, or Claude Code in a terminal).
    pub claude_running: bool,
    /// What the Claude card says while it has no usage numbers.
    pub usage_note: &'static str,
}

/// What the Notifications tab (M8) draws from.
pub struct NotesUi<'a> {
    pub store: &'a pillbox_core::notes::NoteStore,
    /// Milliseconds since 1970.
    pub now_ms: i64,
    /// Shown instead of the list when it is empty (access denied, still starting).
    pub message: Option<&'static str>,
}

/// Everything the list tabs draw from.
pub struct UiState<'a> {
    /// Pointer position over the window, for hover highlights.
    pub pointer: Option<(f32, f32)>,
    /// Where the left button is held down, for the pressed look.
    pub pressed: Option<(f32, f32)>,
    /// The text box that has the keyboard.
    pub focus: Option<Field>,
    pub edits: &'a Edits,
    /// Scroll offset of the current tab's list, pixels.
    pub scroll: f32,
    /// Seconds since 1970 (for "5 min ago").
    pub now_secs: i64,
    pub now_min: Minutes,
    pub clock_24h: bool,
    pub clip: ClipUi<'a>,
    pub shelf: ShelfUi<'a>,
    pub reminders: RemindersUi<'a>,
    pub controls: ControlsUi<'a>,
    pub widgets: WidgetsUi<'a>,
    pub activity: ActivityUi<'a>,
    pub notes: NotesUi<'a>,
}

/// Drawing context shared by the tab painters.
pub struct Ctx<'a> {
    pub k: f32,
    pub alpha: f32,
    pub ink: Ink,
    pub ui: &'a UiState<'a>,
}

impl Ctx<'_> {
    pub fn hovered(&self, (l, t, r, b): Rect) -> bool {
        self.ui
            .pointer
            .is_some_and(|(x, y)| x >= l && x < r && y >= t && y < b)
    }

    pub fn text(&self) -> Rgba {
        self.ink.text.fade(self.alpha)
    }

    pub fn dim(&self) -> Rgba {
        self.ink.dim.fade(self.alpha)
    }

    pub fn accent(&self) -> Rgba {
        self.ink.accent.fade(self.alpha)
    }

    /// Text and icons drawn on an accent-coloured fill: dark on a light accent (a white one
    /// would make white text vanish), white on a dark or vivid one.
    pub fn on_accent(&self) -> Rgba {
        self.ink.accent.readable_on().fade(self.alpha)
    }

    /// A faint fill for rows, chips and boxes.
    pub fn wash(&self, strength: f32) -> Rgba {
        self.ink.wash.with_alpha(strength * self.alpha)
    }
}

pub fn rounded((l, t, r, b): Rect, radius: f32) -> Pill {
    Pill {
        left: l,
        top: t,
        right: r,
        bottom: b,
        radius,
    }
}

/// Thumbnail key for a file on the shelf or in the launcher (a hash of its path).
pub fn file_thumb_key(path: &Path) -> u64 {
    let s = path.to_string_lossy().to_lowercase();
    cliphistory::fingerprint(ClipKind::Files, s.as_bytes()) | (1 << 62)
}

/// Thumbnail key for a clipboard picture.
pub fn clip_thumb_key(id: u64) -> u64 {
    id & !(3 << 62)
}

// ---- small building blocks -------------------------------------------------------------------

/// A one-line text box. Clicking it gives Pillbox the keyboard.
#[allow(clippy::too_many_arguments)]
pub fn text_field(
    f: &mut Frame<'_>,
    cx: &Ctx<'_>,
    rect: Rect,
    field: Field,
    placeholder: &str,
    lead: Option<&str>,
    hits: &mut Vec<Hit>,
) {
    let k = cx.k;
    let (l, t, r, b) = rect;
    let focused = cx.ui.focus == Some(field);
    let h = b - t;
    f.fill_pill(
        &rounded(rect, h / 2.0),
        cx.wash(if focused { 0.16 } else { 0.09 }),
    );
    if focused {
        f.stroke_pill(
            &rounded(rect, h / 2.0),
            cx.accent().with_alpha(0.8 * cx.alpha),
            k,
        );
    }
    let mut text_left = l + 10.0 * k;
    if let Some(g) = lead {
        f.text(g, l + 4.0 * k, t, l + 26.0 * k, b, icon(12.0 * k, cx.dim()));
        text_left = l + 26.0 * k;
    }
    let text_right = r - 10.0 * k;
    let edit = cx.ui.edits.get(field);
    let size = 13.0 * k;
    if edit.text.is_empty() {
        f.text(
            placeholder,
            text_left,
            t,
            text_right,
            b,
            label(
                size,
                false,
                cx.dim().with_alpha(0.75 * cx.alpha),
                Align::Left,
            ),
        );
    }
    // Scroll the text left so the caret stays visible.
    let caret_x = f.measure(edit.before_caret(), size, false);
    let room = (text_right - text_left).max(1.0);
    let shift = (caret_x - room + 2.0 * k).max(0.0);
    f.push_clip(text_left, t, text_right + 1.0, b);
    f.text(
        &edit.text,
        text_left - shift,
        t,
        text_left - shift + 100_000.0,
        b,
        label(size, false, cx.text(), Align::Left),
    );
    if focused {
        let x = text_left - shift + caret_x;
        f.fill_rect(x, t + h * 0.22, x + k.max(1.0), b - h * 0.22, cx.text());
    }
    f.pop_clip();
    hits.push(Hit {
        rect,
        action: Action::Focus(field),
    });
}

/// A round icon button.
#[allow(clippy::too_many_arguments)]
pub fn icon_button(
    f: &mut Frame<'_>,
    cx: &Ctx<'_>,
    center: (f32, f32),
    size: f32,
    glyph: &str,
    color: Rgba,
    action: Action,
    hits: &mut Vec<Hit>,
) {
    let half = size / 2.0;
    let rect = (
        center.0 - half,
        center.1 - half,
        center.0 + half,
        center.1 + half,
    );
    if cx.hovered(rect) {
        f.fill_pill(&rounded(rect, half), cx.wash(0.14));
    }
    f.text(
        glyph,
        rect.0,
        rect.1,
        rect.2,
        rect.3,
        icon(size * 0.46, color),
    );
    hits.push(Hit { rect, action });
}

/// A pill button with an icon and a label.
#[allow(clippy::too_many_arguments)]
pub fn button(
    f: &mut Frame<'_>,
    cx: &Ctx<'_>,
    rect: Rect,
    glyph: &str,
    text: &str,
    filled: bool,
    action: Action,
    hits: &mut Vec<Hit>,
) {
    let k = cx.k;
    let (l, t, r, b) = rect;
    let h = b - t;
    let bg = if filled {
        cx.accent()
    } else {
        cx.wash(if cx.hovered(rect) { 0.2 } else { 0.11 })
    };
    f.fill_pill(&rounded(rect, h / 2.0), bg);
    let fg = if filled { cx.on_accent() } else { cx.text() };
    let text_w = f.measure(text, 12.0 * k, true);
    let icon_w = if glyph.is_empty() { 0.0 } else { 18.0 * k };
    let start = l + ((r - l) - text_w - icon_w).max(0.0) / 2.0;
    if !glyph.is_empty() {
        f.text(glyph, start, t, start + 16.0 * k, b, icon(11.5 * k, fg));
    }
    f.text(
        text,
        start + icon_w,
        t,
        r,
        b,
        label(12.0 * k, true, fg, Align::Left),
    );
    hits.push(Hit { rect, action });
}

/// A horizontal slider; the hit spans its whole length so a click anywhere sets the value.
pub fn slider(
    f: &mut Frame<'_>,
    cx: &Ctx<'_>,
    (l, r): (f32, f32),
    y: f32,
    value: f32,
    action: Action,
    hits: &mut Vec<Hit>,
) {
    let k = cx.k;
    let v = value.clamp(0.0, 1.0);
    let half = 3.0 * k;
    f.fill_pill(&rounded((l, y - half, r, y + half), half), cx.wash(0.18));
    let head = l + (r - l) * v;
    f.fill_pill(
        &rounded((l, y - half, head.max(l + 2.0 * half), y + half), half),
        cx.accent(),
    );
    f.fill_ellipse(
        head,
        y,
        7.0 * k,
        7.0 * k,
        Rgba::rgb(0xFFFFFF).fade(cx.alpha),
    );
    hits.push(Hit {
        rect: (l - 8.0 * k, y - 12.0 * k, r + 8.0 * k, y + 12.0 * k),
        action,
    });
}

/// A scrolling area: rows are drawn shifted by the scroll offset and clipped; hits are kept
/// only where they are visible.
struct Scroller {
    top: f32,
    bottom: f32,
    offset: f32,
}

impl Scroller {
    fn new(top: f32, bottom: f32, content_h: f32, scroll: f32) -> (Self, f32) {
        let max = (content_h - (bottom - top)).max(0.0);
        (
            Self {
                top,
                bottom,
                offset: scroll.clamp(0.0, max),
            },
            max,
        )
    }

    fn y(&self, content_y: f32) -> f32 {
        self.top + content_y - self.offset
    }

    fn visible(&self, t: f32, b: f32) -> bool {
        b > self.top && t < self.bottom
    }

    /// Clip a hit rectangle to the visible area; `None` if nothing of it shows.
    fn clip_hit(&self, (l, t, r, b): Rect) -> Option<Rect> {
        let (t, b) = (t.max(self.top), b.min(self.bottom));
        (b > t).then_some((l, t, r, b))
    }

    /// Clip drawing to the list (until `pop_fade_clip`), and soften the edge on each side where
    /// more of it is scrolled out of sight: a hint that it scrolls, instead of rows cut off
    /// sharply.
    fn clip(&self, f: &mut Frame<'_>, left: f32, right: f32, max: f32, k: f32) {
        let more_above = self.offset > 0.5;
        let more_below = self.offset < max - 0.5;
        f.push_fade_clip(
            (left, self.top, right, self.bottom),
            if more_above { 14.0 * k } else { 0.0 },
            if more_below { 22.0 * k } else { 0.0 },
        );
    }
}

fn push_clipped(sc: &Scroller, hits: &mut Vec<Hit>, rect: Rect, action: Action) {
    if let Some(rect) = sc.clip_hit(rect) {
        hits.push(Hit { rect, action });
    }
}

/// A short grey note in the middle of an area.
fn note(f: &mut Frame<'_>, cx: &Ctx<'_>, rect: Rect, text: &str) {
    f.text_wrapped(
        text,
        rect.0 + 12.0 * cx.k,
        rect.1 + (rect.3 - rect.1) * 0.3,
        rect.2 - 12.0 * cx.k,
        rect.3,
        label(13.0 * cx.k, false, cx.dim(), Align::Center),
    );
}

// ---- clipboard -------------------------------------------------------------------------------

fn kind_glyph(kind: ClipKind) -> &'static str {
    match kind {
        ClipKind::Text => G_TEXT,
        ClipKind::Link => G_LINK,
        ClipKind::Picture => G_PICTURE,
        ClipKind::Files => G_FILES,
    }
}

pub fn clipboard(
    f: &mut Frame<'_>,
    cx: &Ctx<'_>,
    (cl, ct, cr, cb): Rect,
    hits: &mut Vec<Hit>,
) -> f32 {
    let k = cx.k;
    let c = &cx.ui.clip;
    if !c.enabled {
        note(
            f,
            cx,
            (cl, ct, cr, cb),
            "Clipboard history is off. Turn it on in Settings.",
        );
        return 0.0;
    }
    // Search box and "clear" button.
    let row_h = 28.0 * k;
    text_field(
        f,
        cx,
        (cl, ct, cr - 34.0 * k, ct + row_h),
        Field::ClipSearch,
        "Search clipboard",
        Some(G_SEARCH),
        hits,
    );
    icon_button(
        f,
        cx,
        (cr - 14.0 * k, ct + row_h / 2.0),
        28.0 * k,
        G_DELETE,
        cx.dim(),
        Action::ClipClear,
        hits,
    );
    // Sub-tab chips.
    let chip_top = ct + row_h + 7.0 * k;
    let chip_h = 22.0 * k;
    let mut x = cl;
    for filter in ClipFilter::ALL {
        let w = f.measure(filter.label(), 11.5 * k, true) + 16.0 * k;
        let rect = (x, chip_top, x + w, chip_top + chip_h);
        let active = filter == c.filter;
        if active {
            f.fill_pill(
                &rounded(rect, chip_h / 2.0),
                cx.accent().with_alpha(0.85 * cx.alpha),
            );
        } else if cx.hovered(rect) {
            f.fill_pill(&rounded(rect, chip_h / 2.0), cx.wash(0.12));
        }
        let color = if active { cx.on_accent() } else { cx.dim() };
        f.text(
            filter.label(),
            rect.0,
            rect.1,
            rect.2,
            rect.3,
            label(11.5 * k, true, color, Align::Center),
        );
        hits.push(Hit {
            rect,
            action: Action::ClipFilter(filter),
        });
        x += w + 2.0 * k;
    }

    let list_top = chip_top + chip_h + 8.0 * k;
    let area = (cl - 6.0 * k, list_top, cr + 6.0 * k, cb);
    if c.filter == ClipFilter::Snippets {
        return snippets(f, cx, area, hits);
    }
    let query = &cx.ui.edits.clip_search.text;
    let items = c.history.view(c.filter, query);
    if items.is_empty() {
        let text = if !query.is_empty() {
            "Nothing matches your search."
        } else if c.history.items.is_empty() {
            "Copy something and it shows up here. Pinned items stay until you unpin them."
        } else {
            "Nothing of this kind yet."
        };
        note(f, cx, area, text);
        return 0.0;
    }
    let cols = if c.two_columns { 2 } else { 1 };
    let row_h = if cols == 2 { 54.0 } else { 44.0 } * k;
    let gap = 4.0 * k;
    let rows = items.len().div_ceil(cols);
    let content_h = rows as f32 * (row_h + gap);
    let (sc, max) = Scroller::new(area.1, area.3, content_h, cx.ui.scroll);
    let col_w = (area.2 - area.0 - gap * (cols as f32 - 1.0)) / cols as f32;
    sc.clip(f, area.0, area.2, max, cx.k);
    for (i, item) in items.iter().enumerate() {
        let (row, col) = (i / cols, i % cols);
        let top = sc.y(row as f32 * (row_h + gap));
        if !sc.visible(top, top + row_h) {
            continue;
        }
        let l = area.0 + col as f32 * (col_w + gap);
        let rect = (l, top, l + col_w, top + row_h);
        clip_row(f, cx, &sc, rect, item, cols == 2, hits);
    }
    f.pop_fade_clip();
    max
}

#[allow(clippy::too_many_arguments)]
fn clip_row(
    f: &mut Frame<'_>,
    cx: &Ctx<'_>,
    sc: &Scroller,
    rect: Rect,
    item: &cliphistory::ClipItem,
    compact: bool,
    hits: &mut Vec<Hit>,
) {
    let k = cx.k;
    let (l, t, r, b) = rect;
    let hovered = cx.hovered(rect) && sc.clip_hit(rect).is_some();
    f.fill_pill(
        &rounded(rect, 10.0 * k),
        cx.wash(if hovered { 0.13 } else { 0.06 }),
    );
    // Whole row copies; the small buttons on top of it take precedence (pushed later, and
    // the app picks the last matching hit).
    push_clipped(sc, hits, rect, Action::ClipCopy(item.id));
    let side = (b - t) - 12.0 * k;
    let thumb = rounded(
        (l + 6.0 * k, t + 6.0 * k, l + 6.0 * k + side, b - 6.0 * k),
        7.0 * k,
    );
    let drawn = item.kind == ClipKind::Picture
        && f.fill_pill_image(ImageSlot::Thumb(clip_thumb_key(item.id)), &thumb, cx.alpha);
    if !drawn {
        f.fill_pill(&thumb, cx.wash(0.1));
        f.text(
            kind_glyph(item.kind),
            thumb.left,
            thumb.top,
            thumb.right,
            thumb.bottom,
            icon(side * 0.42, cx.dim()),
        );
    }
    let text_l = thumb.right + 9.0 * k;
    let buttons_w = if hovered {
        if item.kind == ClipKind::Picture {
            84.0
        } else {
            58.0
        }
    } else if item.pinned {
        20.0
    } else {
        4.0
    } * k;
    let text_r = r - buttons_w;
    let title = match item.kind {
        ClipKind::Picture => format!("Picture {}×{}", item.size.0, item.size.1),
        ClipKind::Files => {
            let n = item.text.lines().count();
            let first = item
                .text
                .lines()
                .next()
                .and_then(|p| Path::new(p).file_name())
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            if n > 1 {
                format!("{first} and {} more", n - 1)
            } else {
                first
            }
        }
        _ => cliphistory::preview(&item.text, 160),
    };
    let ago = cliphistory::ago(cx.ui.now_secs - item.at);
    let source = item.source.trim_end_matches(".exe");
    let sub = if source.is_empty() {
        ago
    } else {
        format!("{ago} · {source}")
    };
    if compact {
        f.text_wrapped(
            &title,
            text_l,
            t + 6.0 * k,
            text_r,
            b - 18.0 * k,
            label(12.5 * k, false, cx.text(), Align::Left),
        );
        f.text(
            &sub,
            text_l,
            b - 19.0 * k,
            r - 6.0 * k,
            b - 3.0 * k,
            label(10.5 * k, false, cx.dim(), Align::Left),
        );
    } else {
        let mid = (t + b) / 2.0;
        f.text(
            &title,
            text_l,
            t + 3.0 * k,
            text_r,
            mid + 2.0 * k,
            label(13.0 * k, false, cx.text(), Align::Left),
        );
        f.text(
            &sub,
            text_l,
            mid,
            text_r,
            b - 3.0 * k,
            label(10.5 * k, false, cx.dim(), Align::Left),
        );
    }
    let cy = (t + b) / 2.0;
    if hovered {
        let mut x = r - 16.0 * k;
        let size = 26.0 * k;
        icon_button(
            f,
            cx,
            (x, cy),
            size,
            G_DELETE,
            cx.dim(),
            Action::ClipDelete(item.id),
            hits,
        );
        x -= 26.0 * k;
        let (glyph, color) = if item.pinned {
            (G_UNPIN, cx.accent())
        } else {
            (G_PIN, cx.dim())
        };
        icon_button(
            f,
            cx,
            (x, cy),
            size,
            glyph,
            color,
            Action::ClipPin(item.id),
            hits,
        );
        if item.kind == ClipKind::Picture {
            x -= 26.0 * k;
            icon_button(
                f,
                cx,
                (x, cy),
                size,
                G_OCR,
                cx.dim(),
                Action::ClipOcr(item.id),
                hits,
            );
        }
    } else if item.pinned {
        f.text(
            G_PIN,
            r - 22.0 * k,
            t,
            r - 4.0 * k,
            b,
            icon(10.5 * k, cx.accent()),
        );
    }
}

fn snippets(f: &mut Frame<'_>, cx: &Ctx<'_>, area: Rect, hits: &mut Vec<Hit>) -> f32 {
    let k = cx.k;
    let list = cx.ui.clip.snippets;
    if list.is_empty() {
        note(
            f,
            cx,
            (area.0, area.1, area.2, area.3 - 40.0 * k),
            "No snippets yet. Add text you paste often in Settings → Clipboard.",
        );
        let w = 140.0 * k;
        let mid = (area.0 + area.2) / 2.0;
        button(
            f,
            cx,
            (
                mid - w / 2.0,
                area.3 - 40.0 * k,
                mid + w / 2.0,
                area.3 - 12.0 * k,
            ),
            "",
            "Open Settings",
            false,
            Action::OpenSettings,
            hits,
        );
        return 0.0;
    }
    let row_h = 40.0 * k;
    let gap = 4.0 * k;
    let (sc, max) = Scroller::new(
        area.1,
        area.3,
        list.len() as f32 * (row_h + gap),
        cx.ui.scroll,
    );
    sc.clip(f, area.0, area.2, max, cx.k);
    for (i, s) in list.iter().enumerate() {
        let top = sc.y(i as f32 * (row_h + gap));
        if !sc.visible(top, top + row_h) {
            continue;
        }
        let rect = (area.0, top, area.2, top + row_h);
        let hovered = cx.hovered(rect);
        f.fill_pill(
            &rounded(rect, 10.0 * k),
            cx.wash(if hovered { 0.13 } else { 0.06 }),
        );
        f.text(
            G_SNIPPET,
            rect.0 + 6.0 * k,
            top,
            rect.0 + 34.0 * k,
            top + row_h,
            icon(13.0 * k, cx.dim()),
        );
        let name = if s.name.is_empty() {
            "Snippet"
        } else {
            s.name.as_str()
        };
        let mid = top + row_h / 2.0;
        f.text(
            name,
            rect.0 + 38.0 * k,
            top + 2.0 * k,
            rect.2 - 8.0 * k,
            mid + 2.0 * k,
            label(12.5 * k, true, cx.text(), Align::Left),
        );
        f.text(
            &cliphistory::preview(&s.text, 120),
            rect.0 + 38.0 * k,
            mid,
            rect.2 - 8.0 * k,
            top + row_h - 2.0 * k,
            label(11.0 * k, false, cx.dim(), Align::Left),
        );
        push_clipped(&sc, hits, rect, Action::SnippetCopy(i));
    }
    f.pop_fade_clip();
    max
}

// ---- shelf -----------------------------------------------------------------------------------

pub fn shelf(f: &mut Frame<'_>, cx: &Ctx<'_>, (cl, ct, cr, cb): Rect, hits: &mut Vec<Hit>) -> f32 {
    let k = cx.k;
    let s = &cx.ui.shelf;
    if let Some(p) = s.phone {
        phone_panel(f, cx, (cl, ct, cr, cb), &p, hits);
        return 0.0;
    }
    let row_h = 28.0 * k;
    let selected = s.selected.iter().filter(|x| **x).count();
    let summary = match (s.files.len(), selected) {
        (0, _) => "Shelf".to_string(),
        (n, 0) => format!("{n} file{}", if n == 1 { "" } else { "s" }),
        (n, sel) => format!("{sel} of {n} selected"),
    };
    f.text(
        &summary,
        cl,
        ct,
        cr,
        ct + row_h,
        label(13.0 * k, true, cx.text(), Align::Left),
    );
    let mut right = cr;
    if s.phone_enabled {
        // Phone to PC: a code for sending photos, files and text here.
        let w = if s.files.is_empty() {
            150.0 * k
        } else {
            30.0 * k
        };
        let text = if s.files.is_empty() {
            "Receive from phone"
        } else {
            ""
        };
        button(
            f,
            cx,
            (right - w, ct + 2.0 * k, right, ct + row_h - 2.0 * k),
            G_PHONE,
            text,
            false,
            Action::PhoneReceive,
            hits,
        );
        right -= w + 6.0 * k;
    }
    if !s.files.is_empty() {
        let w = 74.0 * k;
        let remove_label = if selected > 0 { "Remove" } else { "Clear" };
        button(
            f,
            cx,
            (right - w, ct + 2.0 * k, right, ct + row_h - 2.0 * k),
            G_DELETE,
            remove_label,
            false,
            Action::ShelfRemove,
            hits,
        );
        right -= w + 6.0 * k;
        button(
            f,
            cx,
            (right - w, ct + 2.0 * k, right, ct + row_h - 2.0 * k),
            G_COPY,
            "Copy",
            false,
            Action::ShelfCopy,
            hits,
        );
        right -= w + 6.0 * k;
        button(
            f,
            cx,
            (right - 30.0 * k, ct + 2.0 * k, right, ct + row_h - 2.0 * k),
            G_SELECT_ALL,
            "",
            false,
            Action::ShelfSelectAll,
            hits,
        );
    }
    let area = (cl - 4.0 * k, ct + row_h + 8.0 * k, cr + 4.0 * k, cb);
    if s.files.is_empty() {
        note(
            f,
            cx,
            area,
            "Drop files on Pillbox to keep them here, then drag them out into any app. New screenshots land here too.",
        );
        return 0.0;
    }
    let cols = 4usize;
    let gap = 6.0 * k;
    let tile_w = (area.2 - area.0 - gap * (cols as f32 - 1.0)) / cols as f32;
    let tile_h = 82.0 * k;
    let rows = s.files.len().div_ceil(cols);
    let (sc, max) = Scroller::new(area.1, area.3, rows as f32 * (tile_h + gap), cx.ui.scroll);
    let mut menu_tile: Option<Rect> = None;
    sc.clip(f, area.0, area.2, max, cx.k);
    for (i, path) in s.files.iter().enumerate() {
        let (row, col) = (i / cols, i % cols);
        let top = sc.y(row as f32 * (tile_h + gap));
        if !sc.visible(top, top + tile_h) {
            continue;
        }
        let l = area.0 + col as f32 * (tile_w + gap);
        let rect = (l, top, l + tile_w, top + tile_h);
        if s.menu.is_some_and(|m| m.index == i) {
            menu_tile = Some(rect);
        }
        let sel = s.selected.get(i).copied().unwrap_or(false);
        let bg = if sel {
            cx.accent().with_alpha(0.45 * cx.alpha)
        } else {
            cx.wash(if cx.hovered(rect) { 0.13 } else { 0.06 })
        };
        f.fill_pill(&rounded(rect, 12.0 * k), bg);
        let side = 44.0 * k;
        let mid = (rect.0 + rect.2) / 2.0;
        let img = rounded(
            (
                mid - side / 2.0,
                top + 8.0 * k,
                mid + side / 2.0,
                top + 8.0 * k + side,
            ),
            6.0 * k,
        );
        if !f.fill_pill_image(ImageSlot::Thumb(file_thumb_key(path)), &img, cx.alpha) {
            f.text(
                G_FILE,
                img.left,
                img.top,
                img.right,
                img.bottom,
                icon(side * 0.5, cx.dim()),
            );
        }
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path.to_string_lossy().to_string());
        let color = if path.exists() {
            cx.text()
        } else {
            RED.fade(cx.alpha)
        };
        f.text(
            &name,
            rect.0 + 4.0 * k,
            top + tile_h - 26.0 * k,
            rect.2 - 4.0 * k,
            top + tile_h - 6.0 * k,
            label(11.0 * k, false, color, Align::Center),
        );
        push_clipped(&sc, hits, rect, Action::ShelfItem(i));
        // Send to phone: a small button in the tile's corner (files only, not folders).
        if s.phone_enabled && !path.is_dir() {
            let b = 24.0 * k;
            let corner = (
                rect.2 - b - 3.0 * k,
                top + 3.0 * k,
                rect.2 - 3.0 * k,
                top + 3.0 * k + b,
            );
            let hot = cx.hovered(corner);
            if hot || cx.hovered(rect) {
                f.fill_pill(
                    &rounded(corner, b / 2.0),
                    cx.wash(if hot { 0.22 } else { 0.10 }),
                );
                f.text(
                    G_PHONE,
                    corner.0,
                    corner.1,
                    corner.2,
                    corner.3,
                    icon(12.0 * k, cx.text()),
                );
            }
            push_clipped(&sc, hits, corner, Action::ShelfPhone(i));
        }
    }
    f.pop_fade_clip();
    if let (Some(m), Some(tile)) = (s.menu, menu_tile)
        && let Some(path) = s.files.get(m.index)
    {
        shelf_menu(f, cx, area, tile, path, m, s.phone_enabled, hits);
    }
    max
}

/// The right-click menu on a shelf file, drawn over the shelf beside the file's tile.
#[allow(clippy::too_many_arguments)]
fn shelf_menu(
    f: &mut Frame<'_>,
    cx: &Ctx<'_>,
    area: Rect,
    tile: Rect,
    path: &Path,
    m: ShelfMenuUi,
    phone_enabled: bool,
    hits: &mut Vec<Hit>,
) {
    use crate::scene::ShelfCmd as C;
    let k = cx.k;
    let exists = path.exists();
    let is_file = path.is_file();
    let mut lines: Vec<(&str, &str, C)> = Vec::new();
    if exists {
        lines.push(("\u{E8E5}", "Open", C::Open));
        if is_file {
            lines.push(("\u{E7AC}", "Open with…", C::OpenWith));
        }
        lines.push(("\u{E838}", "Show in folder", C::ShowInFolder));
        if is_file && m.from_phone {
            lines.push(("\u{E896}", "Download…", C::Download));
        }
        if is_file && phone_enabled {
            lines.push((G_PHONE, "Send to phone", C::SendToPhone));
        }
    }
    lines.push((G_DELETE, "Remove from shelf", C::Remove));
    // Any click outside the menu just closes it (pushed first, so the lines win).
    hits.push(Hit {
        rect: area,
        action: Action::ShelfMenuClose,
    });
    let row = 28.0 * k;
    let pad = 5.0 * k;
    let w = 168.0 * k;
    let h = row * lines.len() as f32 + pad * 2.0;
    // Beside the tile (right if it fits, else left), its top level with the tile's,
    // kept inside the shelf area.
    let x = if tile.2 + 6.0 * k + w <= area.2 {
        tile.2 + 6.0 * k
    } else {
        (tile.0 - 6.0 * k - w).max(area.0)
    };
    let y = tile.1.min(area.3 - h).max(area.1);
    let panel = (x, y, x + w, y + h);
    // An opaque panel in the theme's light-or-dark sense, so the tiles below don't show through.
    let t = cx.ink.text;
    let light_text = t.r + t.g + t.b > 1.5;
    let bg = if light_text {
        Rgba::rgb(0x1F1F26)
    } else {
        Rgba::rgb(0xFAFAFC)
    };
    f.fill_pill(&rounded(panel, 12.0 * k), bg.fade(cx.alpha));
    f.stroke_pill(&rounded(panel, 12.0 * k), cx.wash(0.16), 1.0);
    for (n, (glyph, text, cmd)) in lines.into_iter().enumerate() {
        let top = y + pad + n as f32 * row;
        let r = (x + pad, top, x + w - pad, top + row);
        if cx.hovered(r) {
            f.fill_pill(&rounded(r, 8.0 * k), cx.wash(0.12));
        }
        let color = if cmd == C::Remove {
            RED.fade(cx.alpha)
        } else {
            cx.text()
        };
        f.text(
            glyph,
            r.0 + 4.0 * k,
            r.1,
            r.0 + 26.0 * k,
            r.3,
            icon(13.0 * k, color),
        );
        f.text(
            text,
            r.0 + 30.0 * k,
            r.1,
            r.2 - 6.0 * k,
            r.3,
            label(12.5 * k, false, color, Align::Left),
        );
        hits.push(Hit {
            rect: r,
            action: Action::ShelfMenu(m.index, cmd),
        });
    }
}

/// The QR code with what it does, its status and a Done button.
fn phone_panel(
    f: &mut Frame<'_>,
    cx: &Ctx<'_>,
    (cl, ct, cr, cb): Rect,
    p: &PhoneUi<'_>,
    hits: &mut Vec<Hit>,
) {
    let k = cx.k;
    let side = (cb - ct - 8.0 * k).min(200.0 * k).max(80.0);
    // Whole pixels per module and a quiet zone of three modules, all snapped to the pixel grid
    // so the code stays sharp (a blurry code scans badly).
    let quiet = 3usize;
    let cells = p.qr.width + 2 * quiet;
    let m = ((side / cells as f32).floor()).max(1.0);
    let box_side = m * cells as f32;
    let x0 = cl.round();
    let y0 = (ct + (cb - ct - box_side) / 2.0).round();
    f.fill_pill(
        &rounded((x0, y0, x0 + box_side, y0 + box_side), 10.0 * k),
        Rgba::rgb(0xFFFFFF).fade(cx.alpha),
    );
    let ink = Rgba::rgb(0x000000).fade(cx.alpha);
    for (row, col, len) in p.qr.runs() {
        let x = x0 + (col + quiet) as f32 * m;
        let y = y0 + (row + quiet) as f32 * m;
        f.fill_rect(x, y, x + len as f32 * m, y + m, ink);
    }
    // Text column to the right of the code.
    let tl = x0 + box_side + 16.0 * k;
    let mut y = y0 + 2.0 * k;
    f.text_wrapped(
        p.title,
        tl,
        y,
        cr,
        y + 44.0 * k,
        label(15.0 * k, true, cx.text(), Align::Left),
    );
    y += 44.0 * k;
    let bottom = y0 + box_side;
    if let Some(warning) = p.warning {
        // The firewall will drop the phone's connection: say so instead of "Waiting…".
        f.text_wrapped(
            warning,
            tl,
            y,
            cr,
            bottom - 58.0 * k,
            label(11.5 * k, true, RED.fade(cx.alpha), Align::Left),
        );
    } else {
        f.text_wrapped(
            p.detail,
            tl,
            y,
            cr,
            y + 52.0 * k,
            label(12.0 * k, false, cx.dim(), Align::Left),
        );
        y += 56.0 * k;
        f.text_wrapped(
            p.status,
            tl,
            y,
            cr,
            y + 34.0 * k,
            label(12.5 * k, true, cx.accent(), Align::Left),
        );
    }
    let mins = p.seconds_left.div_ceil(60);
    f.text(
        &format!("{} · {mins} min left", p.address),
        tl,
        bottom - 54.0 * k,
        cr,
        bottom - 36.0 * k,
        label(10.0 * k, false, cx.dim(), Align::Left),
    );
    button(
        f,
        cx,
        (tl, bottom - 28.0 * k, cr, bottom),
        G_CLOSE,
        "Done",
        false,
        Action::PhoneClose,
        hits,
    );
}

// ---- reminders -------------------------------------------------------------------------------

pub fn reminders(
    f: &mut Frame<'_>,
    cx: &Ctx<'_>,
    (cl, ct, cr, cb): Rect,
    hits: &mut Vec<Hit>,
) -> f32 {
    let k = cx.k;
    let r = &cx.ui.reminders;
    let row_h = 28.0 * k;
    text_field(
        f,
        cx,
        (cl, ct, cr - 34.0 * k, ct + row_h),
        Field::ReminderAdd,
        "Remind me… e.g. tea in 10m, call mum at 18:30",
        Some(G_BELL),
        hits,
    );
    icon_button(
        f,
        cx,
        (cr - 14.0 * k, ct + row_h / 2.0),
        28.0 * k,
        G_ADD,
        cx.accent(),
        Action::ReminderAdd,
        hits,
    );
    let mut top = ct + row_h + 6.0 * k;
    if let Some(err) = r.error {
        f.text(
            err,
            cl + 8.0 * k,
            top,
            cr,
            top + 18.0 * k,
            label(11.5 * k, false, RED.fade(cx.alpha), Align::Left),
        );
        top += 20.0 * k;
    } else {
        top += 2.0 * k;
    }
    let area = (cl - 6.0 * k, top, cr + 6.0 * k, cb);
    if r.list.items.is_empty() {
        note(
            f,
            cx,
            area,
            "No reminders. Type one above and press Enter. Repeats: “daily 8:00 …”, “weekdays 9:30 …”, “every friday 16:00 …”.",
        );
        return 0.0;
    }
    let row_h = 44.0 * k;
    let gap = 4.0 * k;
    let (sc, max) = Scroller::new(
        area.1,
        area.3,
        r.list.items.len() as f32 * (row_h + gap),
        cx.ui.scroll,
    );
    sc.clip(f, area.0, area.2, max, cx.k);
    for (i, item) in r.list.items.iter().enumerate() {
        let t = sc.y(i as f32 * (row_h + gap));
        if !sc.visible(t, t + row_h) {
            continue;
        }
        let rect = (area.0, t, area.2, t + row_h);
        let ringing = r.ringing.contains(&item.id);
        let hovered = cx.hovered(rect);
        let bg = if ringing {
            cx.accent().with_alpha(0.4 * cx.alpha)
        } else {
            cx.wash(if hovered { 0.12 } else { 0.06 })
        };
        f.fill_pill(&rounded(rect, 10.0 * k), bg);
        let glyph = if item.repeat == reminders::Repeat::Once {
            G_BELL
        } else {
            G_REPEAT
        };
        f.text(
            glyph,
            rect.0 + 6.0 * k,
            t,
            rect.0 + 32.0 * k,
            t + row_h,
            icon(13.0 * k, if ringing { cx.text() } else { cx.dim() }),
        );
        let mut when = reminders::describe(item.rings_at(), cx.ui.now_min, cx.ui.clock_24h);
        if item.snoozed.is_some() {
            when.push_str(" · snoozed");
        }
        if item.repeat != reminders::Repeat::Once {
            when = format!("{when} · {}", item.repeat.label());
        }
        let buttons_w = if ringing {
            150.0
        } else if hovered {
            60.0
        } else {
            6.0
        } * k;
        let mid = t + row_h / 2.0;
        f.text(
            &item.text,
            rect.0 + 36.0 * k,
            t + 3.0 * k,
            rect.2 - buttons_w,
            mid + 2.0 * k,
            label(13.0 * k, ringing, cx.text(), Align::Left),
        );
        f.text(
            &when,
            rect.0 + 36.0 * k,
            mid,
            rect.2 - buttons_w,
            t + row_h - 3.0 * k,
            label(10.5 * k, false, cx.dim(), Align::Left),
        );
        if ringing {
            let bw = 66.0 * k;
            let (bt, bb) = (t + 9.0 * k, t + row_h - 9.0 * k);
            let snooze = format!("{} min", r.snooze_minutes);
            let done_rect = (rect.2 - 8.0 * k - bw, bt, rect.2 - 8.0 * k, bb);
            let snooze_rect = (done_rect.0 - 6.0 * k - bw, bt, done_rect.0 - 6.0 * k, bb);
            if sc.clip_hit(rect).is_some() {
                button(
                    f,
                    cx,
                    snooze_rect,
                    G_SNOOZE,
                    &snooze,
                    false,
                    Action::ReminderSnooze(item.id),
                    hits,
                );
                button(
                    f,
                    cx,
                    done_rect,
                    G_CHECK,
                    "Done",
                    true,
                    Action::ReminderDone(item.id),
                    hits,
                );
            }
        } else if hovered {
            let size = 26.0 * k;
            icon_button(
                f,
                cx,
                (rect.2 - 16.0 * k, mid),
                size,
                G_DELETE,
                cx.dim(),
                Action::ReminderDelete(item.id),
                hits,
            );
            icon_button(
                f,
                cx,
                (rect.2 - 42.0 * k, mid),
                size,
                G_CHECK,
                cx.dim(),
                Action::ReminderDone(item.id),
                hits,
            );
        }
    }
    f.pop_fade_clip();
    max
}

// ---- controls --------------------------------------------------------------------------------

/// The camera mirror: the picture fills the content area, with a close button on top.
fn mirror(f: &mut Frame<'_>, cx: &Ctx<'_>, rect: Rect, m: MirrorUi<'_>, hits: &mut Vec<Hit>) {
    let k = cx.k;
    let (l, t, r, b) = rect;
    let frame = rounded(rect, 14.0 * k);
    if !(m.live && f.fill_pill_image(ImageSlot::Camera, &frame, cx.alpha)) {
        f.fill_pill(&frame, cx.wash(0.08));
        f.text(
            G_CAMERA,
            l,
            t,
            r,
            (t + b) / 2.0 + 4.0 * k,
            icon(22.0 * k, cx.dim()),
        );
        f.text(
            m.message,
            l + 12.0 * k,
            (t + b) / 2.0 + 6.0 * k,
            r - 12.0 * k,
            (t + b) / 2.0 + 30.0 * k,
            label(12.0 * k, false, cx.dim(), Align::Center),
        );
    }
    // Close button in the top-right corner, on a dark disc so it reads over any picture.
    let size = 28.0 * k;
    let (cxp, cyp) = (r - size / 2.0 - 6.0 * k, t + size / 2.0 + 6.0 * k);
    f.fill_ellipse(
        cxp,
        cyp,
        size / 2.0,
        size / 2.0,
        Rgba::new(0.0, 0.0, 0.0, 0.45 * cx.alpha),
    );
    f.text(
        G_CLOSE,
        cxp - size / 2.0,
        cyp - size / 2.0,
        cxp + size / 2.0,
        cyp + size / 2.0,
        icon(11.0 * k, Rgba::rgb(0xFFFFFF).fade(cx.alpha)),
    );
    hits.push(Hit {
        rect: (
            cxp - size / 2.0,
            cyp - size / 2.0,
            cxp + size / 2.0,
            cyp + size / 2.0,
        ),
        action: Action::Mirror,
    });
}

// ---- home: widgets ---------------------------------------------------------------------------

pub fn home(f: &mut Frame<'_>, cx: &Ctx<'_>, (cl, ct, cr, cb): Rect, hits: &mut Vec<Hit>) -> f32 {
    let k = cx.k;
    let w = &cx.ui.widgets;
    let area = (cl - 6.0 * k, ct - 2.0 * k, cr + 6.0 * k, cb);
    let cols = 2usize;
    let gap = 6.0 * k;
    let tile_w = (area.2 - area.0 - gap) / cols as f32;
    let tile_h = widgets::TILE_H * k;
    // Wide widgets take a whole row; a tile can be taller than the rest (Play time, expanded).
    let mut placed: Vec<(usize, Rect)> = Vec::new();
    let (mut col, mut y) = (0usize, 0.0f32);
    for (i, kind) in w.shown.iter().enumerate() {
        let wide = widgets::is_wide_on_home(*kind, w);
        let height = widgets::tile_height(*kind, w) * k;
        if wide && col != 0 {
            col = 0;
            y += tile_h + gap;
        }
        let l = area.0 + col as f32 * (tile_w + gap);
        let width = if wide { area.2 - area.0 } else { tile_w };
        placed.push((i, (l, y, l + width, y + height)));
        if wide || col + 1 == cols {
            col = 0;
            y += height + gap;
        } else {
            col += 1;
        }
    }
    let content_h = if col == 0 { y } else { y + tile_h + gap };
    let (sc, max) = Scroller::new(area.1, area.3, content_h, cx.ui.scroll);
    sc.clip(f, area.0, area.2, max, cx.k);
    for (i, (l, t, r, b)) in placed {
        let (t, b) = (sc.y(t), sc.y(b));
        if !sc.visible(t, b) {
            continue;
        }
        let rect = (l, t, r, b);
        f.fill_pill(&rounded(rect, 14.0 * k), cx.wash(0.07));
        let mut tile_hits = Vec::new();
        widgets::paint_tile(f, cx, w.shown[i], i, rect, &mut tile_hits);
        for h in tile_hits {
            push_clipped(&sc, hits, h.rect, h.action);
        }
    }
    f.pop_fade_clip();
    max
}
