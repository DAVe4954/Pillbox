//! The Settings window (`pillbox.exe --settings`, its own process): a sidebar of pages, each a
//! column of cards with one setting per row. Changes apply at once: the window writes
//! `settings.json` a moment after the last change (text when its box is left or Enter is
//! pressed), and the running Pillbox notices the file and applies it.

mod kit;
mod pages;
mod preview;

use std::{
    sync::mpsc,
    time::{Duration, Instant, SystemTime},
};

use anyhow::{Result, anyhow};
use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, Layout, Margin, Pos2, Rect, RichText, Sense,
    Stroke, StrokeKind, vec2,
};
use kit::*;
use pillbox_core::{face::Place, settings::Settings};

use crate::{
    platform::{
        foreground, media, settings_window,
        system::{self, Monitor},
    },
    store,
};

/// The window title; also how the frame helper finds the window.
pub const TITLE: &str = "Pillbox Settings";
/// A change is written this long after the last edit (a slider being dragged writes once).
const SAVE_AFTER: Duration = Duration::from_millis(300);
/// How often the window looks for changes Pillbox made to the file.
const DISK_CHECK: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    General,
    ClosedPill,
    Appearance,
    Music,
    Widgets,
    Clipboard,
    Notifications,
    Games,
    Agents,
    Shortcuts,
    About,
}

impl Page {
    const ALL: [Page; 11] = [
        Page::General,
        Page::ClosedPill,
        Page::Appearance,
        Page::Music,
        Page::Widgets,
        Page::Clipboard,
        Page::Notifications,
        Page::Games,
        Page::Agents,
        Page::Shortcuts,
        Page::About,
    ];

    fn title(self) -> &'static str {
        match self {
            Page::General => "General",
            Page::ClosedPill => "Closed pill",
            Page::Appearance => "Appearance",
            Page::Music => "Music",
            Page::Widgets => "Home & widgets",
            Page::Clipboard => "Clipboard & Shelf",
            Page::Notifications => "Notifications & calls",
            Page::Games => "Games & Apps",
            Page::Agents => "Agents",
            Page::Shortcuts => "Shortcuts",
            Page::About => "About",
        }
    }

    /// One line under the page heading: what the page is for.
    fn blurb(self) -> &'static str {
        match self {
            Page::General => "Starting, where Pillbox sits, how big it is, hiding and opening",
            Page::ClosedPill => {
                "What the small pill shows: the clock, a song, a call and other notes"
            }
            Page::Appearance => "Colours, themes, fades and glass",
            Page::Music => "The song on the pill and the Music tab",
            Page::Widgets => "The Home tab's widgets, weather and the System window",
            Page::Clipboard => "Clipboard history, snippets, the file shelf and your phone",
            Page::Notifications => "Windows notifications, calls and the microphone, reminders",
            Page::Games => "What Pillbox does while you play, and rules for single apps",
            Page::Agents => "Claude Code, Codex and the Activity tab",
            Page::Shortcuts => "Keyboard shortcuts and the launcher",
            Page::About => "Version, your data, and resetting",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Page::General => icon::SETTINGS,
            Page::ClosedPill => icon::HOME,
            Page::Appearance => icon::PALETTE,
            Page::Music => icon::MUSIC,
            Page::Widgets => icon::TILES,
            Page::Clipboard => icon::PASTE,
            Page::Notifications => icon::BELL,
            Page::Games => icon::GAME,
            Page::Agents => icon::ROBOT,
            Page::Shortcuts => icon::KEYBOARD,
            Page::About => icon::INFO,
        }
    }
}

/// Text typed into the "add" boxes of the lists, kept between frames.
#[derive(Default)]
struct NewEntries {
    source: String,
    calls_ignore: String,
    clip_ignore: String,
    games_ignore: String,
    zone: String,
    site: String,
    currency: String,
    note_app: String,
    rule_exe: String,
}

/// The weather page's city search: one request at a time, on its own thread.
#[derive(Default)]
struct CitySearch {
    query: String,
    results: Vec<Place>,
    busy: Option<mpsc::Receiver<Result<Vec<Place>, String>>>,
    /// The last search's outcome when it found nothing or failed.
    message: Option<String>,
}

pub struct SettingsApp {
    /// What the window shows and edits.
    draft: Settings,
    /// The draft as it was when last written; the draft differs from it while edits wait.
    written: Settings,
    /// The draft one frame ago, to see when the last edit happened.
    last_draft: Settings,
    last_edit: Instant,
    /// The file's modification time after our own last write or read.
    disk_mtime: Option<SystemTime>,
    disk_checked: Instant,
    page: Page,
    query: String,
    /// A start scroll position from the developer aid, used on the first frame.
    dev_scroll: Option<f32>,
    /// Where the developer aid saves a picture of the window, then closes it.
    dev_shot: Option<std::path::PathBuf>,
    frames: u32,
    shot_asked: bool,
    started: Instant,
    status: Option<(String, bool, Instant)>,
    monitors: Vec<Monitor>,
    open_apps: Vec<String>,
    media_apps: Vec<String>,
    autostart: bool,
    /// Apps using the microphone (General > On the closed pill), and when that was read.
    mic_apps: Vec<String>,
    mic_read: Option<Instant>,
    new: NewEntries,
    city: CitySearch,
    confirm_reset: bool,
    show_timing: bool,
    /// Windows 11 rounded corners and dark frame requested (retried until the window exists).
    framed: bool,
    /// Write to `settings.json` (false only in tests).
    persist: bool,
    /// How many times the draft was written (for tests).
    saves: u32,
}

/// Developer aid for checking pages without clicking: `PILLBOX_SETTINGS_PAGE=music` opens that
/// page (its title or the first word of it, any case) and `PILLBOX_SETTINGS_QUERY=…` fills the
/// search box. With either set, the window opens without taking the focus.
#[derive(Default)]
struct DevStart {
    page: Option<Page>,
    query: Option<String>,
    /// `PILLBOX_SETTINGS_SCROLL=600`: start the page scrolled down this far.
    scroll: Option<f32>,
    /// `PILLBOX_SETTINGS_SHOT=C:\…\page.png`: save a picture of the window there, then close.
    shot: Option<std::path::PathBuf>,
}

impl DevStart {
    fn any(&self) -> bool {
        self.page.is_some() || self.query.is_some() || self.scroll.is_some() || self.shot.is_some()
    }
}

fn dev_start() -> DevStart {
    let page = std::env::var("PILLBOX_SETTINGS_PAGE")
        .ok()
        .and_then(|want| {
            let want = want.trim().to_lowercase();
            Page::ALL.into_iter().find(|p| {
                let title = p.title().to_lowercase();
                title == want || title.split_whitespace().next() == Some(want.as_str())
            })
        });
    let query = std::env::var("PILLBOX_SETTINGS_QUERY")
        .ok()
        .filter(|q| !q.trim().is_empty());
    let scroll = std::env::var("PILLBOX_SETTINGS_SCROLL")
        .ok()
        .and_then(|s| s.trim().parse::<f32>().ok())
        .filter(|s| s.is_finite() && *s > 0.0);
    let shot = std::env::var_os("PILLBOX_SETTINGS_SHOT")
        .filter(|s| !s.is_empty())
        .map(std::path::PathBuf::from);
    DevStart {
        page,
        query,
        scroll,
        shot,
    }
}

fn disk_mtime() -> Option<SystemTime> {
    let path = store::path().ok()?;
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

impl SettingsApp {
    fn load() -> Self {
        let saved = store::load().unwrap_or_default();
        let dev = dev_start();
        let mut app = Self::with(saved, true);
        app.page = dev.page.unwrap_or(Page::General);
        app.query = dev.query.unwrap_or_default();
        app.dev_scroll = dev.scroll;
        app.dev_shot = dev.shot;
        app.monitors = system::monitors();
        app.open_apps = foreground::open_apps();
        app.media_apps = media::session_app_ids();
        app.autostart = system::autostart_enabled();
        app.disk_mtime = disk_mtime();
        app
    }

    /// The window state for `saved`, with nothing read from the PC. `persist` = false (tests)
    /// never touches `settings.json`: a save only marks the draft as written.
    fn with(saved: Settings, persist: bool) -> Self {
        Self {
            draft: saved.clone(),
            written: saved.clone(),
            last_draft: saved,
            last_edit: Instant::now(),
            disk_mtime: None,
            disk_checked: Instant::now(),
            page: Page::General,
            query: String::new(),
            dev_scroll: None,
            dev_shot: None,
            frames: 0,
            shot_asked: false,
            started: Instant::now(),
            status: None,
            monitors: Vec::new(),
            open_apps: Vec::new(),
            media_apps: Vec::new(),
            autostart: false,
            mic_apps: Vec::new(),
            mic_read: None,
            new: NewEntries::default(),
            city: CitySearch::default(),
            confirm_reset: false,
            show_timing: false,
            framed: false,
            persist,
            saves: 0,
        }
    }

    fn say(&mut self, text: impl Into<String>, ok: bool) {
        self.status = Some((text.into(), ok, Instant::now()));
    }

    /// Write the draft. The System window's on/off is the one setting Pillbox itself also
    /// writes (from its own ✕ and the tray): unless it was changed here, the file's current
    /// value is kept, so a stale draft never undoes it.
    fn save(&mut self) {
        self.saves += 1;
        if !self.persist {
            self.written = self.draft.clone();
            return;
        }
        let mut next = self.draft.clone();
        if self.draft.widgets.system_window == self.written.widgets.system_window
            && let Some(disk) = store::load()
        {
            next.widgets.system_window = disk.widgets.system_window;
            self.draft.widgets.system_window = disk.widgets.system_window;
        }
        let mut warnings = Vec::new();
        next.sanitize(&mut warnings);
        for w in &warnings {
            tracing::info!("settings: {w}");
        }
        match store::save(&next) {
            Ok(()) => {
                self.written = self.draft.clone();
                self.disk_mtime = disk_mtime();
                self.say("Saved", true);
            }
            Err(e) => {
                tracing::error!("saving settings: {e:#}");
                self.say(format!("Could not save: {e:#}"), false);
                // Try again after the next edit rather than every frame.
                self.written = self.draft.clone();
            }
        }
    }

    /// Pick up a change Pillbox (or a hand edit) made to the file. With no edits waiting,
    /// the draft becomes the file; with edits waiting, only the System window switch is taken
    /// (the edits are written over the rest a moment later).
    fn check_disk(&mut self) {
        if !self.persist || self.disk_checked.elapsed() < DISK_CHECK {
            return;
        }
        self.disk_checked = Instant::now();
        let mtime = disk_mtime();
        if mtime == self.disk_mtime {
            return;
        }
        self.disk_mtime = mtime;
        let Some(disk) = store::load() else { return };
        if self.draft == self.written {
            self.draft = disk.clone();
            self.written = disk.clone();
            self.last_draft = disk;
        } else if self.draft.widgets.system_window == self.written.widgets.system_window {
            self.draft.widgets.system_window = disk.widgets.system_window;
            self.written.widgets.system_window = disk.widgets.system_window;
        }
    }

    /// Write waiting edits once they have settled: no slider held, no text box being typed in.
    fn autosave(&mut self, ctx: &egui::Context) {
        if self.draft != self.last_draft {
            self.last_draft = self.draft.clone();
            self.last_edit = Instant::now();
        }
        if self.draft == self.written {
            return;
        }
        let typing = ctx.wants_keyboard_input();
        let holding = ctx.input(|i| i.pointer.any_down());
        if self.last_edit.elapsed() >= SAVE_AFTER && !typing && !holding {
            self.save();
        } else {
            ctx.request_repaint_after(SAVE_AFTER);
        }
    }

    /// The city search's answer, when it arrives.
    fn poll_city(&mut self) {
        let Some(rx) = &self.city.busy else { return };
        match rx.try_recv() {
            Ok(Ok(places)) => {
                self.city.message = places
                    .is_empty()
                    .then(|| "No place by that name".to_string());
                self.city.results = places;
                self.city.busy = None;
            }
            Ok(Err(e)) => {
                self.city.message = Some(e);
                self.city.results.clear();
                self.city.busy = None;
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => {
                self.city.message = Some("The search stopped".into());
                self.city.busy = None;
            }
        }
    }

    fn start_city_search(&mut self, ctx: &egui::Context) {
        let name = self.city.query.trim().to_string();
        if name.is_empty() || self.city.busy.is_some() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        let spawned = std::thread::Builder::new()
            .name("city-search".into())
            .spawn(move || {
                let agent = ureq::AgentBuilder::new()
                    .timeout(Duration::from_secs(10))
                    .build();
                let result = agent
                    .get(&pillbox_core::face::geocode_url(&name))
                    .call()
                    .map_err(|e| format!("Search failed: {e}"))
                    .and_then(|r| r.into_string().map_err(|e| format!("Search failed: {e}")))
                    .map(|body| pillbox_core::face::parse_geocode(&body));
                let _ = tx.send(result);
                ctx.request_repaint();
            });
        match spawned {
            Ok(_) => {
                self.city.busy = Some(rx);
                self.city.message = None;
            }
            Err(e) => self.city.message = Some(format!("Search failed: {e}")),
        }
    }

    fn close(&mut self, ctx: &egui::Context) {
        if self.draft != self.written {
            self.save();
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }

    // ---- chrome -------------------------------------------------------------------------------

    fn sidebar(&mut self, ui: &mut egui::Ui) {
        // The top of the sidebar moves the window, like a title bar.
        let (head, drag) = ui.allocate_exact_size(vec2(ui.available_width(), 48.0), Sense::drag());
        if drag.drag_started() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }
        let tile = Rect::from_min_size(head.min + vec2(2.0, 6.0), vec2(36.0, 36.0));
        ui.painter()
            .rect_filled(tile, 10.0, Color32::from_rgb(0x2C, 0x2C, 0x31));
        let pill = Rect::from_center_size(tile.center(), vec2(24.0, 10.0));
        ui.painter().rect_filled(pill, 5.0, Color32::BLACK);
        ui.painter().rect_stroke(
            pill,
            5.0,
            Stroke::new(1.0_f32, Color32::from_rgb(0x55, 0x55, 0x5E)),
            StrokeKind::Outside,
        );
        ui.painter().circle_filled(
            Pos2::new(pill.right() - 5.0, pill.center().y),
            2.0,
            accent(ui),
        );
        ui.painter().text(
            Pos2::new(tile.right() + 12.0, tile.top() + 2.0),
            Align2::LEFT_TOP,
            "Pillbox",
            semibold(15.0),
            TEXT,
        );
        ui.painter().text(
            Pos2::new(tile.right() + 12.0, tile.top() + 21.0),
            Align2::LEFT_TOP,
            "Settings",
            regular(12.5),
            SUB,
        );
        ui.add_space(14.0);

        // Search: a field-coloured box holding the icon, a frameless text box and a clear button.
        egui::Frame::new()
            .fill(FIELD)
            .stroke(Stroke::new(1.0_f32, CARD_EDGE))
            .corner_radius(CornerRadius::same(9))
            .inner_margin(Margin::symmetric(10, 4))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    ui.label(RichText::new(icon::SEARCH).font(icon_font(13.0)).color(SUB));
                    // The clear button (28) and the gap before it (6), with a little room.
                    let clear_w = if self.query.is_empty() { 0.0 } else { 36.0 };
                    ui.add(
                        egui::TextEdit::singleline(&mut self.query)
                            .hint_text(RichText::new("Search settings").color(FAINT))
                            .font(regular(13.5))
                            .frame(false)
                            .margin(Margin::symmetric(0, 6))
                            .desired_width(ui.available_width() - clear_w),
                    );
                    if !self.query.is_empty()
                        && icon_button(ui, icon::CANCEL, "Clear the search").clicked()
                    {
                        self.query.clear();
                    }
                });
            });
        ui.add_space(12.0);

        let searching = !self.query.trim().is_empty();
        for page in Page::ALL {
            let selected = page == self.page && !searching;
            let (rect, resp) =
                ui.allocate_exact_size(vec2(ui.available_width(), 38.0), Sense::click());
            if selected {
                ui.painter()
                    .rect_filled(rect, 8.0, Color32::from_rgb(0x2A, 0x2A, 0x2F));
                let bar = Rect::from_min_size(
                    Pos2::new(rect.left(), rect.center().y - 9.0),
                    vec2(3.0, 18.0),
                );
                ui.painter().rect_filled(bar, 2.0, accent(ui));
            } else if resp.hovered() {
                ui.painter()
                    .rect_filled(rect, 8.0, Color32::from_rgb(0x22, 0x22, 0x26));
            }
            let color = if selected {
                TEXT
            } else {
                Color32::from_rgb(0xC4, 0xC4, 0xCA)
            };
            ui.painter().text(
                Pos2::new(rect.left() + 22.0, rect.center().y),
                Align2::CENTER_CENTER,
                page.icon(),
                icon_font(15.0),
                color,
            );
            ui.painter().text(
                Pos2::new(rect.left() + 42.0, rect.center().y),
                Align2::LEFT_CENTER,
                page.title(),
                if selected {
                    semibold(14.0)
                } else {
                    regular(14.0)
                },
                color,
            );
            if resp
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
            {
                self.page = page;
                self.query.clear();
            }
            ui.add_space(2.0);
        }

        ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
            ui.label(
                RichText::new(format!("Version {}", env!("CARGO_PKG_VERSION")))
                    .font(regular(12.0))
                    .color(FAINT),
            );
        });
    }

    /// The page heading with the window buttons; the empty part moves the window.
    fn header(&mut self, ui: &mut egui::Ui, title: &str) {
        let (bar, drag) = ui.allocate_exact_size(vec2(ui.available_width(), 72.0), Sense::drag());
        if drag.drag_started() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }
        ui.painter().text(
            Pos2::new(bar.left() + 36.0, bar.center().y + 4.0),
            Align2::LEFT_CENTER,
            title,
            semibold(26.0),
            TEXT,
        );
        // Saved / error note, fading after a few seconds.
        if let Some((text, ok, at)) = &self.status {
            let age = at.elapsed().as_secs_f32();
            if age < 3.0 {
                let fade = (3.0 - age).min(1.0);
                let color = if *ok { GOOD } else { BAD };
                let glyph = if *ok { icon::CHECK } else { icon::WARNING };
                let x = bar.right() - 112.0;
                let galley = ui.painter().layout_no_wrap(
                    text.clone(),
                    regular(12.5),
                    color.gamma_multiply(fade),
                );
                let left = x - galley.size().x;
                ui.painter().galley(
                    Pos2::new(left, bar.center().y + 4.0 - galley.size().y / 2.0),
                    galley,
                    color,
                );
                ui.painter().text(
                    Pos2::new(left - 12.0, bar.center().y + 4.0),
                    Align2::CENTER_CENTER,
                    glyph,
                    icon_font(11.0),
                    color.gamma_multiply(fade),
                );
                ui.ctx().request_repaint_after(Duration::from_millis(100));
            }
        }
        // Minimise and close.
        let size = vec2(40.0, 32.0);
        let close_rect = Rect::from_min_size(Pos2::new(bar.right() - 52.0, bar.top() + 14.0), size);
        let min_rect = close_rect.translate(vec2(-44.0, 0.0));
        for (rect, glyph, is_close) in [
            (min_rect, icon::MINIMIZE, false),
            (close_rect, icon::CLOSE, true),
        ] {
            let resp = ui.interact(rect, egui::Id::new(("chrome", is_close)), Sense::click());
            if resp.hovered() {
                let fill = if is_close {
                    Color32::from_rgb(0xC4, 0x2B, 0x1C)
                } else {
                    HOVER
                };
                ui.painter().rect_filled(rect, 8.0, fill);
            }
            ui.painter().text(
                rect.center(),
                Align2::CENTER_CENTER,
                glyph,
                icon_font(11.0),
                if resp.hovered() { TEXT } else { SUB },
            );
            if resp.clicked() {
                if is_close {
                    self.close(ui.ctx());
                } else {
                    ui.ctx()
                        .send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                }
            }
        }
    }

    fn content(&mut self, ui: &mut egui::Ui) {
        let query = self.query.trim().to_lowercase();
        let title = if query.is_empty() {
            self.page.title().to_string()
        } else {
            "Search".to_string()
        };
        self.header(ui, &title);
        let mut scroll = egui::ScrollArea::vertical().auto_shrink([false, false]);
        if let Some(y) = self.dev_scroll.take() {
            scroll = scroll.vertical_scroll_offset(y);
        }
        scroll.show(ui, |ui| {
            egui::Frame::new()
                .inner_margin(Margin {
                    left: 36,
                    right: 36,
                    top: 0,
                    bottom: 32,
                })
                .show(ui, |ui| {
                    // At most 860 wide; never wider than the window allows.
                    ui.set_max_width(ui.available_width().min(860.0));
                    if query.is_empty() {
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(self.page.blurb())
                                    .font(regular(13.0))
                                    .color(SUB),
                            )
                            .wrap(),
                        );
                        ui.add_space(10.0);
                        self.show_page(ui, self.page, "");
                    } else {
                        let before = ui.min_rect().height();
                        for page in Page::ALL {
                            self.show_page(ui, page, &query);
                        }
                        if (ui.min_rect().height() - before).abs() < 1.0 {
                            ui.add_space(40.0);
                            ui.vertical_centered(|ui| {
                                ui.label(
                                    RichText::new("No settings match your search")
                                        .font(regular(14.0))
                                        .color(SUB),
                                );
                            });
                        }
                    }
                });
        });
    }
}

impl eframe::App for SettingsApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.07, 0.07, 0.08, 1.0]
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if !self.framed {
            self.framed = settings_window::style(TITLE);
            if !self.framed {
                ctx.request_repaint_after(Duration::from_millis(50));
            }
        }
        self.frame(ctx);
    }
}

impl SettingsApp {
    /// One frame of the window: everything but the native frame styling, so a test can drive it
    /// headless.
    fn frame(&mut self, ctx: &egui::Context) {
        // Pillbox's accent colours the window too.
        let accent = pillbox_core::theme::parse_hex(&self.draft.appearance.accent)
            .map(|c| {
                Color32::from_rgb(
                    (c.r * 255.0).round() as u8,
                    (c.g * 255.0).round() as u8,
                    (c.b * 255.0).round() as u8,
                )
            })
            .unwrap_or(Color32::from_rgb(0x7C, 0x5C, 0xFF));
        ctx.style_mut(|s| s.visuals.selection.bg_fill = accent);

        self.check_disk();
        self.poll_city();
        if ctx.input(|i| i.viewport().close_requested()) && self.draft != self.written {
            self.save();
        }

        egui::SidePanel::left("nav")
            .exact_width(236.0)
            .resizable(false)
            .show_separator_line(false)
            .frame(egui::Frame::new().fill(SIDEBAR).inner_margin(Margin {
                left: 14,
                right: 14,
                top: 14,
                bottom: 14,
            }))
            .show(ctx, |ui| self.sidebar(ui));
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(CONTENT)
                    .inner_margin(Margin::same(0)),
            )
            .show(ctx, |ui| self.content(ui));

        self.autosave(ctx);
        self.dev_screenshot(ctx);
        ctx.request_repaint_after(DISK_CHECK);
    }
    /// The `PILLBOX_SETTINGS_SHOT` developer aid: once the window has settled (fonts loaded,
    /// layout done), ask egui for a picture of it, save it as PNG and close.
    fn dev_screenshot(&mut self, ctx: &egui::Context) {
        let Some(path) = self.dev_shot.clone() else {
            return;
        };
        self.frames += 1;
        // `PILLBOX_SETTINGS_SHOT_DELAY` (ms) waits longer, e.g. for posted test clicks.
        let delay = std::env::var("PILLBOX_SETTINGS_SHOT_DELAY")
            .ok()
            .and_then(|s| s.trim().parse::<u64>().ok())
            .unwrap_or(0);
        if self.frames >= 15
            && !self.shot_asked
            && self.started.elapsed() >= Duration::from_millis(delay)
        {
            self.shot_asked = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::new(())));
        }
        let image = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(image) = image {
            let [w, h] = image.size;
            let mut pixels = Vec::with_capacity(w * h * 4);
            for c in &image.pixels {
                pixels.extend_from_slice(&[c.b(), c.g(), c.r(), 255]);
            }
            let bgra = pillbox_core::image::Bgra {
                width: w as u32,
                height: h as u32,
                pixels,
            };
            match bgra.to_png() {
                Ok(png) => {
                    if let Err(e) = std::fs::write(&path, png) {
                        tracing::warn!("settings screenshot: {e}");
                    }
                }
                Err(e) => tracing::warn!("settings screenshot: {e}"),
            }
            self.dev_shot = None;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        ctx.request_repaint();
    }
}

/// The dark style every control starts from (the custom controls in `kit` draw themselves).
fn style(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = CONTENT;
    visuals.window_fill = Color32::from_rgb(0x2A, 0x2A, 0x2F);
    visuals.window_stroke = Stroke::new(1.0_f32, Color32::from_rgb(0x3A, 0x3A, 0x41));
    visuals.window_corner_radius = CornerRadius::same(10);
    visuals.menu_corner_radius = CornerRadius::same(10);
    visuals.extreme_bg_color = FIELD;
    visuals.faint_bg_color = CARD;
    visuals.override_text_color = Some(TEXT);
    visuals.selection.bg_fill = Color32::from_rgb(0x7C, 0x5C, 0xFF);
    visuals.selection.stroke = Stroke::new(1.0_f32, Color32::WHITE);
    visuals.hyperlink_color = Color32::from_rgb(0x9D, 0x8A, 0xFF);
    let radius = CornerRadius::same(8);
    for (w, fill, edge) in [
        (&mut visuals.widgets.noninteractive, CARD, CARD_EDGE),
        (
            &mut visuals.widgets.inactive,
            Color32::from_rgb(0x30, 0x30, 0x35),
            CARD_EDGE,
        ),
        (
            &mut visuals.widgets.hovered,
            Color32::from_rgb(0x38, 0x38, 0x3E),
            Color32::from_rgb(0x4A, 0x4A, 0x52),
        ),
        (
            &mut visuals.widgets.active,
            Color32::from_rgb(0x40, 0x40, 0x47),
            Color32::from_rgb(0x55, 0x55, 0x5E),
        ),
        (
            &mut visuals.widgets.open,
            Color32::from_rgb(0x34, 0x34, 0x3A),
            Color32::from_rgb(0x4A, 0x4A, 0x52),
        ),
    ] {
        w.corner_radius = radius;
        w.bg_fill = fill;
        w.weak_bg_fill = fill;
        w.bg_stroke = Stroke::new(1.0_f32, edge);
        w.expansion = 0.0;
    }
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, TEXT);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, TEXT);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, TEXT);
    visuals.widgets.active.fg_stroke = Stroke::new(1.0_f32, TEXT);
    ctx.set_visuals(visuals);
    ctx.style_mut(|s| {
        s.spacing.item_spacing = vec2(8.0, 6.0);
        s.spacing.button_padding = vec2(12.0, 6.0);
        s.spacing.interact_size.y = 32.0;
        s.spacing.combo_height = 320.0;
        s.spacing.scroll = egui::style::ScrollStyle::thin();
        s.interaction.selectable_labels = false;
    });
}

/// The app icon for the window's taskbar button and Alt+Tab entry. Set on the window itself,
/// so it shows even when Windows has not read the exe's icon resource.
fn window_icon() -> egui::IconData {
    const SIZE: u32 = 64;
    egui::IconData {
        rgba: pillbox_core::appicon::render(SIZE),
        width: SIZE,
        height: SIZE,
    }
}

pub fn run() -> Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(TITLE)
            .with_inner_size([940.0, 680.0])
            .with_min_inner_size([760.0, 480.0])
            .with_icon(window_icon())
            .with_decorations(false)
            .with_resizable(true)
            .with_active(!dev_start().any()),
        ..Default::default()
    };
    eframe::run_native(
        TITLE,
        options,
        Box::new(|cc| {
            install_fonts(&cc.egui_ctx);
            style(&cc.egui_ctx);
            Ok(Box::new(SettingsApp::load()))
        }),
    )
    .map_err(|e| anyhow!("{e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{Event, Modifiers, PointerButton, RawInput, Shape};

    /// Where `text` was drawn and every filled rectangle, from one frame's shapes.
    fn find(shapes: &[egui::epaint::ClippedShape], text: &str) -> (Option<Pos2>, Vec<Rect>) {
        fn walk(shape: &Shape, text: &str, at: &mut Option<Pos2>, rects: &mut Vec<Rect>) {
            match shape {
                Shape::Vec(list) => list.iter().for_each(|s| walk(s, text, at, rects)),
                Shape::Text(t) if t.galley.text() == text => *at = Some(t.pos),
                Shape::Rect(r) => rects.push(r.rect),
                _ => {}
            }
        }
        let (mut at, mut rects) = (None, Vec::new());
        for c in shapes {
            walk(&c.shape, text, &mut at, &mut rects);
        }
        (at, rects)
    }

    fn input(events: Vec<Event>) -> RawInput {
        RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(940.0, 680.0))),
            events,
            ..Default::default()
        }
    }

    fn button(pos: Pos2, pressed: bool) -> Event {
        Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::default(),
        }
    }

    #[test]
    fn a_click_flips_a_toggle_and_the_change_is_written_once_settled() {
        let ctx = egui::Context::default();
        install_fonts(&ctx);
        style(&ctx);
        let mut app = SettingsApp::with(Settings::default(), false);
        app.page = Page::Music;
        assert!(app.draft.music.pill_colors);
        // Lay the page out (twice: the first pass of a new window only measures).
        let _ = ctx.run(input(vec![]), |ctx| app.frame(ctx));
        let out = ctx.run(input(vec![]), |ctx| app.frame(ctx));
        let (title, rects) = find(&out.shapes, "Album colours on the pill");
        let title = match title {
            Some(t) => t,
            None => panic!("the row is drawn"),
        };
        // The row's switch: the 42×24 track on the same row, right of the title.
        let switch = rects
            .iter()
            .filter(|r| (r.width() - 42.0).abs() < 0.5 && (r.height() - 24.0).abs() < 0.5)
            .min_by(|a, b| {
                (a.center().y - title.y - 10.0)
                    .abs()
                    .total_cmp(&(b.center().y - title.y - 10.0).abs())
            })
            .copied();
        let switch = match switch {
            Some(s) => s,
            None => panic!("the switch is drawn"),
        };
        assert!(
            switch.center().x > title.x + 200.0,
            "on the right: {switch:?}"
        );
        // Press and release on it.
        let at = switch.center();
        let _ = ctx.run(
            input(vec![Event::PointerMoved(at), button(at, true)]),
            |ctx| app.frame(ctx),
        );
        let _ = ctx.run(input(vec![button(at, false)]), |ctx| app.frame(ctx));
        assert!(!app.draft.music.pill_colors, "the click turned it off");
        // Not written at once (a slider being dragged must not write on every frame)...
        assert_eq!(app.saves, 0);
        // ...but once the edit has settled.
        std::thread::sleep(SAVE_AFTER + Duration::from_millis(50));
        let _ = ctx.run(
            input(vec![Event::PointerMoved(Pos2::new(600.0, 30.0))]),
            |ctx| app.frame(ctx),
        );
        assert_eq!(app.saves, 1);
        assert_eq!(app.written, app.draft);
        // Nothing more to write: no second save.
        let _ = ctx.run(input(vec![]), |ctx| app.frame(ctx));
        assert_eq!(app.saves, 1);
    }

    #[test]
    fn every_page_and_a_search_draw_without_trouble() {
        let ctx = egui::Context::default();
        install_fonts(&ctx);
        style(&ctx);
        let mut app = SettingsApp::with(Settings::default(), false);
        for page in Page::ALL {
            app.page = page;
            let _ = ctx.run(input(vec![]), |ctx| app.frame(ctx));
            let out = ctx.run(input(vec![]), |ctx| app.frame(ctx));
            let (heading, _) = find(&out.shapes, page.title());
            assert!(heading.is_some(), "{} has its heading", page.title());
        }
        app.query = "clock".into();
        let out = ctx.run(input(vec![]), |ctx| app.frame(ctx));
        assert!(find(&out.shapes, "Clock format").0.is_some());
        assert!(find(&out.shapes, "Clock colour").0.is_some());
        assert!(
            find(&out.shapes, "Synced lyrics").0.is_none(),
            "unrelated rows hidden"
        );
        // Drawing changes nothing by itself.
        assert_eq!(app.draft, Settings::default());
        assert_eq!(app.saves, 0);
    }
}
