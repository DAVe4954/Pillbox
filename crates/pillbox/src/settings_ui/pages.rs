//! The Settings pages. Every setting in `settings.json` that a person would change has one row
//! here; the search box matches row titles, their grey lines, a few extra words, and the page
//! and section names.

use eframe::egui::{self, Align2, Color32, CornerRadius, Pos2, Rect, Stroke, StrokeKind, Ui, vec2};
use pillbox_core::{
    settings::{
        AppRule, AppTheme, Appearance, CLOCK_SIZE_MAX, CLOCK_SIZE_MIN, ClockColor,
        FADE_OPACITY_MIN, FADE_STOPS_MAX, FADE_STOPS_MIN, FadeStop, GameBehavior, LAUNCH_SLOTS,
        LaunchSlot, MusicReveal, NoteMode, NoteRule, PillClock, ProgressStyle, ReopenTab,
        RestingFace, RuleBehavior, SIZE_MAX, SIZE_MIN, Side, Snippet, SourceMode, StripBackground,
        Style, Surface, Visualizer, WeatherLook, WidgetKind,
    },
    tabs::{Tab, TabsSettings},
    theme::{self, Theme},
};

use super::{Page, SettingsApp, kit::*};
use crate::platform::{foreground, media, sensors, system};

/// Accent colours offered as one-click swatches (any colour can still be picked).
const ACCENTS: [&str; 8] = [
    "#7C5CFF", "#3B82F6", "#06B6D4", "#22C55E", "#EAB308", "#F97316", "#EF4444", "#EC4899",
];

fn to_color32(c: theme::Rgba) -> Color32 {
    Color32::from_rgba_unmultiplied(
        (c.r.clamp(0.0, 1.0) * 255.0).round() as u8,
        (c.g.clamp(0.0, 1.0) * 255.0).round() as u8,
        (c.b.clamp(0.0, 1.0) * 255.0).round() as u8,
        (c.a.clamp(0.0, 1.0) * 255.0).round() as u8,
    )
}

fn hex_color(hex: &str) -> Color32 {
    theme::parse_hex(hex).map_or(Color32::from_rgb(0x7C, 0x5C, 0xFF), to_color32)
}

impl SettingsApp {
    /// One page; `q` is the search (lower case, empty = show everything).
    pub(super) fn show_page(&mut self, ui: &mut Ui, page: Page, q: &str) {
        match page {
            Page::General => self.general(ui, q),
            Page::ClosedPill => self.closed_pill(ui, q),
            Page::Appearance => self.appearance(ui, q),
            Page::Music => self.music(ui, q),
            Page::Widgets => self.widgets(ui, q),
            Page::Clipboard => self.clipboard(ui, q),
            Page::Notifications => self.notifications(ui, q),
            Page::Games => self.games(ui, q),
            Page::Agents => self.agents(ui, q),
            Page::Shortcuts => self.shortcuts(ui, q),
            Page::About => self.about(ui, q),
        }
    }

    // ---- Closed pill --------------------------------------------------------------------------

    /// The Closed pill page: its resting face, then everything else that can appear on it.
    fn closed_pill(&mut self, ui: &mut Ui, q: &str) {
        self.resting_face(ui, q);
        self.on_the_pill(ui, q);
    }

    /// What the closed pill shows when nothing else is going on, and how its text looks.
    fn resting_face(&mut self, ui: &mut Ui, q: &str) {
        let page = Page::ClosedPill.title();
        let b = &mut self.draft.behavior;
        let face = &mut self.draft.appearance.resting_face;
        let ck = &mut self.draft.appearance.clock;
        let mut s = Section::new(ui, "Resting face", page, q);
        s.row(
            icon::HOME,
            "Shows",
            "What the pill shows when nothing else is going on",
            "resting face clock time date weather battery cpu pet",
            |ui| {
                let options: Vec<(RestingFace, String)> = RestingFace::ALL
                    .iter()
                    .map(|f| (*f, f.label().to_string()))
                    .collect();
                dropdown(ui, "face", face, &options, 180.0);
            },
        );
        let clock_face = *face == RestingFace::Time;
        s.row(
            icon::CALENDAR,
            "Clock",
            if clock_face {
                "The time alone, or with the weekday or date"
            } else {
                "Used when the pill shows the time"
            },
            "day date weekday",
            |ui| {
                segmented_enabled(
                    ui,
                    &mut b.pill_clock,
                    &[
                        (PillClock::Time, "Time"),
                        (PillClock::DayTime, "Day + time"),
                        (PillClock::DateTime, "Date + time"),
                    ],
                    clock_face,
                );
            },
        );
        s.row(
            icon::CLOCK,
            "Clock format",
            "How the time is written everywhere",
            "12 24 hour am pm",
            |ui| {
                segmented(
                    ui,
                    &mut b.clock_24h,
                    &[(false, "12-hour"), (true, "24-hour")],
                );
            },
        );
        s.row(
            icon::TEXT,
            "Text size",
            "The clock and the other text on the closed pill; the pill widens to fit",
            "font bigger smaller large",
            |ui| {
                let mut pct = ck.size * 100.0;
                if slider(
                    ui,
                    &mut pct,
                    CLOCK_SIZE_MIN * 100.0..=CLOCK_SIZE_MAX * 100.0,
                    5.0,
                    |v| format!("{v:.0}%"),
                ) {
                    ck.size = pct / 100.0;
                }
            },
        );
        s.row(
            icon::EDIT,
            "Bold text",
            "Heavier letters on the closed pill",
            "weight font",
            |ui| {
                toggle(ui, &mut ck.bold);
            },
        );
        s.row(
            icon::TIMER,
            "Show seconds",
            if clock_face {
                "14:05:09; the pill redraws once a second"
            } else {
                "Used when the pill shows the time"
            },
            "clock tick",
            |ui| {
                toggle_enabled(ui, &mut ck.seconds, clock_face);
            },
        );
        s.row(
            icon::CLOCK,
            "Leading zero",
            if clock_face {
                "09:05 instead of 9:05"
            } else {
                "Used when the pill shows the time"
            },
            "zero hour",
            |ui| {
                toggle_enabled(ui, &mut ck.leading_zero, clock_face);
            },
        );
        s.row(
            icon::PALETTE,
            "Text colour",
            "The surface's text colour, the accent colour or your own",
            "color colour font",
            |ui| {
                // Right to left: the colour of "Custom" first, then the choices.
                if ck.color == ClockColor::Custom {
                    if let Some(hex) = color_button(ui, &ck.custom_color, 0xFFFFFF) {
                        ck.custom_color = hex;
                    }
                    ui.add_space(8.0);
                }
                segmented(
                    ui,
                    &mut ck.color,
                    &[
                        (ClockColor::Text, "Surface"),
                        (ClockColor::Accent, "Accent"),
                        (ClockColor::Custom, "Custom"),
                    ],
                );
            },
        );
        s.end();
    }

    // ---- General ------------------------------------------------------------------------------

    /// Every switch for what the closed pill shows, in one place. The same settings also appear
    /// on their own pages (Music, Notifications); both edit the same values.
    fn on_the_pill(&mut self, ui: &mut Ui, q: &str) {
        let page = Page::ClosedPill.title();
        // Which apps hold the microphone now, read again every two seconds while shown.
        if self
            .mic_read
            .is_none_or(|at| at.elapsed() > std::time::Duration::from_secs(2))
        {
            self.mic_apps = sensors::read().mic;
            self.mic_read = Some(std::time::Instant::now());
        }
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_secs(2));

        let m = &mut self.draft.music;
        let c = &mut self.draft.calls;
        let b = &mut self.draft.behavior;
        let pop = &mut self.draft.popups;
        let mut s = Section::new(ui, "What else it shows", page, q);
        s.row(
            icon::MUSIC,
            "Playing song",
            "The cover, title and bars while music plays",
            "music song spotify youtube now playing show hide",
            |ui| {
                toggle(ui, &mut m.on_pill);
            },
        );
        let song = m.on_pill;
        s.row(
            icon::TEXT,
            "Song title",
            if song {
                "The title next to the cover; off shows only the cover and bars"
            } else {
                "Turn on Playing song first"
            },
            "music title name",
            |ui| {
                toggle_enabled(ui, &mut m.title_in_pill, song);
            },
        );
        s.row(
            icon::TIMER,
            "Call timer",
            "A timer while an app uses the microphone (a call, a voice chat)",
            "call meeting zoom teams discord voice microphone",
            |ui| {
                toggle(ui, &mut c.call_timer);
            },
        );
        let timer = c.call_timer;
        s.row(
            icon::EQUALIZER,
            "Music before calls",
            if !timer {
                "Turn on Call timer first"
            } else if song {
                "While music plays the song stays; the timer shows when it stops"
            } else {
                "Used when Playing song is on"
            },
            "discord voice chat priority music call",
            |ui| {
                toggle_enabled(ui, &mut c.music_first, timer && song);
            },
        );
        s.row(
            icon::MIC,
            "Microphone and camera dot",
            "Orange = microphone, green = camera",
            "privacy indicator sensor dot",
            |ui| {
                toggle(ui, &mut c.sensor_dot);
            },
        );
        let using: Vec<String> = self
            .mic_apps
            .iter()
            .filter(|app| !c.ignore_apps.iter().any(|i| app.contains(i.as_str())))
            .cloned()
            .collect();
        s.block(
            icon::MIC,
            "Using the microphone now",
            "Ignore an app to never show a timer or dot for it",
            "discord ignore microphone app exclude",
            |ui| {
                if using.is_empty() {
                    note(ui, "No app is using the microphone right now.");
                }
                for app in &using {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(app.as_str()).font(regular(13.5)));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if button(ui, icon::CANCEL, "Ignore").clicked()
                                && !c.ignore_apps.contains(app)
                            {
                                c.ignore_apps.push(app.clone());
                            }
                        });
                    });
                }
                if !c.ignore_apps.is_empty() {
                    note(
                        ui,
                        &format!(
                            "Ignored: {}. Remove them in Notifications > Calls.",
                            c.ignore_apps.join(", ")
                        ),
                    );
                }
            },
        );
        s.row(
            icon::LOCK,
            "Caps, Num and Scroll Lock",
            "A short notice when one changes",
            "lock keys keyboard notice",
            |ui| {
                toggle(ui, &mut pop.lock_keys);
            },
        );
        s.row(
            icon::BOLT,
            "Charger",
            "Plugged in, pulled out, or fully charged (laptops)",
            "device notice battery charger charging unplugged full power",
            |ui| {
                toggle(ui, &mut pop.charging);
            },
        );
        s.row(
            icon::WARNING,
            "Low battery",
            "A notice at 20% and at 10% (laptops)",
            "device notice battery empty power",
            |ui| {
                toggle(ui, &mut pop.low_battery);
            },
        );
        s.row(
            icon::HEADPHONES,
            "Headphones",
            "Connected or disconnected: Bluetooth, USB or the headphone jack",
            "device notice headset earbuds bluetooth audio",
            |ui| {
                toggle(ui, &mut pop.headphones);
            },
        );
        s.row(
            icon::DRIVE,
            "Drives",
            "A USB stick, disk or memory card plugged in or removed",
            "device notice usb stick disk sd card removed",
            |ui| {
                toggle(ui, &mut pop.drives);
            },
        );
        s.row(
            icon::GLOBE,
            "Internet",
            "The connection was lost, or is back. Reads Windows' own state; sends nothing",
            "device notice network wifi offline online connection",
            |ui| {
                toggle(ui, &mut pop.internet);
            },
        );
        let g = &mut self.draft.games;
        s.row(
            icon::GAME,
            "Game recap",
            "How long you played, when you quit a game",
            "play time summary quit game",
            |ui| {
                toggle(ui, &mut g.recap);
            },
        );
        s.row(
            icon::POWER,
            "Start-up greeting",
            "Slides in and says \"Pillbox is running\" after it starts, even when hidden",
            "startup greeting intro running",
            |ui| {
                toggle(ui, &mut b.start_greeting);
            },
        );
        s.end();
    }

    fn general(&mut self, ui: &mut Ui, q: &str) {
        let page = Page::General.title();
        let mut s = Section::new(ui, "Startup", page, q);
        let mut autostart = self.autostart;
        let mut failed = None;
        s.row(
            icon::POWER,
            "Start with Windows",
            "Open Pillbox when you sign in",
            "autostart boot login",
            |ui| {
                if toggle(ui, &mut autostart)
                    && let Err(e) = system::set_autostart(autostart)
                {
                    failed = Some(format!("{e:#}"));
                }
            },
        );
        s.end();
        if let Some(e) = failed {
            self.say(format!("Start with Windows: {e}"), false);
        } else {
            self.autostart = autostart;
        }

        let l = &mut self.draft.layout;
        let mut s = Section::new(ui, "Position and size", page, q);
        s.row(
            icon::FULLSCREEN,
            "Shape",
            "A floating pill, or a notch joined to the screen edge",
            "style notch pill pillbox",
            |ui| {
                segmented(
                    ui,
                    &mut l.style,
                    &[(Style::Pill, "Pill"), (Style::Notch, "Notch")],
                );
            },
        );
        s.row(
            icon::PIN,
            "Screen edge",
            "Where Pillbox sits",
            "side top left right position",
            |ui| {
                segmented(
                    ui,
                    &mut l.side,
                    &[
                        (Side::Top, "Top"),
                        (Side::Left, "Left"),
                        (Side::Right, "Right"),
                    ],
                );
            },
        );
        let monitors = &self.monitors;
        s.row(
            icon::MONITOR,
            "Display",
            "Which screen Pillbox appears on",
            "monitor screen",
            |ui| {
                let mut options = vec![("primary".to_string(), "Primary display".to_string())];
                for m in monitors {
                    let name = m.name.trim_start_matches(r"\\.\").to_string();
                    let primary = if m.primary { ", primary" } else { "" };
                    options.push((
                        m.name.clone(),
                        format!("{name} ({}×{}{primary})", m.rect.width(), m.rect.height()),
                    ));
                }
                if !options
                    .iter()
                    .any(|(v, _)| v.eq_ignore_ascii_case(&l.monitor))
                {
                    options.push((l.monitor.clone(), format!("{} (not connected)", l.monitor)));
                }
                dropdown(ui, "monitor", &mut l.monitor, &options, 220.0);
            },
        );
        let notch = l.style == Style::Notch;
        s.row(
            icon::SPEED,
            "Distance from the edge",
            if notch {
                "The notch always touches the edge"
            } else {
                "Move the pill closer to or farther from the edge"
            },
            "gap margin offset",
            |ui| {
                slider_enabled(
                    ui,
                    &mut l.distance,
                    0.0..=120.0,
                    1.0,
                    |v| format!("{v:.0} px"),
                    !notch,
                );
            },
        );
        s.row(
            icon::FULLSCREEN,
            "Size",
            "Scale the whole Pillbox, closed and open, text included",
            "scale bigger smaller zoom",
            |ui| {
                slider(ui, &mut l.size, SIZE_MIN..=SIZE_MAX, 0.05, |v| {
                    format!("{:.0}%", v * 100.0)
                });
            },
        );
        s.row(
            icon::EYE,
            "Hide until hover",
            "Slide off screen until the pointer touches the edge",
            "hidden auto hide",
            |ui| {
                toggle(ui, &mut l.hidden_until_hover);
            },
        );
        s.end();

        // What brings a hidden Pillbox into view; it hides again when the reason ends.
        let hidden = self.draft.layout.hidden_until_hover;
        let r = &mut self.draft.layout.reveal;
        let needs = "Turn on Hide until hover first";
        let mut s = Section::new(ui, "Hiding: show it again when", page, q);
        s.row(
            icon::MUSIC,
            "Music plays",
            if hidden {
                "Keep it in view while a song plays, or only for a moment when a song changes"
            } else {
                needs
            },
            "song spotify show reveal hidden",
            |ui| {
                segmented_enabled(
                    ui,
                    &mut r.music,
                    &[
                        (MusicReveal::Never, "Never"),
                        (MusicReveal::WhilePlaying, "While playing"),
                        (MusicReveal::OnSongChange, "New song"),
                    ],
                    hidden,
                );
            },
        );
        let timed = hidden && r.music == MusicReveal::OnSongChange;
        s.row(
            icon::TIMER,
            "A new song stays in view for",
            if timed {
                "Then it hides again"
            } else {
                "Used with Music plays: New song"
            },
            "seconds song change",
            |ui| {
                let mut v = r.song_seconds as f32;
                if slider_enabled(ui, &mut v, 2.0..=30.0, 1.0, |v| format!("{v:.0} s"), timed) {
                    r.song_seconds = v.round() as u32;
                }
            },
        );
        s.row(
            icon::BELL,
            "A notification arrives",
            if hidden {
                "Windows notifications mirrored on Pillbox"
            } else {
                needs
            },
            "toast alert stack",
            |ui| {
                toggle_enabled(ui, &mut r.notifications, hidden);
            },
        );
        s.row(
            icon::BOLT,
            "An agent or script flashes",
            if hidden {
                "Claude Code, Codex and pillboxctl flashes"
            } else {
                needs
            },
            "flash agent claude codex pillboxctl",
            |ui| {
                toggle_enabled(ui, &mut r.flashes, hidden);
            },
        );
        s.row(
            icon::MIC,
            "A call starts",
            if hidden {
                "An app uses the microphone"
            } else {
                needs
            },
            "meeting microphone timer",
            |ui| {
                toggle_enabled(ui, &mut r.calls, hidden);
            },
        );
        s.row(
            icon::CLOCK,
            "A reminder rings",
            if hidden { "Until you answer it" } else { needs },
            "alarm timer",
            |ui| {
                toggle_enabled(ui, &mut r.reminders, hidden);
            },
        );
        s.row(
            icon::LOCK,
            "A short notice shows",
            if hidden {
                "Lock keys, microphone muted, screenshot saved, colour copied"
            } else {
                needs
            },
            "caps lock num lock screenshot color picker",
            |ui| {
                toggle_enabled(ui, &mut r.notices, hidden);
            },
        );
        s.end();

        let b = &mut self.draft.behavior;
        let mut s = Section::new(ui, "Opening", page, q);
        s.row(
            icon::TILES,
            "Open on",
            "The tab the card shows when it opens",
            "reopen tab last music",
            |ui| {
                segmented(
                    ui,
                    &mut b.reopen_tab,
                    &[(ReopenTab::Last, "Last tab"), (ReopenTab::Music, "Music")],
                );
            },
        );
        s.row(
            icon::TIMER,
            "Open delay",
            "How long the pointer rests on the pill before it opens",
            "hover",
            |ui| {
                slider_u32(ui, &mut b.open_delay_ms, 0..=1500, 10, |v| {
                    format!("{v} ms")
                });
            },
        );
        s.row(
            icon::TIMER,
            "Close delay",
            "How long the card stays after the pointer leaves",
            "hover",
            |ui| {
                slider_u32(ui, &mut b.close_delay_ms, 0..=1500, 10, |v| {
                    format!("{v} ms")
                });
            },
        );
        s.row(
            icon::SPEED,
            "Animation speed",
            "How fast Pillbox opens, closes and changes shape",
            "motion fast slow",
            |ui| {
                slider(ui, &mut b.animation_speed, 0.5..=2.0, 0.05, |v| {
                    format!("{v:.2}×")
                });
            },
        );
        s.end();

        self.tabs(ui, q);
    }

    /// The card's tabs: their order and which ones show.
    fn tabs(&mut self, ui: &mut Ui, q: &str) {
        let page = Page::General.title();
        let music = self.draft.music.enabled;
        let clipboard = self.draft.clipboard.enabled;
        let script = self.draft.script.enabled;
        let notes = self.draft.notifications.enabled;
        // Why a tab that is switched on may still not be there.
        let why = |tab: Tab| match tab {
            Tab::Music if !music => "Music is off",
            Tab::Clipboard if !clipboard => "Clipboard history is off",
            Tab::Activity if !script => "Agents are off",
            Tab::Activity => "Shows once an agent has connected",
            Tab::Notifications if !notes => "Notifications are off",
            _ => "",
        };
        let t = &mut self.draft.tabs;
        let mut s = Section::new(ui, "Tabs", page, q);
        s.block(
            icon::LIST,
            "Tabs on the card",
            "In this order. Hiding a tab does not switch its feature off",
            "tab order reorder hide show icons home music clipboard shelf reminders controls system activity notifications",
            |ui| {
                let mut step: Option<(usize, usize)> = None;
                let count = t.order.len();
                for (i, tab) in t.order.clone().into_iter().enumerate() {
                    ui.horizontal(|ui| {
                        let mut on = !t.is_hidden(tab);
                        let ink = if on { TEXT } else { FAINT };
                        let (rect, _) =
                            ui.allocate_exact_size(vec2(26.0, 28.0), egui::Sense::hover());
                        ui.painter().text(
                            rect.center(),
                            Align2::CENTER_CENTER,
                            tab.icon(),
                            icon_font(14.0),
                            ink,
                        );
                        ui.label(
                            egui::RichText::new(tab.name())
                                .font(regular(13.5))
                                .color(ink),
                        );
                        if on && !why(tab).is_empty() {
                            ui.label(
                                egui::RichText::new(why(tab))
                                    .font(regular(12.0))
                                    .color(FAINT),
                            );
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if tab.can_hide() {
                                if toggle(ui, &mut on) {
                                    t.set_hidden(tab, !on);
                                }
                            } else {
                                // Home always shows.
                                toggle_enabled(ui, &mut on, false);
                            }
                            ui.add_space(8.0);
                            // Both arrows keep their place; a missing one leaves its gap.
                            let slot = 28.0 + ui.spacing().item_spacing.x;
                            if i + 1 < count {
                                if icon_button(ui, icon::DOWN, "Move down").clicked() {
                                    step = Some((i, i + 1));
                                }
                            } else {
                                ui.add_space(slot);
                            }
                            if i > 0 {
                                if icon_button(ui, icon::UP, "Move up").clicked() {
                                    step = Some((i, i - 1));
                                }
                            } else {
                                ui.add_space(slot);
                            }
                        });
                    });
                }
                if let Some((from, to)) = step {
                    t.order.swap(from, to);
                }
                if *t != TabsSettings::default() {
                    ui.add_space(4.0);
                    if button(ui, icon::RESET, "Show all in the normal order").clicked() {
                        *t = TabsSettings::default();
                    }
                }
            },
        );
        s.end();
    }

    // ---- Appearance ---------------------------------------------------------------------------

    fn appearance(&mut self, ui: &mut Ui, q: &str) {
        let page = Page::Appearance.title();
        let a = &mut self.draft.appearance;
        let accent = theme::parse_hex(&a.accent).unwrap_or(theme::Rgba::rgb(0x7C5CFF));
        let mut s = Section::new(ui, "Surface", page, q);
        s.block(
            icon::PALETTE,
            "Surface",
            "The material of the pill and the card",
            "theme look glass acrylic black graphite rose lavender peach sakura oled candy",
            |ui| {
                card_grid(ui, Surface::ALL.len(), 150.0, |ui, i, w| {
                    let surface = Surface::ALL[i];
                    let sub = if surface.is_glass() { "Glass" } else { "" };
                    if choice_card(
                        ui,
                        a.surface == surface,
                        vec2(w, 78.0),
                        surface.label(),
                        sub,
                        |p, r| {
                            paint_surface(p, r, surface, accent);
                        },
                    ) {
                        a.surface = surface;
                    }
                });
            },
        );
        s.end();

        let theme_now = a.app_theme;
        let mut s = Section::new(ui, "App theme", page, q);
        s.block(
            icon::PALETTE,
            "App theme",
            "Colours the pill and the card: one colour, or a fade of several. Your surface keeps its material",
            "colors colours tabs midnight ocean forest sunset violet ember mono fade gradient aurora golden hour deep sea neon mist lava cotton candy custom",
            |ui| {
                card_grid(ui, AppTheme::ALL.len(), 130.0, |ui, i, w| {
                    let app = AppTheme::ALL[i];
                    let t = Theme::from_appearance(&Appearance {
                        app_theme: app,
                        ..a.clone()
                    });
                    let sub = match app {
                        AppTheme::Default => "The surface's own",
                        AppTheme::Custom => "Your colours",
                        _ if t.motion.is_some() || fade_moves(app) => "Fade · moves",
                        _ if app.is_fade() => "Fade",
                        _ => "One colour",
                    };
                    let slot = format!("app-theme-{i}");
                    if choice_card(ui, theme_now == app, vec2(w, 70.0), app.label(), sub, |p, r| {
                        paint_app_theme(p, r, &t, &slot);
                    }) {
                        a.app_theme = app;
                    }
                });
            },
        );
        s.row(
            icon::SPEED,
            "Animate backgrounds",
            "Aurora and Lava drift slowly; Midnight neon shimmers now and then. They stand still while Pillbox is hidden, a game is fullscreen, or Windows animation effects are off",
            "motion animation move drift shimmer aurora lava neon",
            |ui| {
                toggle(ui, &mut a.animate);
            },
        );
        s.end();

        // What the pill and the card will look like, with this very theme (moving, if it
        // moves).
        let live = Theme::from_appearance(a);
        let mut s = Section::new(ui, "Preview", page, q);
        s.block(
            icon::EYE,
            "Preview",
            "The pill and the card as they will look",
            "preview look theme fade",
            |ui| {
                let clock = if live.motion.is_some() {
                    let now = ui.input(|i| i.time) as f32;
                    let wait = live.motion.map_or(1.0, |m| m.next_frame_in(now));
                    ui.ctx()
                        .request_repaint_after(std::time::Duration::from_secs_f32(
                            wait.clamp(0.02, 10.0),
                        ));
                    now
                } else {
                    0.0
                };
                paint_theme_preview(ui, &live, clock);
            },
        );
        s.end();

        self.fade_editor(ui, q);
        let a = &mut self.draft.appearance;

        let mut s = Section::new(ui, "Colour and glass", page, q);
        s.row(
            icon::PALETTE,
            "Accent colour",
            "Buttons, bars and highlights",
            "color colour",
            |ui| {
                // Right to left: the custom picker, then the swatches.
                if let Some(hex) = color_button(ui, &a.accent, 0x7C5CFF) {
                    a.accent = hex;
                }
                ui.add_space(6.0);
                for hex in ACCENTS.iter().rev() {
                    let chosen = a.accent.eq_ignore_ascii_case(hex);
                    let (rect, resp) =
                        ui.allocate_exact_size(vec2(24.0, 24.0), egui::Sense::click());
                    ui.painter()
                        .circle_filled(rect.center(), 10.0, hex_color(hex));
                    if chosen {
                        ui.painter().circle_stroke(
                            rect.center(),
                            12.0,
                            Stroke::new(2.0_f32, Color32::WHITE),
                        );
                    } else if resp.hovered() {
                        ui.painter()
                            .circle_stroke(rect.center(), 12.0, Stroke::new(1.0_f32, SUB));
                    }
                    if resp
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .clicked()
                    {
                        a.accent = (*hex).to_string();
                    }
                }
            },
        );
        let glass = a.surface.is_glass();
        s.row(
            icon::CLOUD,
            "Glass blur",
            if glass {
                "How strongly the screen behind the glass is blurred"
            } else {
                "Only for glass surfaces"
            },
            "frosted",
            |ui| {
                let mut pct = a.glass_blur * 100.0;
                if slider_enabled(
                    ui,
                    &mut pct,
                    0.0..=100.0,
                    1.0,
                    |v| format!("{v:.0}%"),
                    glass,
                ) {
                    a.glass_blur = pct / 100.0;
                }
            },
        );
        s.row(
            icon::SHIELD,
            "Hide from screenshots",
            if glass {
                "Always on for glass surfaces"
            } else {
                "Keep Pillbox out of screenshots and recordings"
            },
            "capture recording privacy obs stream",
            |ui| {
                if glass {
                    let mut on = true;
                    toggle_enabled(ui, &mut on, false);
                } else {
                    toggle(ui, &mut a.hide_from_capture);
                }
            },
        );
        s.end();
    }

    // ---- Music --------------------------------------------------------------------------------

    fn music(&mut self, ui: &mut Ui, q: &str) {
        let page = Page::Music.title();
        let m = &mut self.draft.music;
        let mut s = Section::new(ui, "Now playing", page, q);
        s.row(
            icon::MUSIC,
            "Show what's playing",
            "Cover, title, controls and the visualizer",
            "media song spotify",
            |ui| {
                toggle(ui, &mut m.enabled);
            },
        );
        let media_apps = &mut self.media_apps;
        let new_source = &mut self.new.source;
        s.block(
            icon::LIST,
            "Follow these apps",
            "Which players the music card follows",
            "source spotify browser player",
            |ui| {
                segmented(
                    ui,
                    &mut m.source_mode,
                    &[
                        (SourceMode::All, "Every app"),
                        (SourceMode::Only, "Only these"),
                        (SourceMode::Except, "All except these"),
                    ],
                );
                if m.source_mode != SourceMode::All {
                    chip_list(
                        ui,
                        "sources",
                        &mut m.source_apps,
                        new_source,
                        "Part of an app's name, e.g. spotify",
                        true,
                    );
                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            egui::RichText::new("Playing now:")
                                .font(regular(12.5))
                                .color(SUB),
                        );
                        if media_apps.is_empty() {
                            ui.label(
                                egui::RichText::new("nothing")
                                    .font(regular(12.5))
                                    .color(FAINT),
                            );
                        }
                        for app in media_apps.iter() {
                            let short =
                                app.rsplit(['!', '\\']).next().unwrap_or(app).to_lowercase();
                            let short = short.trim_end_matches(".exe").to_string();
                            let have = m.source_apps.contains(&short);
                            if button_enabled(ui, icon::ADD, &short, !have).clicked() && !have {
                                m.source_apps.push(short);
                            }
                        }
                        if icon_button(ui, icon::REFRESH, "Look again").clicked() {
                            *media_apps = media::session_app_ids();
                        }
                    });
                }
            },
        );
        s.end();

        let b = &mut self.draft.behavior;
        let mut s = Section::new(ui, "Pill", page, q);
        s.row(
            icon::TEXT,
            "Song title on the pill",
            "The pill widens to show up to 30 letters",
            "name",
            |ui| {
                toggle(ui, &mut m.title_in_pill);
            },
        );
        s.row(
            icon::PALETTE,
            "Album colours on the pill",
            "Colours from the cover flow slowly along the pill",
            "tint gradient cover color",
            |ui| {
                toggle(ui, &mut m.pill_colors);
            },
        );
        s.block(
            icon::EQUALIZER,
            "Visualizer",
            "The bars that move with the music, on the pill and the card",
            "bars spectrum audio",
            |ui| {
                const STYLES: [(Visualizer, &str); 7] = [
                    (Visualizer::PixelBars, "Pixel bars"),
                    (Visualizer::RoundedBars, "Rounded bars"),
                    (Visualizer::Equalizer, "Equalizer"),
                    (Visualizer::Burst, "Burst"),
                    (Visualizer::Wave, "Wave"),
                    (Visualizer::Dots, "Dots"),
                    (Visualizer::Off, "Off"),
                ];
                let accent = accent(ui);
                card_grid(ui, STYLES.len(), 112.0, |ui, i, w| {
                    let (style, name) = STYLES[i];
                    if choice_card(
                        ui,
                        b.visualizer == style,
                        vec2(w, 64.0),
                        name,
                        "",
                        |p, r| {
                            paint_visualizer(p, r, style, accent);
                        },
                    ) {
                        b.visualizer = style;
                    }
                });
            },
        );
        s.end();

        let mut s = Section::new(ui, "Music card", page, q);
        s.row(
            icon::PHOTO,
            "Album art background",
            "The blurred cover behind the music card and lyrics",
            "blur cover",
            |ui| {
                toggle(ui, &mut m.art_background);
            },
        );
        s.row(
            icon::EQUALIZER,
            "Reactive background",
            "The card's background glows in the cover's colours with the music, and flashes on each new lyric line",
            "glow colors mood audio visualizer lyrics",
            |ui| {
                toggle(ui, &mut m.reactive_background);
            },
        );
        s.block(
            icon::SPEED,
            "Progress bar",
            "How the song's progress is drawn",
            "seek position",
            |ui| {
                let accent = accent(ui);
                card_grid(ui, ProgressStyle::ALL.len(), 120.0, |ui, i, w| {
                    let style = ProgressStyle::ALL[i];
                    if choice_card(
                        ui,
                        m.progress_style == style,
                        vec2(w, 48.0),
                        style.label(),
                        "",
                        |p, r| {
                            paint_progress(p, r, style, accent);
                        },
                    ) {
                        m.progress_style = style;
                    }
                });
            },
        );
        s.row(
            icon::CHAT,
            "Synced lyrics",
            "Beside the player on the Music tab; scroll them with the wheel. Looks up each song's title, artist, album and length on lrclib.net",
            "lyrics karaoke network online",
            |ui| {
                toggle(ui, &mut m.lyrics);
            },
        );
        s.end();
    }

    // ---- Widgets ------------------------------------------------------------------------------

    fn widgets(&mut self, ui: &mut Ui, q: &str) {
        let page = Page::Widgets.title();
        let w = &mut self.draft.widgets;
        let mut s = Section::new(ui, "Home tab", page, q);
        s.block(
            icon::TILES,
            "Widgets",
            "Shown on the Home tab, in this order",
            "home tiles add remove order",
            |ui| {
                let mut action: Option<(usize, i32)> = None; // (index, -1 up / +1 down / 0 remove)
                let count = w.shown.len();
                for (i, kind) in w.shown.iter().enumerate() {
                    ui.horizontal(|ui| {
                        let (rect, _) =
                            ui.allocate_exact_size(vec2(26.0, 28.0), egui::Sense::hover());
                        ui.painter().text(
                            rect.center(),
                            Align2::CENTER_CENTER,
                            format!("{}", i + 1),
                            regular(12.5),
                            FAINT,
                        );
                        let online = if kind.online() { "  ·  online" } else { "" };
                        ui.label(
                            egui::RichText::new(format!("{}{online}", kind.label()))
                                .font(regular(13.5))
                                .color(TEXT),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if icon_button(ui, icon::DELETE, "Remove").clicked() {
                                action = Some((i, 0));
                            }
                            // Both arrows keep their place; a missing one leaves its gap.
                            let slot = 28.0 + ui.spacing().item_spacing.x;
                            if i + 1 < count {
                                if icon_button(ui, icon::DOWN, "Move down").clicked() {
                                    action = Some((i, 1));
                                }
                            } else {
                                ui.add_space(slot);
                            }
                            if i > 0 {
                                if icon_button(ui, icon::UP, "Move up").clicked() {
                                    action = Some((i, -1));
                                }
                            } else {
                                ui.add_space(slot);
                            }
                        });
                    });
                }
                match action {
                    Some((i, 0)) => {
                        w.shown.remove(i);
                    }
                    Some((i, -1)) if i > 0 => w.shown.swap(i, i - 1),
                    Some((i, 1)) if i + 1 < w.shown.len() => w.shown.swap(i, i + 1),
                    _ => {}
                }
                if w.shown.is_empty() {
                    note(ui, "No widgets: the Home tab is empty.");
                }
                let hidden: Vec<WidgetKind> = WidgetKind::ALL
                    .iter()
                    .copied()
                    .filter(|k| !w.shown.contains(k))
                    .collect();
                if !hidden.is_empty() {
                    ui.add_space(4.0);
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
                        for kind in hidden {
                            if button(ui, icon::ADD, kind.label()).clicked() {
                                w.shown.push(kind);
                            }
                        }
                    });
                }
            },
        );
        s.end();

        let new = &mut self.new;
        let mut s = Section::new(ui, "Widget options", page, q);
        s.row(
            icon::PALETTE,
            "Clock colour",
            "The Clock, World clock and Day clock widgets",
            "color colour",
            |ui| {
                if let Some(hex) = color_button(ui, &w.clock_color, 0xFFFFFF) {
                    w.clock_color = hex;
                }
            },
        );
        s.block(
            icon::GLOBE,
            "World clocks",
            "Time zones, e.g. Asia/Tokyo, America/New_York or Tokyo Standard Time",
            "timezone zone",
            |ui| {
                chip_list(
                    ui,
                    "zones",
                    &mut w.world_clocks,
                    &mut new.zone,
                    "Time zone",
                    false,
                );
            },
        );
        s.row(
            icon::TIMER,
            "Focus timer",
            "Length of one focus session",
            "pomodoro",
            |ui| {
                slider_u32(ui, &mut w.focus_minutes, 5..=120, 5, |v| format!("{v} min"));
            },
        );
        s.block(
            icon::CALENDAR,
            "Countdown",
            "Days left to a date",
            "event date",
            |ui| {
                ui.horizontal(|ui| {
                    field(ui, &mut w.countdown_name, "Name, e.g. Holiday", 220.0);
                    field(ui, &mut w.countdown_date, "YYYY-MM-DD", 130.0);
                });
                if !w.countdown_date.trim().is_empty()
                    && pillbox_core::calendar::parse_date(w.countdown_date.trim()).is_none()
                {
                    ui.label(
                        egui::RichText::new("Write the date as YYYY-MM-DD")
                            .font(regular(12.0))
                            .color(BAD),
                    );
                }
            },
        );
        s.row(
            icon::FOLDER,
            "Photo folder",
            "Where the Photos widget takes pictures from",
            "pictures images",
            |ui| {
                field(ui, &mut w.photos_folder, "Empty = Pictures", 260.0);
            },
        );
        s.row(
            icon::DIAGNOSTIC,
            "Network speed",
            "How network speed is written",
            "mbps megabytes",
            |ui| {
                segmented(
                    ui,
                    &mut w.network_mbps,
                    &[(true, "Mbit/s"), (false, "MB/s")],
                );
            },
        );
        s.end();

        let mut s = Section::new(ui, "Online widgets", page, q);
        s.row(
            icon::GLOBE,
            "Allow online widgets",
            "Ping, Website check, Currency rates and Air quality. Off = they never use the network",
            "internet network",
            |ui| {
                toggle(ui, &mut w.online);
            },
        );
        s.row(
            icon::DIAGNOSTIC,
            "Ping host",
            "The address the Ping widget measures",
            "latency",
            |ui| {
                field(ui, &mut w.ping_host, "e.g. 1.1.1.1", 200.0);
            },
        );
        s.block(
            icon::GLOBE,
            "Websites to check",
            "The Website check widget shows whether these answer",
            "uptime status",
            |ui| {
                chip_list(
                    ui,
                    "sites",
                    &mut w.websites,
                    &mut new.site,
                    "e.g. example.com",
                    false,
                );
            },
        );
        s.block(
            icon::SPEED,
            "Currency rates",
            "One unit of the first currency in each of the others",
            "exchange money eur usd",
            |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("1").font(regular(13.5)));
                    field(ui, &mut w.currency_base, "EUR", 70.0);
                    ui.label(egui::RichText::new("in").font(regular(13.5)).color(SUB));
                });
                chip_list(
                    ui,
                    "currencies",
                    &mut w.currencies,
                    &mut new.currency,
                    "Three letters, e.g. USD",
                    false,
                );
                for c in w.currencies.iter_mut() {
                    *c = c.trim().to_uppercase();
                }
            },
        );
        s.end();

        self.weather_section(ui, q);

        let w = &mut self.draft.widgets;
        let mut s = Section::new(ui, "System window", page, q);
        s.row(
            icon::DIAGNOSTIC,
            "System window",
            "CPU, GPU, RAM, disk, network and battery in a desktop window",
            "stats performance monitor",
            |ui| {
                toggle(ui, &mut w.system_window);
            },
        );
        s.row(
            icon::REFRESH,
            "Refresh",
            "How often the System tab and window read the numbers",
            "rate interval update",
            |ui| {
                slider_u32(ui, &mut w.system_refresh_ms, 250..=5000, 50, |v| {
                    if v < 1000 {
                        format!("{v} ms")
                    } else {
                        format!("{:.1} s", v as f32 / 1000.0)
                    }
                });
            },
        );
        s.end();
    }

    fn weather_section(&mut self, ui: &mut Ui, q: &str) {
        let page = Page::Widgets.title();
        let ctx = ui.ctx().clone();
        let mut search_now = false;
        let wt = &mut self.draft.weather;
        let city = &mut self.city;
        let mut s = Section::new(ui, "Weather", page, q);
        s.row(
            icon::SUN,
            "Weather",
            "Asks open-meteo.com for the weather at your location",
            "forecast temperature network",
            |ui| {
                toggle(ui, &mut wt.enabled);
            },
        );
        s.block(
            icon::LOCATION,
            "Location",
            "Searching sends only the name you type to open-meteo.com",
            "city place latitude longitude coordinates",
            |ui| {
                let set = wt.latitude != 0.0 || wt.longitude != 0.0;
                let current = if !set {
                    "Not set".to_string()
                } else if wt.place.is_empty() {
                    format!("{:.3}, {:.3}", wt.latitude, wt.longitude)
                } else {
                    format!("{}  ({:.2}, {:.2})", wt.place, wt.latitude, wt.longitude)
                };
                ui.label(
                    egui::RichText::new(current)
                        .font(semibold(13.5))
                        .color(if set { TEXT } else { FAINT }),
                );
                ui.horizontal(|ui| {
                    let resp = field(ui, &mut city.query, "City, e.g. Prague", 260.0);
                    let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    let busy = city.busy.is_some();
                    let label = if busy { "Searching…" } else { "Search" };
                    if (button_enabled(
                        ui,
                        icon::SEARCH,
                        label,
                        !busy && !city.query.trim().is_empty(),
                    )
                    .clicked()
                        || enter)
                        && !busy
                    {
                        search_now = true;
                    }
                });
                if let Some(m) = &city.message {
                    ui.label(egui::RichText::new(m).font(regular(12.5)).color(SUB));
                }
                let mut picked = None;
                for (i, place) in city.results.iter().enumerate() {
                    if button(ui, icon::LOCATION, &place.label).clicked() {
                        picked = Some(i);
                    }
                }
                if let Some(i) = picked
                    && let Some(place) = city.results.get(i)
                {
                    wt.latitude = place.latitude;
                    wt.longitude = place.longitude;
                    wt.place = place.label.clone();
                    city.results.clear();
                    city.query.clear();
                }
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("Or coordinates:")
                            .font(regular(12.5))
                            .color(SUB),
                    );
                    let lat = ui.add(
                        egui::DragValue::new(&mut wt.latitude)
                            .speed(0.01)
                            .range(-90.0..=90.0)
                            .max_decimals(3)
                            .prefix("lat "),
                    );
                    let lon = ui.add(
                        egui::DragValue::new(&mut wt.longitude)
                            .speed(0.01)
                            .range(-180.0..=180.0)
                            .max_decimals(3)
                            .prefix("lon "),
                    );
                    if lat.changed() || lon.changed() {
                        // Typed coordinates no longer belong to the searched place.
                        wt.place.clear();
                    }
                });
            },
        );
        s.row(
            icon::SUN,
            "Units",
            "Temperatures in Celsius or Fahrenheit",
            "celsius fahrenheit",
            |ui| {
                segmented(ui, &mut wt.fahrenheit, &[(false, "°C"), (true, "°F")]);
            },
        );
        let look = &mut self.draft.widgets.weather_look;
        s.row(
            icon::TILES,
            "Weather widget",
            "Just the temperature, or a few more lines",
            "look detailed minimal",
            |ui| {
                segmented(
                    ui,
                    look,
                    &[
                        (WeatherLook::Minimal, "Minimal"),
                        (WeatherLook::Detailed, "Detailed"),
                    ],
                );
            },
        );
        s.end();
        if search_now {
            self.start_city_search(&ctx);
        }
    }

    // ---- Clipboard & Shelf --------------------------------------------------------------------

    fn clipboard(&mut self, ui: &mut Ui, q: &str) {
        let page = Page::Clipboard.title();
        let c = &mut self.draft.clipboard;
        let new = &mut self.new;
        let mut s = Section::new(ui, "Clipboard history", page, q);
        s.row(
            icon::PASTE,
            "Keep clipboard history",
            "Encrypted on disk; copies marked private are skipped",
            "copy paste",
            |ui| {
                toggle(ui, &mut c.enabled);
            },
        );
        s.row(
            icon::LIST,
            "History length",
            "Older unpinned items are dropped",
            "max items count",
            |ui| {
                slider_u32(ui, &mut c.max_items, 10..=1000, 10, |v| {
                    format!("{v} items")
                });
            },
        );
        s.row(
            icon::HISTORY,
            "Delete copies after",
            "Takes effect within a minute. Pinned items and snippets stay",
            "expire delete old privacy days keep",
            |ui| {
                segmented(
                    ui,
                    &mut c.keep_days,
                    &[(1, "1 day"), (2, "2 days"), (7, "7 days"), (30, "30 days")],
                );
            },
        );
        s.row(
            icon::PHOTO,
            "Keep pictures",
            "The last 30 unpinned pictures",
            "images screenshots",
            |ui| {
                toggle(ui, &mut c.pictures);
            },
        );
        s.row(
            icon::TILES,
            "Two columns",
            "Show the list in two columns",
            "layout grid",
            |ui| {
                toggle(ui, &mut c.two_columns);
            },
        );
        s.block(
            icon::SHIELD,
            "Never record from these apps",
            "For apps that do not mark their copies as private, e.g. a password manager",
            "ignore exclude password",
            |ui| {
                chip_list(
                    ui,
                    "clip-ignore",
                    &mut c.ignore_apps,
                    &mut new.clip_ignore,
                    "Program name, e.g. keepass.exe",
                    true,
                );
            },
        );
        s.end();

        let mut s = Section::new(ui, "Snippets", page, q);
        s.block(
            icon::TEXT,
            "Snippets",
            "Saved text you paste often, on the Clipboard tab",
            "text templates",
            |ui| {
                let mut remove = None;
                for (i, sn) in c.snippets.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        ui.push_id(("snippet", i), |ui| {
                            field(ui, &mut sn.name, "Name", 140.0);
                            let wide = (ui.available_width() - 40.0).max(160.0);
                            field(ui, &mut sn.text, "Text", wide);
                            if icon_button(ui, icon::DELETE, "Remove").clicked() {
                                remove = Some(i);
                            }
                        });
                    });
                }
                if let Some(i) = remove {
                    c.snippets.remove(i);
                }
                if button(ui, icon::ADD, "Add snippet").clicked() {
                    c.snippets.push(Snippet::default());
                }
                note(ui, "A snippet with no text is not kept.");
            },
        );
        s.end();

        let shelf = &mut self.draft.shelf;
        let p = &mut self.draft.phone;
        let mut s = Section::new(ui, "Shelf and phone", page, q);
        s.row(
            icon::PHOTO,
            "Screenshots go on the shelf",
            "They are saved to Pictures\\Screenshots either way",
            "capture",
            |ui| {
                toggle(ui, &mut shelf.captures);
            },
        );
        s.row(
            icon::PHONE,
            "Send to and from your phone",
            "Phone buttons on the Shelf tab; a link lives only while its QR code shows",
            "mobile qr wifi transfer",
            |ui| {
                toggle(ui, &mut p.enabled);
            },
        );
        s.row(
            icon::FOLDER,
            "Save files from the phone in",
            "Empty = Downloads\\Pillbox",
            "receive folder",
            |ui| {
                field(ui, &mut p.receive_folder, "Downloads\\Pillbox", 260.0);
            },
        );
        s.row(
            icon::PASTE,
            "Put files from the phone on the shelf",
            "Text from the phone goes to the clipboard",
            "receive",
            |ui| {
                toggle(ui, &mut p.received_to_shelf);
            },
        );
        s.row(
            icon::TIMER,
            "QR code closes after",
            "The phone link stops working then",
            "minutes",
            |ui| {
                slider_u32(ui, &mut p.code_minutes, 1..=30, 1, |v| format!("{v} min"));
            },
        );
        s.end();
    }

    // ---- Notifications ------------------------------------------------------------------------

    fn notifications(&mut self, ui: &mut Ui, q: &str) {
        let page = Page::Notifications.title();
        let n = &mut self.draft.notifications;
        let new_app = &mut self.new.note_app;
        let mut s = Section::new(ui, "Windows notifications", page, q);
        s.row(
            icon::BELL,
            "Show notifications on Pillbox",
            "Reads the text of every notification on this PC; nothing leaves it",
            "mirror toast alert",
            |ui| {
                toggle(ui, &mut n.enabled);
            },
        );
        s.row(
            icon::TIMER,
            "New notifications stay for",
            "How long the Stack stays on the closed pill",
            "stack seconds",
            |ui| {
                slider_u32(ui, &mut n.stack_seconds, 1..=60, 1, |v| format!("{v} s"));
            },
        );
        s.block(
            icon::LIST,
            "Rules for single apps",
            "Hide an app's notifications, or list them without showing them on the pill",
            "mute block hide app",
            |ui| {
                let mut remove = None;
                for (i, rule) in n.rules.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        ui.add_sized(
                            [220.0, 28.0],
                            egui::Label::new(egui::RichText::new(&rule.app).font(regular(13.5)))
                                .truncate(),
                        );
                        dropdown(
                            ui,
                            &format!("note-rule-{i}"),
                            &mut rule.mode,
                            &[
                                (NoteMode::Hide, "Hide completely".to_string()),
                                (NoteMode::ListOnly, "List only, not on the pill".to_string()),
                            ],
                            200.0,
                        );
                        if icon_button(ui, icon::DELETE, "Remove").clicked() {
                            remove = Some(i);
                        }
                    });
                }
                if let Some(i) = remove {
                    n.rules.remove(i);
                }
                ui.horizontal(|ui| {
                    let resp = field(ui, new_app, "App name, as on its notifications", 260.0);
                    let app = new_app.trim().to_string();
                    let ok = !app.is_empty()
                        && !n.rules.iter().any(|r| r.app.eq_ignore_ascii_case(&app));
                    let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    if (button_enabled(ui, icon::ADD, "Add rule", ok).clicked() || enter) && ok {
                        n.rules.push(NoteRule {
                            app,
                            mode: NoteMode::Hide,
                        });
                        new_app.clear();
                    }
                });
            },
        );
        s.end();

        let c = &mut self.draft.calls;
        let new = &mut self.new.calls_ignore;
        let mut s = Section::new(ui, "Calls, microphone and camera", page, q);
        s.row(
            icon::TIMER,
            "Call timer",
            "A timer on the pill while any app uses the microphone",
            "meeting zoom teams discord",
            |ui| {
                toggle(ui, &mut c.call_timer);
            },
        );
        let timer = c.call_timer;
        s.row(
            icon::EQUALIZER,
            "Music before calls",
            "While a song is on the pill it stays; the timer shows when the music stops",
            "discord voice chat priority music call",
            |ui| {
                toggle_enabled(ui, &mut c.music_first, timer);
            },
        );
        s.row(
            icon::MIC,
            "Microphone and camera dot",
            "Orange = microphone, green = camera",
            "privacy indicator sensor",
            |ui| {
                toggle(ui, &mut c.sensor_dot);
            },
        );
        s.block(
            icon::CAMERA,
            "Ignore these apps",
            "Their microphone and camera use shows no dot and no timer",
            "exclude",
            |ui| {
                chip_list(
                    ui,
                    "calls-ignore",
                    &mut c.ignore_apps,
                    new,
                    "Part of a name, e.g. discord",
                    true,
                );
            },
        );
        s.end();

        let r = &mut self.draft.reminders;
        let mut s = Section::new(ui, "Reminders", page, q);
        s.row(
            icon::BELL,
            "Chime until answered",
            "Stops when you press Snooze or Done",
            "sound alarm",
            |ui| {
                toggle(ui, &mut r.chime);
            },
        );
        s.row(
            icon::TIMER,
            "Snooze for",
            "How long Snooze puts a reminder off",
            "minutes",
            |ui| {
                slider_u32(ui, &mut r.snooze_minutes, 1..=60, 1, |v| format!("{v} min"));
            },
        );
        s.end();
    }

    // ---- Games & Apps -------------------------------------------------------------------------

    fn games(&mut self, ui: &mut Ui, q: &str) {
        let page = Page::Games.title();
        let g = &mut self.draft.games;
        let new = &mut self.new.games_ignore;
        let mut s = Section::new(ui, "Fullscreen games", page, q);
        s.row(
            icon::GAME,
            "React to fullscreen games",
            "Borderless or windowed fullscreen",
            "gaming",
            |ui| {
                toggle(ui, &mut g.enabled);
            },
        );
        s.row(
            icon::STRIP,
            "Over a game",
            "A slim strip of live stats, or nothing at all",
            "strip hide fps",
            |ui| {
                segmented(
                    ui,
                    &mut g.behavior,
                    &[
                        (GameBehavior::Strip, "Stats strip"),
                        (GameBehavior::Hide, "Hide"),
                    ],
                );
            },
        );
        s.row(
            icon::FULLSCREEN,
            "Every fullscreen app counts",
            "Also browsers, video players and slide shows",
            "video youtube",
            |ui| {
                toggle(ui, &mut g.every_fullscreen_app);
            },
        );
        s.row(
            icon::PALETTE,
            "Strip background",
            "Pillbox's surface, or just the numbers",
            "transparent",
            |ui| {
                segmented(
                    ui,
                    &mut g.strip_background,
                    &[
                        (StripBackground::Surface, "Surface"),
                        (StripBackground::NumbersOnly, "Numbers only"),
                    ],
                );
            },
        );
        s.row(
            icon::DIAGNOSTIC,
            "Usage bars",
            "Small bars under CPU and RAM on the strip",
            "cpu ram",
            |ui| {
                toggle(ui, &mut g.usage_bars);
            },
        );
        s.block(
            icon::SHIELD,
            "Never treat these as games",
            "On top of the built-in list",
            "exclude ignore",
            |ui| {
                chip_list(
                    ui,
                    "games-ignore",
                    &mut g.ignore_apps,
                    new,
                    "Program name, e.g. kodi.exe",
                    true,
                );
            },
        );
        s.end();

        let rules = &mut self.draft.app_rules;
        let open_apps = &mut self.open_apps;
        let new_exe = &mut self.new.rule_exe;
        let mut s = Section::new(ui, "Rules for single apps", page, q);
        s.block(
            icon::LIST,
            "While an app is in front",
            "Hide Pillbox, shrink it to a strip, or always show it",
            "rule exe program hide strip show",
            |ui| {
                let mut remove = None;
                for (i, rule) in rules.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        ui.add_sized(
                            [220.0, 28.0],
                            egui::Label::new(egui::RichText::new(&rule.exe).font(regular(13.5)))
                                .truncate(),
                        );
                        dropdown(
                            ui,
                            &format!("app-rule-{i}"),
                            &mut rule.behavior,
                            &[
                                (RuleBehavior::Hide, "Hide Pillbox".to_string()),
                                (RuleBehavior::Strip, "Shrink to a strip".to_string()),
                                (RuleBehavior::Show, "Always show".to_string()),
                            ],
                            180.0,
                        );
                        if icon_button(ui, icon::DELETE, "Remove").clicked() {
                            remove = Some(i);
                        }
                    });
                }
                if let Some(i) = remove {
                    rules.remove(i);
                }
                if rules.is_empty() {
                    note(ui, "No rules yet.");
                }
                ui.horizontal(|ui| {
                    let mut options: Vec<(String, String)> =
                        open_apps.iter().map(|a| (a.clone(), a.clone())).collect();
                    options.insert(0, (String::new(), "Pick an open app".to_string()));
                    if !options.iter().any(|(v, _)| v == new_exe) {
                        options.push((new_exe.clone(), new_exe.clone()));
                    }
                    dropdown(ui, "open-apps", new_exe, &options, 200.0);
                    field(ui, new_exe, "or type, e.g. game.exe", 170.0);
                    let exe = new_exe.trim().to_lowercase();
                    let ok = !exe.is_empty() && !rules.iter().any(|r| r.exe == exe);
                    if button_enabled(ui, icon::ADD, "Add rule", ok).clicked() && ok {
                        rules.push(AppRule {
                            exe,
                            behavior: RuleBehavior::Hide,
                        });
                        new_exe.clear();
                    }
                    if icon_button(ui, icon::REFRESH, "Look again").clicked() {
                        *open_apps = foreground::open_apps();
                    }
                });
            },
        );
        s.end();
    }

    // ---- Agents -------------------------------------------------------------------------------

    fn agents(&mut self, ui: &mut Ui, q: &str) {
        let page = Page::Agents.title();
        let sc = &mut self.draft.script;
        let mut s = Section::new(ui, "Agents", page, q);
        s.row(
            icon::ROBOT,
            "Activity tab and script API",
            "The tab shows once an agent connects. Local only (127.0.0.1), with a token",
            "claude codex pillboxctl hooks status",
            |ui| {
                toggle(ui, &mut sc.enabled);
            },
        );
        for (key, name) in [("claude", "Claude Code"), ("codex", "Codex")] {
            let mut shown = !sc.hidden_agents.iter().any(|h| h == key);
            s.row(
                icon::ROBOT,
                &format!("Show {name}"),
                "Its Sessions on the Activity tab and its Flashes",
                "agent hide",
                |ui| {
                    if toggle(ui, &mut shown) {
                        sc.hidden_agents.retain(|h| h != key);
                        if !shown {
                            sc.hidden_agents.push(key.to_string());
                        }
                    }
                },
            );
        }
        s.row(
            icon::BOLT,
            "Flash when an agent finishes or needs you",
            "\"Claude Code is done\", \"Codex needs you\"",
            "flash notify",
            |ui| {
                toggle(ui, &mut sc.agent_flashes);
            },
        );
        s.row(
            icon::SHIELD,
            "Answer permission questions",
            "Allow / Deny buttons on Pillbox (needs: pillboxctl hooks claude --reply)",
            "allow deny approve",
            |ui| {
                toggle(ui, &mut sc.answer_permissions);
            },
        );
        s.row(
            icon::DIAGNOSTIC,
            "Claude usage limits",
            "Asks Anthropic once a minute with your Claude Code sign-in, for the Claude card",
            "limits quota account anthropic",
            |ui| {
                toggle(ui, &mut sc.claude_account_usage);
            },
        );
        s.end();

        let searching = !q.is_empty();
        if !searching {
            ui.add_space(10.0);
            let label = if self.show_timing {
                "Hide timing"
            } else {
                "Show timing"
            };
            let glyph = if self.show_timing {
                icon::UP
            } else {
                icon::DOWN
            };
            if button(ui, glyph, label).clicked() {
                self.show_timing = !self.show_timing;
            }
        }
        if self.show_timing || searching {
            let sc = &mut self.draft.script;
            let mut s = Section::new(ui, "Timing", page, q);
            s.row(
                icon::BOLT,
                "A Flash stays on the pill for",
                "",
                "seconds",
                |ui| {
                    slider_u32(ui, &mut sc.flash_seconds, 1..=60, 1, |v| format!("{v} s"));
                },
            );
            s.row(
                icon::HISTORY,
                "Finished work stays on the Activity tab for",
                "",
                "keep minutes",
                |ui| {
                    slider_u32(ui, &mut sc.keep_minutes, 5..=1440, 5, |v| {
                        if v < 120 {
                            format!("{v} min")
                        } else {
                            format!("{:.1} h", v as f32 / 60.0)
                        }
                    });
                },
            );
            s.row(
                icon::TIMER,
                "A silent working agent shows as idle after",
                "",
                "stale minutes",
                |ui| {
                    slider_u32(ui, &mut sc.stale_minutes, 2..=240, 1, |v| {
                        format!("{v} min")
                    });
                },
            );
            s.row(
                icon::SHIELD,
                "A permission question waits for",
                "Then the agent asks in its own window",
                "answer seconds",
                |ui| {
                    slider_u32(ui, &mut sc.answer_seconds, 5..=600, 5, |v| format!("{v} s"));
                },
            );
            s.end();
        }
    }

    // ---- Shortcuts ----------------------------------------------------------------------------

    fn shortcuts(&mut self, ui: &mut Ui, q: &str) {
        let page = Page::Shortcuts.title();
        let h = &mut self.draft.hotkeys;
        let mut s = Section::new(ui, "Global shortcuts", page, q);
        let rows: [(&str, &str, &str, &mut String); 6] = [
            (
                icon::FULLSCREEN,
                "Open or close the card",
                "toggle",
                &mut h.toggle_pillbox,
            ),
            (icon::MUSIC, "Play / pause", "media", &mut h.play_pause),
            (icon::MIC, "Mute microphone", "mic", &mut h.mute_mic),
            (icon::EYE, "Hide or show Pillbox", "hide", &mut h.hide),
            (icon::PHOTO, "Screenshot", "capture", &mut h.screenshot),
            (
                icon::PALETTE,
                "Colour picker",
                "color eyedropper",
                &mut h.color_picker,
            ),
        ];
        for (glyph, title, keys, value) in rows {
            s.row(
                glyph,
                title,
                "",
                &format!("{keys} hotkey keyboard shortcut"),
                |ui| {
                    hotkey_recorder(ui, title, value);
                },
            );
        }
        s.end();
        if q.is_empty() {
            note(
                ui,
                "Click a shortcut and press the keys. Esc cancels, Backspace removes it. Keys another app (or Pillbox itself) already uses never reach this window, so they cannot be recorded.",
            );
        }

        let slots = &mut self.draft.launcher.slots;
        let mut s = Section::new(ui, "Launcher", page, q);
        s.block(icon::LAUNCH, "Favourites", "Up to six apps, files, folders or links on the Controls tab, each with its own shortcut", "launch app program open slot", |ui| {
            let mut remove = None;
            for (i, slot) in slots.iter_mut().enumerate() {
                ui.push_id(("slot", i), |ui| {
                    ui.horizontal(|ui| {
                        field(ui, &mut slot.name, "Name", 120.0);
                        let wide = (ui.available_width() - 40.0).max(160.0);
                        field(ui, &mut slot.path, "Program, file, folder or link", wide);
                        if icon_button(ui, icon::DELETE, "Remove").clicked() {
                            remove = Some(i);
                        }
                    });
                    ui.horizontal(|ui| {
                        field(ui, &mut slot.args, "Arguments (optional)", 200.0);
                        hotkey_recorder(ui, &format!("slot-{i}"), &mut slot.hotkey);
                    });
                    ui.add_space(4.0);
                });
            }
            if let Some(i) = remove {
                slots.remove(i);
            }
            if slots.len() < LAUNCH_SLOTS && button(ui, icon::ADD, "Add favourite").clicked() {
                slots.push(LaunchSlot::default());
            }
            note(ui, "A favourite with no path is not kept.");
        });
        s.end();
    }

    // ---- About --------------------------------------------------------------------------------

    fn about(&mut self, ui: &mut Ui, q: &str) {
        let page = Page::About.title();
        let mut open_folder = false;
        let mut reset = false;
        let confirm = &mut self.confirm_reset;
        let mut s = Section::new(ui, "Pillbox", page, q);
        s.row(
            icon::INFO,
            "Version",
            "A floating pill for Windows 10 and 11",
            "about",
            |ui| {
                ui.label(
                    egui::RichText::new(env!("CARGO_PKG_VERSION"))
                        .font(semibold(13.5))
                        .color(SUB),
                );
            },
        );
        s.row(
            icon::SHIELD,
            "Licence and terms",
            "Free to use, no copying. Copyright (c) 2026 David Sejkora. LICENSE.txt, TERMS.txt and THIRD-PARTY-NOTICES.txt are in Pillbox's install folder",
            "licence license terms copyright legal notices",
            |_| {},
        );
        s.row(
            icon::CLOUD,
            "Weather data by Open-Meteo.com",
            "Weather, air quality and place search, under the CC BY 4.0 licence",
            "credits attribution open-meteo weather",
            |_| {},
        );
        s.row(
            icon::FOLDER,
            "Settings folder",
            "settings.json and Pillbox's data; edits there apply at once",
            "file json data",
            |ui| {
                if button(ui, icon::BROWSE, "Open folder").clicked() {
                    open_folder = true;
                }
            },
        );
        s.row(
            icon::RESET,
            "Reset all settings",
            "Back to how Pillbox first started. Clipboard history, reminders and the shelf are kept",
            "default restore",
            |ui| {
                if *confirm {
                    // Right to left: Reset is the rightmost.
                    let (rect, resp) =
                        ui.allocate_exact_size(vec2(84.0, 32.0), egui::Sense::click());
                    let fill = if resp.hovered() {
                        Color32::from_rgb(0xD9, 0x3A, 0x2B)
                    } else {
                        Color32::from_rgb(0xC4, 0x2B, 0x1C)
                    };
                    ui.painter().rect_filled(rect, 8.0, fill);
                    ui.painter().text(
                        rect.center(),
                        Align2::CENTER_CENTER,
                        "Reset",
                        semibold(13.0),
                        Color32::WHITE,
                    );
                    if resp
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .clicked()
                    {
                        reset = true;
                        *confirm = false;
                    }
                    if button(ui, "", "Cancel").clicked() {
                        *confirm = false;
                    }
                } else if button(ui, icon::RESET, "Reset…").clicked() {
                    *confirm = true;
                }
            },
        );
        s.end();
        if open_folder {
            match crate::store::dir() {
                Ok(dir) => {
                    if let Err(e) = system::open_path(&dir.to_string_lossy(), "") {
                        self.say(format!("{e:#}"), false);
                    }
                }
                Err(e) => self.say(format!("{e:#}"), false),
            }
        }
        if reset {
            self.draft = pillbox_core::settings::Settings::default();
            self.save();
            self.say("Settings reset", true);
        }
    }
}

// ---- previews ---------------------------------------------------------------------------------

/// A pill on a two-tone "wallpaper" in the colours a surface gives it.
fn paint_surface(p: &egui::Painter, r: Rect, surface: Surface, accent: theme::Rgba) {
    let top = CornerRadius {
        nw: 9,
        ne: 9,
        sw: 0,
        se: 0,
    };
    let bottom = CornerRadius {
        nw: 0,
        ne: 0,
        sw: 9,
        se: 9,
    };
    let mid = r.center().y;
    p.rect_filled(
        Rect::from_min_max(r.min, Pos2::new(r.max.x, mid)),
        top,
        Color32::from_rgb(0x3A, 0x4B, 0x7A),
    );
    p.rect_filled(
        Rect::from_min_max(Pos2::new(r.min.x, mid), r.max),
        bottom,
        Color32::from_rgb(0x6B, 0x4A, 0x7E),
    );
    let t = Theme::for_surface(surface, accent);
    let pill = Rect::from_center_size(r.center(), vec2((r.width() * 0.62).min(120.0), 30.0));
    match &t.glass {
        Some(g) => {
            let mix = |a: theme::Rgba, b: theme::Rgba| {
                theme::Rgba::new(
                    (a.r + b.r) / 2.0,
                    (a.g + b.g) / 2.0,
                    (a.b + b.b) / 2.0,
                    (a.a + b.a) / 2.0,
                )
            };
            let tint = mix(g.tint_top, g.tint_bottom);
            p.rect_filled(
                pill,
                15.0,
                Color32::from_rgba_unmultiplied(0xB0, 0xB8, 0xD8, 70),
            );
            p.rect_filled(pill, 15.0, to_color32(tint));
            p.rect_stroke(
                pill,
                15.0,
                Stroke::new(1.0_f32, Color32::from_white_alpha(110)),
                StrokeKind::Inside,
            );
        }
        None => {
            let mix = |a: theme::Rgba, b: theme::Rgba| {
                theme::Rgba::new((a.r + b.r) / 2.0, (a.g + b.g) / 2.0, (a.b + b.b) / 2.0, 1.0)
            };
            p.rect_filled(pill, 15.0, to_color32(mix(t.fill_top, t.fill_bottom)));
            if let Some(rim) = t.rim {
                p.rect_stroke(
                    pill,
                    15.0,
                    Stroke::new(1.0_f32, to_color32(rim)),
                    StrokeKind::Inside,
                );
            }
        }
    }
    p.text(
        Pos2::new(pill.left() + 14.0, pill.center().y),
        Align2::LEFT_CENTER,
        "9:41",
        semibold(12.0),
        to_color32(t.text),
    );
    p.circle_filled(
        Pos2::new(pill.right() - 14.0, pill.center().y),
        3.5,
        to_color32(t.accent),
    );
}

/// A still frame of a visualizer style.
fn paint_visualizer(p: &egui::Painter, r: Rect, style: Visualizer, accent: Color32) {
    const LEVELS: [f32; 6] = [0.55, 0.9, 0.7, 1.0, 0.45, 0.75];
    let (bar, gap) = (6.0, 4.0);
    let width = 6.0 * bar + 5.0 * gap;
    let left = r.center().x - width / 2.0;
    let (cy, h) = (r.center().y, r.height() * 0.6);
    let x_of = |i: usize| left + i as f32 * (bar + gap);
    match style {
        Visualizer::Off => {
            p.text(r.center(), Align2::CENTER_CENTER, "—", regular(16.0), FAINT);
        }
        Visualizer::RoundedBars => {
            for (i, l) in LEVELS.iter().enumerate() {
                let bh = (l * h).max(bar);
                p.rect_filled(
                    Rect::from_min_size(Pos2::new(x_of(i), cy - bh / 2.0), vec2(bar, bh)),
                    3.0,
                    accent,
                );
            }
        }
        Visualizer::Equalizer => {
            let floor = cy + h / 2.0;
            for (i, l) in LEVELS.iter().enumerate() {
                let bh = (l * h).max(bar);
                p.rect_filled(
                    Rect::from_min_size(Pos2::new(x_of(i), floor - bh), vec2(bar, bh)),
                    3.0,
                    accent,
                );
            }
        }
        Visualizer::Burst => {
            const ORDER: [usize; 6] = [4, 2, 0, 1, 3, 5];
            const SORTED: [f32; 6] = [1.0, 0.9, 0.75, 0.7, 0.55, 0.45];
            for (i, band) in ORDER.iter().enumerate() {
                let bh = (SORTED[*band] * h).max(bar);
                p.rect_filled(
                    Rect::from_min_size(Pos2::new(x_of(i), cy - bh / 2.0), vec2(bar, bh)),
                    3.0,
                    accent,
                );
            }
        }
        Visualizer::PixelBars => {
            let step = bar + 2.0;
            let rows = (h / step).floor().max(1.0) as usize;
            let bottom = cy + (rows as f32 * step) / 2.0;
            for (i, l) in LEVELS.iter().enumerate() {
                let lit = 1 + (l * (rows - 1) as f32).round() as usize;
                for row in 0..lit.min(rows) {
                    let y = bottom - row as f32 * step;
                    p.rect_filled(
                        Rect::from_min_size(Pos2::new(x_of(i), y - bar), vec2(bar, bar)),
                        1.0,
                        accent,
                    );
                }
            }
        }
        Visualizer::Wave => {
            let n = width as usize;
            for c in 0..n {
                let x = c as f32 / (n - 1) as f32;
                let pos = x * 5.0;
                let i = (pos as usize).min(4);
                let f = pos - i as f32;
                let f = (1.0 - (f * std::f32::consts::PI).cos()) / 2.0;
                let level = LEVELS[i] + (LEVELS[i + 1] - LEVELS[i]) * f;
                let taper = (x * std::f32::consts::PI).sin().max(0.0).sqrt();
                let amp = (level * taper * h / 2.0).max(1.0);
                let px = left + c as f32;
                p.rect_filled(
                    Rect::from_min_max(Pos2::new(px, cy - amp), Pos2::new(px + 1.0, cy + amp)),
                    0.0,
                    accent,
                );
            }
        }
        Visualizer::Dots => {
            for (i, l) in LEVELS.iter().enumerate() {
                let rad = bar * (0.45 + 0.35 * l);
                let y = cy + (h / 2.0 - rad) * (1.0 - 2.0 * l);
                p.circle_filled(Pos2::new(x_of(i) + bar / 2.0, y), rad, accent);
            }
        }
    }
}

/// A still frame of a progress bar style, 40% played.
fn paint_progress(p: &egui::Painter, r: Rect, style: ProgressStyle, accent: Color32) {
    let track = Color32::from_rgb(0x48, 0x48, 0x50);
    let (left, right) = (r.left() + 14.0, r.right() - 14.0);
    let played = left + (right - left) * 0.4;
    let cy = r.center().y;
    match style {
        ProgressStyle::Line => {
            p.rect_filled(
                Rect::from_min_max(Pos2::new(left, cy - 1.5), Pos2::new(right, cy + 1.5)),
                1.5,
                track,
            );
            p.rect_filled(
                Rect::from_min_max(Pos2::new(left, cy - 1.5), Pos2::new(played, cy + 1.5)),
                1.5,
                accent,
            );
        }
        ProgressStyle::Bar => {
            p.rect_filled(
                Rect::from_min_max(Pos2::new(left, cy - 4.0), Pos2::new(right, cy + 4.0)),
                4.0,
                track,
            );
            p.rect_filled(
                Rect::from_min_max(Pos2::new(left, cy - 4.0), Pos2::new(played, cy + 4.0)),
                4.0,
                accent,
            );
        }
        ProgressStyle::Dots => {
            let n = 14;
            for i in 0..n {
                let x = left + (right - left) * i as f32 / (n - 1) as f32;
                let c = if x <= played { accent } else { track };
                p.circle_filled(Pos2::new(x, cy), 2.5, c);
            }
        }
        ProgressStyle::Wave => {
            let mut x = left;
            while x < played {
                let y = cy + ((x - left) / 5.0).sin() * 4.0;
                p.circle_filled(Pos2::new(x, y), 1.4, accent);
                x += 1.0;
            }
            p.rect_filled(
                Rect::from_min_max(Pos2::new(played, cy - 1.5), Pos2::new(right, cy + 1.5)),
                1.5,
                track,
            );
        }
        ProgressStyle::Segments => {
            let n = 10;
            let gap = 3.0;
            let w = ((right - left) - gap * (n - 1) as f32) / n as f32;
            for i in 0..n {
                let x = left + i as f32 * (w + gap);
                let c = if x + w / 2.0 <= played { accent } else { track };
                p.rect_filled(
                    Rect::from_min_size(Pos2::new(x, cy - 3.0), vec2(w, 6.0)),
                    2.0,
                    c,
                );
            }
        }
    }
}

/// Does this app theme move when animation is allowed (whatever the switch says now)?
fn fade_moves(app: AppTheme) -> bool {
    Theme::from_appearance(&Appearance {
        app_theme: app,
        animate: true,
        ..Appearance::default()
    })
    .motion
    .is_some()
}

impl SettingsApp {
    /// The user's own fade: colours, where they sit, direction, softness and strength. Any
    /// change here picks "Your fade" as the app theme.
    fn fade_editor(&mut self, ui: &mut Ui, q: &str) {
        let page = Page::Appearance.title();
        let a = &mut self.draft.appearance;
        let mut changed = false;
        let mut s = Section::new(ui, "Your fade", page, q);
        // A built-in fade can be the starting point.
        let current = Theme::from_appearance(&Appearance {
            animate: false,
            ..a.clone()
        });
        let copyable = a.app_theme.is_fade() && a.app_theme != AppTheme::Custom;
        s.row(
            icon::EDIT,
            "Start from the current theme",
            if copyable {
                "Copy its colours here to change them"
            } else {
                "Pick a fade theme above to copy its colours"
            },
            "copy fade gradient edit",
            |ui| {
                if button_enabled(ui, icon::EDIT, "Edit a copy", copyable).clicked()
                    && let Some(f) = current.fade
                {
                    a.fade.stops = f
                        .stops()
                        .iter()
                        .map(|s| FadeStop {
                            color: theme::to_hex(s.color),
                            at: s.at,
                        })
                        .collect();
                    a.fade.angle = f.angle;
                    a.fade.softness = f.softness;
                    a.fade.opacity = 1.0;
                    changed = true;
                }
            },
        );
        s.row(
            icon::PALETTE,
            "Colours",
            "How many colours the fade runs through",
            "fade gradient colors colours stops two three",
            |ui| {
                let mut n = a.fade.stops.len().clamp(FADE_STOPS_MIN, FADE_STOPS_MAX);
                if segmented(ui, &mut n, &[(2, "2"), (3, "3")]) {
                    let fade = &mut a.fade;
                    if n == 3 && fade.stops.len() == 2 {
                        // The new middle colour starts as the blend of the two ends.
                        let mix = match (
                            theme::parse_hex(&fade.stops[0].color),
                            theme::parse_hex(&fade.stops[1].color),
                        ) {
                            (Some(x), Some(y)) => theme::to_hex(x.lerp(y, 0.5)),
                            _ => fade.stops[0].color.clone(),
                        };
                        let at = (fade.stops[0].at + fade.stops[1].at) / 2.0;
                        fade.stops.insert(1, FadeStop { color: mix, at });
                    } else if n == 2 && fade.stops.len() == 3 {
                        fade.stops.remove(1);
                    }
                    changed = true;
                }
            },
        );
        let count = a.fade.stops.len();
        for i in 0..count {
            let title = match (i, count) {
                (0, _) => "First colour",
                (1, 3) => "Middle colour",
                _ => "Last colour",
            };
            s.row(
                icon::PALETTE,
                title,
                "Its colour, and where along the fade it is at full strength",
                "fade gradient color colour position stop",
                |ui| {
                    // Kept between its neighbours, so the colours stay in order.
                    let lo = if i == 0 { 0.0 } else { a.fade.stops[i - 1].at };
                    let hi = a.fade.stops.get(i + 1).map_or(1.0, |n| n.at);
                    let mut pct = a.fade.stops[i].at * 100.0;
                    if slider(ui, &mut pct, 0.0..=100.0, 1.0, |v| format!("{v:.0}%")) {
                        a.fade.stops[i].at = (pct / 100.0).clamp(lo, hi);
                        changed = true;
                    }
                    ui.add_space(8.0);
                    if let Some(hex) = color_button(ui, &a.fade.stops[i].color, 0x7C5CFF) {
                        a.fade.stops[i].color = hex;
                        changed = true;
                    }
                },
            );
        }
        s.row(
            icon::REFRESH,
            "Direction",
            "Across, down, corner to corner, or any angle",
            "fade gradient direction angle vertical horizontal diagonal",
            |ui| {
                let mut deg = a.fade.angle;
                if slider(ui, &mut deg, 0.0..=359.0, 1.0, |v| format!("{v:.0}°")) {
                    a.fade.angle = deg;
                    changed = true;
                }
                ui.add_space(8.0);
                let mut preset = a.fade.angle.round() as u32;
                if segmented(
                    ui,
                    &mut preset,
                    &[(0, "Across"), (90, "Down"), (45, "Diagonal")],
                ) {
                    a.fade.angle = preset as f32;
                    changed = true;
                }
            },
        );
        s.row(
            icon::CLOUD,
            "Softness",
            "Low: crisp bands of colour. High: each colour melts into the next",
            "fade gradient soft hard blend",
            |ui| {
                let mut pct = a.fade.softness * 100.0;
                if slider(ui, &mut pct, 0.0..=100.0, 1.0, |v| format!("{v:.0}%")) {
                    a.fade.softness = pct / 100.0;
                    changed = true;
                }
            },
        );
        s.row(
            icon::SUN,
            "Strength",
            "How much of the surface shows through. On glass, how strongly the fade tints it",
            "fade gradient opacity transparency strength",
            |ui| {
                let mut pct = a.fade.opacity * 100.0;
                if slider(ui, &mut pct, FADE_OPACITY_MIN * 100.0..=100.0, 1.0, |v| {
                    format!("{v:.0}%")
                }) {
                    a.fade.opacity = pct / 100.0;
                    changed = true;
                }
            },
        );
        if changed {
            a.app_theme = AppTheme::Custom;
        }
        // What the text colour came out as, and whether the colours had to give way for it.
        let custom = Theme::from_appearance(&Appearance {
            app_theme: AppTheme::Custom,
            animate: false,
            ..a.clone()
        });
        let wanted: Vec<theme::Rgba> = a
            .fade
            .stops
            .iter()
            .filter_map(|s| theme::parse_hex(&s.color))
            .collect();
        let shown: Vec<theme::Rgba> = custom
            .fade
            .map(|f| f.stops().iter().map(|s| s.color).collect())
            .unwrap_or_default();
        let light = custom.text.rel_luminance() > 0.5;
        let mut words = format!(
            "Text on this fade is {}, chosen so it can always be read.",
            if light { "light" } else { "dark" }
        );
        if wanted != shown {
            words.push_str(if light {
                " Some colours are shown a little deeper than picked, to keep it readable."
            } else {
                " Some colours are shown a little lighter than picked, to keep it readable."
            });
        }
        s.block(
            icon::INFO,
            "Readable text",
            &words,
            "contrast text colour readable",
            |_| {},
        );
        s.end();
    }
}

/// The pill and a card in this theme, over a pretend desktop: the body is drawn from the
/// same layers Pillbox paints, with the clock, a line of dim text, the tab row and an accent
/// bar in the theme's colours.
fn paint_theme_preview(ui: &mut Ui, t: &Theme, clock: f32) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(width, 230.0), egui::Sense::hover());
    let p = ui.painter_at(rect);
    // The pretend desktop.
    let desk = Theme {
        fill_top: super::preview::wallpaper(0.0, 0.0),
        fill_bottom: super::preview::wallpaper(1.0, 1.0),
        rim: None,
        glass: None,
        fade: None,
        motion: None,
        ..*t
    };
    super::preview::body(&p, "preview-desk", rect, 12.0, &desk, 0.0, rect);
    // The closed pill.
    let pill = Rect::from_center_size(
        Pos2::new(rect.center().x, rect.top() + 32.0),
        vec2(150.0, 36.0),
    );
    super::preview::body(&p, "preview-pill", pill, 18.0, t, clock, rect);
    p.text(
        Pos2::new(pill.left() + 20.0, pill.center().y),
        Align2::LEFT_CENTER,
        "19:18",
        semibold(14.0),
        to_color32(t.text),
    );
    p.circle_filled(
        Pos2::new(pill.right() - 20.0, pill.center().y),
        4.0,
        to_color32(t.accent),
    );
    // The open card.
    let card = Rect::from_center_size(
        Pos2::new(rect.center().x, rect.top() + 140.0),
        vec2(width.min(360.0) - 24.0, 140.0),
    );
    super::preview::body(&p, "preview-card", card, 20.0, t, clock, rect);
    // Tab row: the lit tab, then dim icons.
    let row_y = card.top() + 22.0;
    let chip = Rect::from_center_size(Pos2::new(card.left() + 44.0, row_y), vec2(60.0, 22.0));
    let strength = if t.chip == t.text { 0.15 } else { 0.28 };
    p.rect_filled(chip, 11.0, to_color32(t.chip.with_alpha(strength)));
    p.text(
        chip.center(),
        Align2::CENTER_CENTER,
        "Home",
        semibold(12.0),
        to_color32(t.text),
    );
    for i in 0..5 {
        p.circle_filled(
            Pos2::new(chip.right() + 22.0 + i as f32 * 24.0, row_y),
            3.0,
            to_color32(t.text_dim),
        );
    }
    // A tile with the clock and the date.
    let tile = Rect::from_min_size(
        Pos2::new(card.left() + 14.0, card.top() + 44.0),
        vec2((card.width() - 40.0) / 2.0, card.height() - 58.0),
    );
    p.rect_filled(tile, 10.0, to_color32(t.wash.with_alpha(0.07)));
    p.text(
        Pos2::new(tile.left() + 12.0, tile.top() + 12.0),
        Align2::LEFT_TOP,
        "19:18",
        semibold(24.0),
        to_color32(t.text),
    );
    p.text(
        Pos2::new(tile.left() + 12.0, tile.bottom() - 12.0),
        Align2::LEFT_BOTTOM,
        "Fri 20 Nov",
        regular(12.5),
        to_color32(t.text_dim),
    );
    // A second tile with a bar in the accent.
    let tile2 = Rect::from_min_size(Pos2::new(tile.right() + 12.0, tile.top()), tile.size());
    p.rect_filled(tile2, 10.0, to_color32(t.wash.with_alpha(0.07)));
    p.text(
        Pos2::new(tile2.left() + 12.0, tile2.top() + 12.0),
        Align2::LEFT_TOP,
        "79%",
        semibold(24.0),
        to_color32(t.text),
    );
    let bar = Rect::from_min_max(
        Pos2::new(tile2.left() + 12.0, tile2.bottom() - 18.0),
        Pos2::new(tile2.right() - 12.0, tile2.bottom() - 14.0),
    );
    p.rect_filled(bar, 2.0, to_color32(t.wash.with_alpha(0.22)));
    p.rect_filled(
        Rect::from_min_max(
            bar.min,
            Pos2::new(bar.left() + bar.width() * 0.79, bar.bottom()),
        ),
        2.0,
        to_color32(t.accent),
    );
}

/// A small card in an app theme's colours: its body (fade and all), the tab row with the lit
/// tab, two tiles and the accent bar.
fn paint_app_theme(p: &egui::Painter, r: Rect, t: &Theme, slot: &str) {
    super::preview::body(p, slot, r, 9.0, t, 0.0, r);
    let y = r.top() + 13.0;
    for i in 0..4 {
        let c = Pos2::new(r.left() + 20.0 + i as f32 * 20.0, y);
        if i == 0 {
            p.rect_filled(
                Rect::from_center_size(c, vec2(24.0, 14.0)),
                7.0,
                to_color32(t.chip.with_alpha(0.3)),
            );
        }
        p.circle_filled(c, 3.0, to_color32(if i == 0 { t.text } else { t.text_dim }));
    }
    let top = r.top() + 26.0;
    let bottom = r.bottom() - 17.0;
    let half = (r.width() - 24.0) / 2.0;
    for k in 0..2 {
        let x = r.left() + 8.0 + k as f32 * (half + 8.0);
        p.rect_filled(
            Rect::from_min_max(Pos2::new(x, top), Pos2::new(x + half, bottom)),
            5.0,
            to_color32(t.wash.with_alpha(0.13)),
        );
    }
    let bar_y = r.bottom() - 9.0;
    let (l, rt) = (r.left() + 10.0, r.right() - 10.0);
    p.rect_filled(
        Rect::from_min_max(Pos2::new(l, bar_y - 1.5), Pos2::new(rt, bar_y + 1.5)),
        1.5,
        to_color32(t.wash.with_alpha(0.22)),
    );
    p.rect_filled(
        Rect::from_min_max(
            Pos2::new(l, bar_y - 1.5),
            Pos2::new(l + (rt - l) * 0.45, bar_y + 1.5),
        ),
        1.5,
        to_color32(t.accent),
    );
}
