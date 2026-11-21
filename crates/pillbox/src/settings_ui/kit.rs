//! The Settings window's look: colours, fonts, icons and the controls every page is built from
//! (a section card with rows, toggles, segmented buttons, sliders, picture cards, lists and the
//! shortcut recorder).

use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, FontFamily, FontId, Id, Layout, Margin, Pos2, Rect,
    Response, RichText, Sense, Stroke, StrokeKind, Ui, Vec2, containers::frame::Prepared, vec2,
};

// ---- colours ----------------------------------------------------------------------------------

pub const SIDEBAR: Color32 = Color32::from_rgb(0x17, 0x17, 0x19);
pub const CONTENT: Color32 = Color32::from_rgb(0x1D, 0x1D, 0x20);
pub const CARD: Color32 = Color32::from_rgb(0x25, 0x25, 0x29);
pub const CARD_EDGE: Color32 = Color32::from_rgb(0x2F, 0x2F, 0x34);
pub const TILE: Color32 = Color32::from_rgb(0x32, 0x32, 0x37);
pub const FIELD: Color32 = Color32::from_rgb(0x1A, 0x1A, 0x1D);
pub const HOVER: Color32 = Color32::from_rgb(0x2B, 0x2B, 0x30);
pub const TEXT: Color32 = Color32::from_rgb(0xF2, 0xF2, 0xF4);
pub const SUB: Color32 = Color32::from_rgb(0x9A, 0x9A, 0xA2);
pub const FAINT: Color32 = Color32::from_rgb(0x6A, 0x6A, 0x72);
pub const GOOD: Color32 = Color32::from_rgb(0x6F, 0xD8, 0x9A);
pub const BAD: Color32 = Color32::from_rgb(0xFF, 0x7A, 0x7A);

/// The accent colour (Pillbox's own, from the draft settings), set on the style each frame.
pub fn accent(ui: &Ui) -> Color32 {
    ui.visuals().selection.bg_fill
}

// ---- fonts and icons --------------------------------------------------------------------------

pub const SEMIBOLD: &str = "semibold";
pub const ICONS: &str = "icons";

pub fn semibold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(SEMIBOLD.into()))
}

pub fn regular(size: f32) -> FontId {
    FontId::proportional(size)
}

pub fn icon_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(ICONS.into()))
}

/// Segoe UI for text (regular and semibold) and Segoe Fluent Icons (or MDL2 Assets on Windows
/// 10) for icons, all read from the Windows fonts folder. Whatever is missing falls back to
/// egui's built-in fonts, so the window always has text.
pub fn install_fonts(ctx: &egui::Context) {
    let dir = std::path::PathBuf::from(
        std::env::var_os("WINDIR").unwrap_or_else(|| "C:\\Windows".into()),
    )
    .join("Fonts");
    let read = |name: &str| std::fs::read(dir.join(name)).ok();
    let mut fonts = egui::FontDefinitions::default();
    let builtin: Vec<String> = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    let mut add = |key: &str, file: &str| -> bool {
        match read(file) {
            Some(bytes) => {
                fonts
                    .font_data
                    .insert(key.into(), egui::FontData::from_owned(bytes).into());
                true
            }
            None => false,
        }
    };
    let regular = add("segoe", "segoeui.ttf");
    let semi = add("segoe-sb", "seguisb.ttf");
    let fluent = add("fluent", "SegoeIcons.ttf");
    let mdl2 = add("mdl2", "segmdl2.ttf");
    let emoji = add("emoji", "seguiemj.ttf");
    let mut proportional = Vec::new();
    if regular {
        proportional.push("segoe".to_string());
    }
    proportional.extend(builtin.iter().cloned());
    if emoji {
        proportional.push("emoji".to_string());
    }
    let mut semibold = Vec::new();
    if semi {
        semibold.push("segoe-sb".to_string());
    }
    semibold.extend(proportional.iter().cloned());
    let mut icons = Vec::new();
    if fluent {
        icons.push("fluent".to_string());
    }
    if mdl2 {
        icons.push("mdl2".to_string());
    }
    icons.extend(builtin);
    fonts
        .families
        .insert(FontFamily::Proportional, proportional);
    fonts
        .families
        .insert(FontFamily::Name(SEMIBOLD.into()), semibold);
    fonts.families.insert(FontFamily::Name(ICONS.into()), icons);
    ctx.set_fonts(fonts);
}

/// Segoe Fluent Icons code points (the same in MDL2 Assets).
pub mod icon {
    pub const SETTINGS: &str = "\u{E713}";
    pub const PALETTE: &str = "\u{E790}";
    pub const MUSIC: &str = "\u{EC4F}";
    pub const TILES: &str = "\u{ECA5}";
    pub const PASTE: &str = "\u{E77F}";
    pub const BELL: &str = "\u{EA8F}";
    pub const GAME: &str = "\u{E7FC}";
    pub const ROBOT: &str = "\u{E99A}";
    pub const KEYBOARD: &str = "\u{E765}";
    pub const INFO: &str = "\u{E946}";
    pub const SEARCH: &str = "\u{E721}";
    pub const CLOSE: &str = "\u{E8BB}";
    pub const MINIMIZE: &str = "\u{E921}";
    pub const MONITOR: &str = "\u{E7F4}";
    pub const FULLSCREEN: &str = "\u{E740}";
    pub const PIN: &str = "\u{E718}";
    pub const CLOCK: &str = "\u{E823}";
    pub const CALENDAR: &str = "\u{E787}";
    pub const POWER: &str = "\u{E7E8}";
    pub const DELETE: &str = "\u{E74D}";
    pub const ADD: &str = "\u{E710}";
    pub const CANCEL: &str = "\u{E711}";
    pub const UP: &str = "\u{E70E}";
    pub const DOWN: &str = "\u{E70D}";
    pub const FOLDER: &str = "\u{E8B7}";
    pub const CLOUD: &str = "\u{E753}";
    pub const SUN: &str = "\u{E706}";
    pub const LOCK: &str = "\u{E72E}";
    pub const EYE: &str = "\u{E7B3}";
    pub const HISTORY: &str = "\u{E81C}";
    pub const REFRESH: &str = "\u{E72C}";
    pub const EDIT: &str = "\u{E70F}";
    pub const MIC: &str = "\u{E720}";
    pub const CAMERA: &str = "\u{E714}";
    pub const HOME: &str = "\u{E80F}";
    pub const GLOBE: &str = "\u{E774}";
    pub const CHAT: &str = "\u{E8F2}";
    pub const CHECK: &str = "\u{E73E}";
    pub const WARNING: &str = "\u{E7BA}";
    pub const BOLT: &str = "\u{E945}";
    pub const LOCATION: &str = "\u{E707}";
    pub const EQUALIZER: &str = "\u{E9E9}";
    pub const PHOTO: &str = "\u{E91B}";
    pub const PHONE: &str = "\u{E8EA}";
    pub const TIMER: &str = "\u{E916}";
    pub const SPEED: &str = "\u{EC4A}";
    pub const TEXT: &str = "\u{E8D2}";
    pub const LIST: &str = "\u{E8FD}";
    pub const SHIELD: &str = "\u{EA18}";
    pub const DIAGNOSTIC: &str = "\u{E9D9}";
    pub const LAUNCH: &str = "\u{E8A7}";
    pub const RESET: &str = "\u{E777}";
    pub const STRIP: &str = "\u{E8C4}";
    pub const BROWSE: &str = "\u{E838}";
}

// ---- search ------------------------------------------------------------------------------------

/// True when every word of `query` (lower case) appears in one of `texts`. An empty query
/// matches everything.
pub fn matches(query: &str, texts: &[&str]) -> bool {
    if query.is_empty() {
        return true;
    }
    let all = texts.join(" ").to_lowercase();
    query.split_whitespace().all(|w| all.contains(w))
}

// ---- a section: a small heading and a card of rows -------------------------------------------

/// A titled card of setting rows. The heading and card appear only once a row is shown, so a
/// search that matches none of a section's rows hides the whole section.
pub struct Section<'u> {
    ui: &'u mut Ui,
    title: String,
    query: &'u str,
    card: Option<Prepared>,
    /// What the search also matches against for every row (the page and section names).
    context: String,
}

impl<'u> Section<'u> {
    pub fn new(ui: &'u mut Ui, title: &str, page: &str, query: &'u str) -> Self {
        // Search results mix pages, so there the heading says which page a section is on.
        let heading = if query.is_empty() || page == title {
            title.to_string()
        } else {
            format!("{page}  ·  {title}")
        };
        Self {
            ui,
            title: heading,
            query,
            card: None,
            context: format!("{page} {title}"),
        }
    }

    /// The card's inside, ready for the next row: the heading and card on the first row, a
    /// hairline above every later one.
    fn open_card(&mut self) -> &mut Ui {
        match self.card.take() {
            Some(card) => {
                let ui = &card.content_ui;
                let rect = ui.available_rect_before_wrap();
                let y = rect.top();
                ui.painter().line_segment(
                    [
                        Pos2::new(rect.left() + 16.0, y),
                        Pos2::new(rect.right() - 16.0, y),
                    ],
                    Stroke::new(1.0_f32, CARD_EDGE),
                );
                self.card = Some(card);
            }
            None => {
                self.ui.add_space(14.0);
                self.ui.horizontal(|ui| {
                    ui.add_space(4.0);
                    ui.label(RichText::new(&self.title).font(semibold(13.0)).color(SUB));
                });
                self.ui.add_space(6.0);
                let frame = egui::Frame::new()
                    .fill(CARD)
                    .stroke(Stroke::new(1.0_f32, CARD_EDGE))
                    .corner_radius(CornerRadius::same(12))
                    .inner_margin(Margin::same(0));
                let mut prepared = frame.begin(self.ui);
                prepared.content_ui.spacing_mut().item_spacing.y = 0.0;
                self.card = Some(prepared);
            }
        }
        match self.card {
            Some(ref mut card) => &mut card.content_ui,
            // Not reachable (filled just above); the outer ui is a harmless fallback.
            None => self.ui,
        }
    }

    fn shown(&self, title: &str, sub: &str, keys: &str) -> bool {
        matches(self.query, &[title, sub, keys, &self.context])
    }

    /// A row: icon tile, title and a grey line under it, the control on the right. The control
    /// is laid out right to left: of several widgets, the first one added is the rightmost.
    pub fn row(
        &mut self,
        glyph: &str,
        title: &str,
        sub: &str,
        keys: &str,
        control: impl FnOnce(&mut Ui),
    ) {
        if !self.shown(title, sub, keys) {
            return;
        }
        let ui = self.open_card();
        row_frame().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.set_min_height(38.0);
                icon_tile(ui, glyph);
                ui.add_space(4.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    control(ui);
                    ui.add_space(12.0);
                    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                        row_text(ui, title, sub);
                    });
                });
            });
        });
    }

    /// A row with room below it for a list, a grid or a form: `body` draws everything under the
    /// title, across the full width.
    pub fn block(
        &mut self,
        glyph: &str,
        title: &str,
        sub: &str,
        keys: &str,
        body: impl FnOnce(&mut Ui),
    ) {
        if !self.shown(title, sub, keys) {
            return;
        }
        let ui = self.open_card();
        row_frame().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.set_min_height(38.0);
                icon_tile(ui, glyph);
                ui.add_space(4.0);
                row_text(ui, title, sub);
            });
            ui.add_space(10.0);
            ui.spacing_mut().item_spacing.y = 8.0;
            body(ui);
        });
    }

    pub fn end(self) {
        if let Some(card) = self.card {
            card.end(self.ui);
        }
    }
}

fn row_frame() -> egui::Frame {
    egui::Frame::new().inner_margin(Margin::symmetric(16, 11))
}

fn icon_tile(ui: &mut Ui, glyph: &str) {
    let (rect, _) = ui.allocate_exact_size(vec2(36.0, 36.0), Sense::hover());
    ui.painter().rect_filled(rect, 9.0, TILE);
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        glyph,
        icon_font(16.0),
        TEXT,
    );
}

fn row_text(ui: &mut Ui, title: &str, sub: &str) {
    if sub.is_empty() {
        // A title alone sits in the middle of the row, level with the icon and the control.
        ui.add(egui::Label::new(RichText::new(title).font(semibold(14.0)).color(TEXT)).wrap());
        return;
    }
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        ui.add(egui::Label::new(RichText::new(title).font(semibold(14.0)).color(TEXT)).wrap());
        if !sub.is_empty() {
            ui.add(egui::Label::new(RichText::new(sub).font(regular(12.5)).color(SUB)).wrap());
        }
    });
}

/// A grey note under a block's content.
pub fn note(ui: &mut Ui, text: &str) {
    ui.add(egui::Label::new(RichText::new(text).font(regular(12.0)).color(FAINT)).wrap());
}

// ---- controls ---------------------------------------------------------------------------------

/// An on/off switch. Returns true when it was flipped.
pub fn toggle(ui: &mut Ui, on: &mut bool) -> bool {
    toggle_enabled(ui, on, true)
}

pub fn toggle_enabled(ui: &mut Ui, on: &mut bool, enabled: bool) -> bool {
    let size = vec2(42.0, 24.0);
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, mut resp) = ui.allocate_exact_size(size, sense);
    let mut flipped = false;
    if enabled && resp.clicked() {
        *on = !*on;
        flipped = true;
        resp.mark_changed();
    }
    let t = ui.ctx().animate_bool_with_time(resp.id, *on, 0.12);
    let accent = accent(ui);
    let off_fill = if resp.hovered() && enabled {
        HOVER
    } else {
        FIELD
    };
    let fill = lerp_color(off_fill, accent, t);
    let fade = |c: Color32| if enabled { c } else { c.gamma_multiply(0.45) };
    let p = ui.painter();
    p.rect_filled(rect, 12.0, fade(fill));
    if t < 1.0 {
        p.rect_stroke(
            rect,
            12.0,
            Stroke::new(
                1.0_f32,
                fade(Color32::from_rgb(0x55, 0x55, 0x5C)).gamma_multiply(1.0 - t),
            ),
            StrokeKind::Inside,
        );
    }
    let x = egui::lerp((rect.left() + 12.0)..=(rect.right() - 12.0), t);
    let knob = lerp_color(Color32::from_rgb(0xC8, 0xC8, 0xCE), Color32::WHITE, t);
    p.circle_filled(Pos2::new(x, rect.center().y), 7.5 + t, fade(knob));
    if enabled {
        resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    }
    flipped
}

fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let m = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round() as u8;
    Color32::from_rgb(m(a.r(), b.r()), m(a.g(), b.g()), m(a.b(), b.b()))
}

/// A row of options in a rounded tray, the chosen one raised. Returns true on a change.
pub fn segmented<T: PartialEq + Copy>(ui: &mut Ui, value: &mut T, options: &[(T, &str)]) -> bool {
    segmented_enabled(ui, value, options, true)
}

pub fn segmented_enabled<T: PartialEq + Copy>(
    ui: &mut Ui,
    value: &mut T,
    options: &[(T, &str)],
    enabled: bool,
) -> bool {
    let font = semibold(13.0);
    let pad = 12.0;
    let widths: Vec<f32> = options
        .iter()
        .map(|(_, label)| text_width(ui, label, &font) + 2.0 * pad)
        .collect();
    let total: f32 = widths.iter().sum::<f32>() + 6.0;
    let (rect, _) = ui.allocate_exact_size(vec2(total, 32.0), Sense::hover());
    let fade = |c: Color32| if enabled { c } else { c.gamma_multiply(0.45) };
    ui.painter().rect_filled(rect, 9.0, FIELD);
    ui.painter().rect_stroke(
        rect,
        9.0,
        Stroke::new(1.0_f32, CARD_EDGE),
        StrokeKind::Inside,
    );
    let mut x = rect.left() + 3.0;
    let mut changed = false;
    for ((option, label), w) in options.iter().zip(widths) {
        let r = Rect::from_min_size(Pos2::new(x, rect.top() + 3.0), vec2(w, rect.height() - 6.0));
        let id = ui.id().with(("seg", label, x as i32));
        let sense = if enabled {
            Sense::click()
        } else {
            Sense::hover()
        };
        let resp = ui.interact(r, id, sense);
        let chosen = *value == *option;
        if chosen {
            ui.painter()
                .rect_filled(r, 7.0, fade(Color32::from_rgb(0x3E, 0x3E, 0x45)));
        } else if resp.hovered() && enabled {
            ui.painter().rect_filled(r, 7.0, HOVER);
        }
        ui.painter().text(
            r.center(),
            Align2::CENTER_CENTER,
            *label,
            font.clone(),
            fade(if chosen { TEXT } else { SUB }),
        );
        if enabled && resp.clicked() && !chosen {
            *value = *option;
            changed = true;
        }
        if enabled {
            resp.on_hover_cursor(egui::CursorIcon::PointingHand);
        }
        x += w;
    }
    changed
}

fn text_width(ui: &Ui, text: &str, font: &FontId) -> f32 {
    ui.fonts(|f| {
        f.layout_no_wrap(text.to_string(), font.clone(), TEXT)
            .size()
            .x
    })
}

/// A slider with the value written to its right. `step` rounds the value; `show` formats it.
pub fn slider(
    ui: &mut Ui,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    step: f32,
    show: impl Fn(f32) -> String,
) -> bool {
    slider_enabled(ui, value, range, step, show, true)
}

pub fn slider_enabled(
    ui: &mut Ui,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    step: f32,
    show: impl Fn(f32) -> String,
    enabled: bool,
) -> bool {
    let (lo, hi) = (*range.start(), *range.end());
    let fade = |c: Color32| if enabled { c } else { c.gamma_multiply(0.45) };
    // Right to left: the value text first, then the track.
    let label = show(*value);
    let label_w =
        text_width(ui, &show(hi), &regular(13.0)).max(text_width(ui, &label, &regular(13.0)));
    let (text_rect, _) = ui.allocate_exact_size(vec2(label_w.max(44.0), 24.0), Sense::hover());
    ui.painter().text(
        text_rect.right_center(),
        Align2::RIGHT_CENTER,
        &label,
        regular(13.0),
        fade(TEXT),
    );
    ui.add_space(6.0);
    let sense = if enabled {
        Sense::click_and_drag()
    } else {
        Sense::hover()
    };
    let (rect, resp) = ui.allocate_exact_size(vec2(170.0, 24.0), sense);
    let track = Rect::from_center_size(rect.center(), vec2(rect.width() - 16.0, 4.0));
    let mut changed = false;
    if enabled
        && (resp.dragged() || resp.clicked())
        && let Some(p) = resp.interact_pointer_pos()
    {
        let t = ((p.x - track.left()) / track.width()).clamp(0.0, 1.0);
        let mut v = lo + t * (hi - lo);
        if step > 0.0 {
            v = (v / step).round() * step;
        }
        let v = v.clamp(lo, hi);
        if (v - *value).abs() > f32::EPSILON {
            *value = v;
            changed = true;
        }
    }
    let t = if hi > lo {
        ((*value - lo) / (hi - lo)).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let x = track.left() + t * track.width();
    let p = ui.painter();
    p.rect_filled(track, 2.0, fade(Color32::from_rgb(0x44, 0x44, 0x4B)));
    p.rect_filled(
        Rect::from_min_max(track.min, Pos2::new(x, track.max.y)),
        2.0,
        fade(accent(ui)),
    );
    let grow = if resp.hovered() || resp.dragged() {
        1.0
    } else {
        0.0
    };
    p.circle_filled(Pos2::new(x, track.center().y), 8.0 + grow, fade(accent(ui)));
    p.circle_filled(Pos2::new(x, track.center().y), 3.5, fade(Color32::WHITE));
    if enabled {
        resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    }
    changed
}

/// `slider` for whole numbers.
pub fn slider_u32(
    ui: &mut Ui,
    value: &mut u32,
    range: std::ops::RangeInclusive<u32>,
    step: u32,
    show: impl Fn(u32) -> String,
) -> bool {
    let mut v = *value as f32;
    let changed = slider(
        ui,
        &mut v,
        (*range.start() as f32)..=(*range.end() as f32),
        step.max(1) as f32,
        |x| show(x.round() as u32),
    );
    if changed {
        *value = (v.round().max(0.0) as u32).clamp(*range.start(), *range.end());
    }
    changed
}

/// A drop-down list. Returns true on a change.
pub fn dropdown<T: PartialEq + Clone>(
    ui: &mut Ui,
    id: &str,
    value: &mut T,
    options: &[(T, String)],
    width: f32,
) -> bool {
    let current = options
        .iter()
        .find(|(v, _)| v == value)
        .map(|(_, l)| l.clone())
        .unwrap_or_default();
    let mut changed = false;
    egui::ComboBox::from_id_salt(id)
        .selected_text(RichText::new(current).font(regular(13.5)).color(TEXT))
        .width(width)
        .height(320.0)
        .show_ui(ui, |ui| {
            ui.set_min_width(width);
            for (v, label) in options {
                if ui
                    .selectable_label(*value == *v, RichText::new(label).font(regular(13.5)))
                    .clicked()
                    && *value != *v
                {
                    *value = v.clone();
                    changed = true;
                }
            }
        });
    changed
}

/// A one-line text box.
pub fn field(ui: &mut Ui, text: &mut String, hint: &str, width: f32) -> Response {
    ui.add(
        egui::TextEdit::singleline(text)
            .hint_text(RichText::new(hint).color(FAINT))
            .font(regular(13.5))
            .desired_width(width)
            .margin(Margin::symmetric(10, 7)),
    )
}

/// A quiet button: an icon (optional) and a label.
pub fn button(ui: &mut Ui, glyph: &str, label: &str) -> Response {
    button_enabled(ui, glyph, label, true)
}

pub fn button_enabled(ui: &mut Ui, glyph: &str, label: &str, enabled: bool) -> Response {
    let font = semibold(13.0);
    let icon_w = if glyph.is_empty() { 0.0 } else { 20.0 };
    let w = text_width(ui, label, &font) + icon_w + 24.0;
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 32.0), sense);
    let fade = |c: Color32| if enabled { c } else { c.gamma_multiply(0.4) };
    let fill = if enabled && resp.is_pointer_button_down_on() {
        Color32::from_rgb(0x40, 0x40, 0x47)
    } else if enabled && resp.hovered() {
        Color32::from_rgb(0x38, 0x38, 0x3E)
    } else {
        Color32::from_rgb(0x30, 0x30, 0x35)
    };
    let p = ui.painter();
    p.rect_filled(rect, 8.0, fade(fill));
    p.rect_stroke(
        rect,
        8.0,
        Stroke::new(1.0_f32, fade(CARD_EDGE)),
        StrokeKind::Inside,
    );
    let mut x = rect.left() + 12.0;
    if !glyph.is_empty() {
        p.text(
            Pos2::new(x + 7.0, rect.center().y),
            Align2::CENTER_CENTER,
            glyph,
            icon_font(13.0),
            fade(TEXT),
        );
        x += icon_w;
    }
    p.text(
        Pos2::new(x, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        font,
        fade(TEXT),
    );
    if enabled {
        resp.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        resp
    }
}

/// A small square icon button (remove, move up, ...).
pub fn icon_button(ui: &mut Ui, glyph: &str, hint: &str) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(28.0, 28.0), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(rect, 7.0, HOVER);
    }
    let color = if resp.hovered() { TEXT } else { SUB };
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        glyph,
        icon_font(12.0),
        color,
    );
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(hint)
}

/// A colour button: a swatch that opens egui's picker. Returns the new `#RRGGBB` on a change.
pub fn color_button(ui: &mut Ui, hex: &str, fallback: u32) -> Option<String> {
    let c = pillbox_core::theme::parse_hex(hex)
        .unwrap_or_else(|| pillbox_core::theme::Rgba::rgb(fallback));
    let mut rgb = [
        (c.r * 255.0).round() as u8,
        (c.g * 255.0).round() as u8,
        (c.b * 255.0).round() as u8,
    ];
    let changed = ui.color_edit_button_srgb(&mut rgb).changed();
    changed.then(|| format!("#{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2]))
}

/// An editable list of short names, as chips with a remove button, plus a box to add one.
/// Returns true on a change. `lower` stores new entries in lower case.
pub fn chip_list(
    ui: &mut Ui,
    id: &str,
    list: &mut Vec<String>,
    new: &mut String,
    hint: &str,
    lower: bool,
) -> bool {
    let mut changed = false;
    let mut remove = None;
    if !list.is_empty() {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            for (i, item) in list.iter().enumerate() {
                let font = regular(13.0);
                let w = text_width(ui, item, &font) + 40.0;
                let (rect, _) = ui.allocate_exact_size(vec2(w, 28.0), Sense::hover());
                ui.painter().rect_filled(rect, 14.0, TILE);
                ui.painter().text(
                    Pos2::new(rect.left() + 12.0, rect.center().y),
                    Align2::LEFT_CENTER,
                    item,
                    font,
                    TEXT,
                );
                let x_rect = Rect::from_center_size(
                    Pos2::new(rect.right() - 14.0, rect.center().y),
                    vec2(20.0, 20.0),
                );
                let x = ui.interact(x_rect, Id::new((id, "x", i)), Sense::click());
                if x.hovered() {
                    ui.painter().circle_filled(x_rect.center(), 9.0, HOVER);
                }
                ui.painter().text(
                    x_rect.center(),
                    Align2::CENTER_CENTER,
                    icon::CANCEL,
                    icon_font(9.0),
                    if x.hovered() { TEXT } else { SUB },
                );
                if x.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    remove = Some(i);
                }
            }
        });
    }
    if let Some(i) = remove {
        list.remove(i);
        changed = true;
    }
    ui.horizontal(|ui| {
        let resp = ui.push_id(id, |ui| field(ui, new, hint, 240.0)).inner;
        let entry = if lower {
            new.trim().to_lowercase()
        } else {
            new.trim().to_string()
        };
        let can_add = !entry.is_empty() && !list.iter().any(|e| e.eq_ignore_ascii_case(&entry));
        let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if (button_enabled(ui, icon::ADD, "Add", can_add).clicked() || enter) && can_add {
            list.push(entry);
            new.clear();
            changed = true;
        }
    });
    changed
}

/// A picture card for a grid of choices: `paint` draws the preview, the title and a grey line
/// sit under it, and the chosen card has an accent outline. Returns true when clicked.
pub fn choice_card(
    ui: &mut Ui,
    chosen: bool,
    size: Vec2,
    title: &str,
    sub: &str,
    paint: impl FnOnce(&egui::Painter, Rect),
) -> bool {
    let text_h = if sub.is_empty() { 26.0 } else { 42.0 };
    let (rect, resp) = ui.allocate_exact_size(vec2(size.x, size.y + text_h), Sense::click());
    let preview = Rect::from_min_size(rect.min, size);
    let painter = ui.painter_at(preview);
    painter.rect_filled(preview, 10.0, Color32::from_rgb(0x10, 0x10, 0x12));
    paint(&painter, preview.shrink(1.0));
    let outline = if chosen {
        Stroke::new(2.0_f32, accent(ui))
    } else if resp.is_pointer_button_down_on() {
        // Pressed: the accent outline starts to show.
        Stroke::new(2.0_f32, accent(ui).gamma_multiply(0.55))
    } else if resp.hovered() {
        Stroke::new(1.0_f32, Color32::from_rgb(0x5A, 0x5A, 0x62))
    } else {
        Stroke::new(1.0_f32, CARD_EDGE)
    };
    ui.painter()
        .rect_stroke(preview, 10.0, outline, StrokeKind::Inside);
    if chosen {
        let badge = Pos2::new(preview.right() - 14.0, preview.top() + 14.0);
        ui.painter().circle_filled(badge, 9.0, accent(ui));
        ui.painter().text(
            badge,
            Align2::CENTER_CENTER,
            icon::CHECK,
            icon_font(10.0),
            Color32::WHITE,
        );
    }
    ui.painter().text(
        Pos2::new(rect.left() + 2.0, preview.bottom() + 8.0),
        Align2::LEFT_TOP,
        title,
        semibold(13.5),
        TEXT,
    );
    if !sub.is_empty() {
        ui.painter().text(
            Pos2::new(rect.left() + 2.0, preview.bottom() + 26.0),
            Align2::LEFT_TOP,
            sub,
            regular(12.0),
            SUB,
        );
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
}

/// Lay out `count` cards of at least `min_w` in rows that fill the width; `card(ui, i, w)`
/// draws card `i` at width `w`.
pub fn card_grid(ui: &mut Ui, count: usize, min_w: f32, mut card: impl FnMut(&mut Ui, usize, f32)) {
    let gap = 12.0;
    let width = ui.available_width();
    let cols = (((width + gap) / (min_w + gap)).floor() as usize).clamp(1, count.max(1));
    let w = ((width - gap * (cols as f32 - 1.0)) / cols as f32).floor();
    for start in (0..count).step_by(cols) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for i in start..(start + cols).min(count) {
                card(ui, i, w);
            }
        });
        ui.add_space(8.0);
    }
}

// ---- shortcut recorder ------------------------------------------------------------------------

/// The hotkey name (as `pillbox_core::hotkey` spells it) for an egui key, if it can be one.
pub fn hotkey_key_name(key: egui::Key) -> Option<String> {
    use egui::Key as K;
    let name = match key {
        K::Space => "Space",
        K::Enter => "Enter",
        K::Tab => "Tab",
        K::Backspace => "Backspace",
        K::Delete => "Delete",
        K::Insert => "Insert",
        K::Home => "Home",
        K::End => "End",
        K::PageUp => "PageUp",
        K::PageDown => "PageDown",
        K::ArrowLeft => "Left",
        K::ArrowRight => "Right",
        K::ArrowUp => "Up",
        K::ArrowDown => "Down",
        K::Minus => "-",
        K::Plus | K::Equals => "=",
        K::Comma => ",",
        K::Period => ".",
        K::Slash => "/",
        K::Semicolon => ";",
        K::Backtick => "`",
        K::OpenBracket => "[",
        K::CloseBracket => "]",
        K::Backslash => "\\",
        K::Quote => "'",
        other => {
            let n = other.name();
            // Letters, digits and F1..F24 are spelled the same way in both.
            let ok = (n.len() == 1 && n.chars().all(|c| c.is_ascii_alphanumeric()))
                || n.strip_prefix('F')
                    .and_then(|d| d.parse::<u32>().ok())
                    .is_some_and(|d| (1..=24).contains(&d));
            return ok.then(|| n.to_string());
        }
    };
    Some(name.to_string())
}

/// The shortcut text for a key press with these modifiers, before validation.
pub fn hotkey_text(mods: egui::Modifiers, key_name: &str) -> String {
    let mut s = String::new();
    if mods.ctrl {
        s.push_str("Ctrl+");
    }
    if mods.alt {
        s.push_str("Alt+");
    }
    if mods.shift {
        s.push_str("Shift+");
    }
    s.push_str(key_name);
    s
}

/// What a shortcut box is doing, kept in egui's memory by its id.
#[derive(Clone, Default)]
struct RecorderState {
    recording: bool,
    error: Option<String>,
}

/// A shortcut box: shows the shortcut; click it and press keys to record a new one (Esc
/// cancels, Backspace clears). Only valid shortcuts are ever stored. Returns true on a change.
pub fn hotkey_recorder(ui: &mut Ui, id: &str, value: &mut String) -> bool {
    let id = Id::new(("hotkey", id));
    let mut state: RecorderState = ui.data(|d| d.get_temp(id)).unwrap_or_default();
    let was_recording = state.recording;
    let mut changed = false;
    let shown = if state.recording {
        "Press keys…".to_string()
    } else if value.is_empty() {
        "Not set".to_string()
    } else {
        value.clone()
    };
    let font = semibold(13.0);
    let w = (text_width(ui, &shown, &font) + 28.0).max(150.0);
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 32.0), Sense::click());
    let accent = accent(ui);
    let (fill, stroke) = if state.recording {
        (FIELD, Stroke::new(1.5_f32, accent))
    } else if resp.hovered() {
        (
            HOVER,
            Stroke::new(1.0_f32, Color32::from_rgb(0x55, 0x55, 0x5C)),
        )
    } else {
        (FIELD, Stroke::new(1.0_f32, CARD_EDGE))
    };
    ui.painter().rect_filled(rect, 8.0, fill);
    ui.painter()
        .rect_stroke(rect, 8.0, stroke, StrokeKind::Inside);
    let color = if state.recording {
        accent
    } else if value.is_empty() {
        FAINT
    } else {
        TEXT
    };
    ui.painter()
        .text(rect.center(), Align2::CENTER_CENTER, &shown, font, color);
    if resp.clicked() {
        state.recording = !state.recording;
        state.error = None;
        if state.recording {
            resp.request_focus();
        }
    }
    if state.recording {
        // Keep the keys for ourselves while recording (Tab would move the focus away).
        ui.memory_mut(|m| {
            m.set_focus_lock_filter(
                resp.id,
                egui::EventFilter {
                    tab: true,
                    horizontal_arrows: true,
                    vertical_arrows: true,
                    escape: true,
                },
            );
        });
        let pressed: Option<(egui::Key, egui::Modifiers)> = ui.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Key {
                    key,
                    pressed: true,
                    repeat: false,
                    modifiers,
                    ..
                } => Some((*key, *modifiers)),
                _ => None,
            })
        });
        if let Some((key, mods)) = pressed {
            let plain = !mods.ctrl && !mods.alt && !mods.shift;
            if key == egui::Key::Escape && plain {
                state.recording = false;
            } else if matches!(key, egui::Key::Backspace | egui::Key::Delete) && plain {
                if !value.is_empty() {
                    value.clear();
                    changed = true;
                }
                state.recording = false;
            } else if let Some(name) = hotkey_key_name(key) {
                match pillbox_core::hotkey::normalize(&hotkey_text(mods, &name)) {
                    Ok(canonical) => {
                        if *value != canonical {
                            *value = canonical;
                            changed = true;
                        }
                        state.recording = false;
                        state.error = None;
                    }
                    Err(e) => state.error = Some(e),
                }
            } else {
                state.error = Some("That key cannot be a shortcut".into());
            }
        }
        if resp.clicked_elsewhere() {
            state.recording = false;
        }
        ui.ctx().request_repaint();
    }
    // Done recording: give the keyboard back (the window saves only when nothing has it).
    if was_recording && !state.recording {
        resp.surrender_focus();
    }
    if let Some(e) = &state.error {
        ui.add_space(8.0);
        ui.add(egui::Label::new(RichText::new(e).font(regular(12.0)).color(BAD)).truncate());
    }
    ui.data_mut(|d| d.insert_temp(id, state));
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(mods: egui::Modifiers, key: egui::Key) -> Result<String, String> {
        let name = hotkey_key_name(key).ok_or_else(|| "no name".to_string())?;
        pillbox_core::hotkey::normalize(&hotkey_text(mods, &name))
    }

    #[test]
    fn recorded_keys_become_valid_shortcuts() {
        let ctrl_alt = egui::Modifiers {
            ctrl: true,
            alt: true,
            ..Default::default()
        };
        assert_eq!(record(ctrl_alt, egui::Key::N).as_deref(), Ok("Ctrl+Alt+N"));
        assert_eq!(
            record(ctrl_alt, egui::Key::F5).as_deref(),
            Ok("Ctrl+Alt+F5")
        );
        assert_eq!(
            record(ctrl_alt, egui::Key::ArrowLeft).as_deref(),
            Ok("Ctrl+Alt+Left")
        );
        assert_eq!(
            record(ctrl_alt, egui::Key::Num7).as_deref(),
            Ok("Ctrl+Alt+7")
        );
        assert_eq!(
            record(ctrl_alt, egui::Key::Minus).as_deref(),
            Ok("Ctrl+Alt+-")
        );
        let shift = egui::Modifiers {
            shift: true,
            ..Default::default()
        };
        // The same rules as typed shortcuts: a plain letter or Shift+letter is refused.
        assert!(record(egui::Modifiers::default(), egui::Key::A).is_err());
        assert!(record(shift, egui::Key::A).is_err());
        // Keys a shortcut cannot use have no name.
        assert_eq!(hotkey_key_name(egui::Key::F30), None);
        assert_eq!(hotkey_key_name(egui::Key::Copy), None);
    }

    #[test]
    fn search_needs_every_word() {
        assert!(matches("", &["anything"]));
        assert!(matches("clock 24", &["Clock format", "12 24 hour"]));
        assert!(!matches("clock weather", &["Clock format", "12 24 hour"]));
    }
}
