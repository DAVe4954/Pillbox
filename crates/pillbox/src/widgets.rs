//! Widget tiles for the Home tab (and for widgets popped out onto the desktop). Drawing only;
//! the app gathers the numbers into `WidgetData`.

use pillbox_core::{settings::WidgetKind, theme::Rgba};

use crate::{
    platform::gfx::{Align, Frame, ImageSlot},
    scene::{Action, Field, Hit, Rect, WidgetAction, icon, label},
    tabs::{self, Ctx, WidgetsUi, rounded},
};

/// Thumbnail key of the current photo for the photo widget.
pub const PHOTO_KEY: u64 = (1 << 61) | 7;

#[derive(Debug, Clone, Default)]
pub struct WorldTime {
    pub name: String,
    pub time: String,
    /// "+8 h", "−5 h 30", "same time".
    pub offset: String,
}

#[derive(Debug, Clone, Default)]
pub struct Todo {
    pub id: u64,
    pub text: String,
    pub done: bool,
}

/// What the System card shows. Texts are ready to draw; `None` = no reading (yet).
#[derive(Debug, Clone, Default)]
pub struct SysView {
    /// The shown graphics card's 3D load, 0..100.
    pub gpu: Option<f32>,
    /// The PC has a graphics card to show (dedicated, or else the integrated one).
    pub has_gpu: bool,
    /// The card shown is integrated graphics: the PC has no dedicated one.
    pub gpu_integrated: bool,
    /// The busiest disk: ("C:", active time in percent, "3.2 MB/s" read, "1.1 MB/s" written).
    /// `None` until two readings.
    pub disk: Option<(String, f32, String, String)>,
    /// The shown graphics card's name ("GeForce RTX 5060 Laptop GPU").
    pub gpu_name: Option<String>,
    /// The connection in use: (name, "↓ 12.3  ↑ 1.2", unit). The rates are `None` for the first
    /// second, before a second reading exists.
    pub net: Option<(String, Option<String>, &'static str)>,
}

/// One day of the expanded Play time card, texts ready to draw.
#[derive(Debug, Clone, Default)]
pub struct PlayDayView {
    /// The day's chip: "Today", "Thu 19".
    pub chip: String,
    /// "Today", "Yesterday", "Wednesday 18 Nov".
    pub title: String,
    /// "2h 5m", or empty for a day without play.
    pub total: String,
    /// Game, time, and the share of the day's longest game (0..1), longest first.
    pub games: Vec<(String, String, f32)>,
}

#[derive(Debug, Clone, Default)]
pub struct WidgetData {
    pub sys: SysView,
    pub time: String,
    pub date: String,
    /// Hours since midnight, fractional, for the 24-hour dial.
    pub day_hours: f32,
    pub clock_color: Option<Rgba>,
    pub world: Vec<WorldTime>,
    pub weather: Option<String>,
    pub weather_detail: Option<String>,
    /// The detailed weather look: condition, range, wind (empty for the minimal look).
    pub weather_lines: Vec<String>,
    /// Play time: today, the last 7 days, the most played game this week, the game now.
    pub play_today: String,
    pub play_week: String,
    pub play_top: Option<String>,
    pub play_now: Option<String>,
    /// The last days of play for the expanded card, today first.
    pub play_days: Vec<PlayDayView>,
    pub weather_note: Option<&'static str>,
    pub air: Option<String>,
    pub sun: Option<(String, String)>,
    pub moon: (f32, &'static str),
    /// Upcoming reminders: text and when.
    pub next: Vec<(String, String)>,
    pub todos: Vec<Todo>,
    pub calc_result: Option<String>,
    /// Remaining and total seconds, and whether it is running.
    pub focus: (u32, u32, bool),
    pub countdown: Option<(String, i64)>,
    pub day_progress: f32,
    pub year_progress: f32,
    pub cpu: Option<f32>,
    /// Used and total memory, GB.
    pub ram: Option<(f32, f32)>,
    /// Download and upload, already formatted.
    pub net: Option<(String, String)>,
    pub battery: Option<(u8, bool)>,
    /// Active time today, seconds.
    pub screen_time: u64,
    pub ping: Option<Result<u32, String>>,
    pub websites: Vec<(String, Option<bool>)>,
    pub currency: Vec<(String, f64)>,
    pub currency_base: String,
    pub photo: bool,
    /// Network widgets are switched off.
    pub offline: bool,
}

/// Widgets that use a whole row.
pub fn is_wide(kind: WidgetKind, d: &WidgetData) -> bool {
    if kind == WidgetKind::Weather {
        return !d.weather_lines.is_empty();
    }
    matches!(
        kind,
        WidgetKind::Todo
            | WidgetKind::QuickNote
            | WidgetKind::NextEvents
            | WidgetKind::WorldClock
            | WidgetKind::WebsiteCheck
            | WidgetKind::Photos
            | WidgetKind::DayClock
    )
}

/// Whether a tile takes the whole row on the Home tab: the wide widgets, and Play time while
/// it is expanded.
pub fn is_wide_on_home(kind: WidgetKind, w: &WidgetsUi<'_>) -> bool {
    is_wide(kind, w.data) || (kind == WidgetKind::PlayTime && w.play_open)
}

/// Height of a tile on the Home tab in logical px (multiply by the scale).
pub fn tile_height(kind: WidgetKind, w: &WidgetsUi<'_>) -> f32 {
    if kind == WidgetKind::PlayTime && w.play_open && !w.data.play_days.is_empty() {
        PLAY_LIST_TOP + play_rows(w.data, w.play_day) as f32 * PLAY_ROW + 8.0
    } else {
        TILE_H
    }
}

/// A normal tile's height, logical px.
pub const TILE_H: f32 = 88.0;
/// Where the game list starts in the expanded Play time card, and the height of one game.
const PLAY_LIST_TOP: f32 = 58.0;
const PLAY_ROW: f32 = 26.0;
/// Most lines the expanded card lists (the last one may be "+N more").
const PLAY_LINES: usize = 6;

/// Games shown for a day, and how many more there are.
fn play_shown(day: &PlayDayView) -> (usize, usize) {
    if day.games.len() > PLAY_LINES {
        (PLAY_LINES - 1, day.games.len() - (PLAY_LINES - 1))
    } else {
        (day.games.len(), 0)
    }
}

/// Lines the expanded card has room for: the chosen day's, at least two so the card never
/// shrinks to nothing (an empty day shows one message line).
fn play_rows(d: &WidgetData, day: usize) -> usize {
    d.play_days
        .get(day)
        .map_or(1, |day| {
            let (shown, more) = play_shown(day);
            shown + usize::from(more > 0)
        })
        .max(2)
}

fn title(f: &mut Frame<'_>, cx: &Ctx<'_>, rect: Rect, glyph: &str, text: &str) {
    let k = cx.k;
    let (l, t, r, _) = rect;
    f.text(
        glyph,
        l + 8.0 * k,
        t + 6.0 * k,
        l + 24.0 * k,
        t + 22.0 * k,
        icon(10.5 * k, cx.dim()),
    );
    f.text(
        text,
        l + 26.0 * k,
        t + 6.0 * k,
        r - 8.0 * k,
        t + 22.0 * k,
        label(11.0 * k, true, cx.dim(), Align::Left),
    );
}

fn big(f: &mut Frame<'_>, cx: &Ctx<'_>, rect: Rect, text: &str, color: Rgba) {
    let k = cx.k;
    let (l, t, r, b) = rect;
    f.text(
        text,
        l + 10.0 * k,
        t + 22.0 * k,
        r - 8.0 * k,
        b - 20.0 * k,
        label(24.0 * k, true, color, Align::Left),
    );
}

fn small(f: &mut Frame<'_>, cx: &Ctx<'_>, rect: Rect, text: &str) {
    let k = cx.k;
    let (l, _, r, b) = rect;
    f.text(
        text,
        l + 10.0 * k,
        b - 24.0 * k,
        r - 8.0 * k,
        b - 6.0 * k,
        label(11.0 * k, false, cx.dim(), Align::Left),
    );
}

fn bar(f: &mut Frame<'_>, cx: &Ctx<'_>, rect: Rect, fraction: f32) {
    let k = cx.k;
    let (l, _, r, b) = rect;
    let (bl, br, y) = (l + 10.0 * k, r - 10.0 * k, b - 14.0 * k);
    let half = 3.0 * k;
    f.fill_pill(&rounded((bl, y - half, br, y + half), half), cx.wash(0.16));
    let head = bl + (br - bl) * fraction.clamp(0.0, 1.0);
    f.fill_pill(
        &rounded((bl, y - half, head.max(bl + 2.0 * half), y + half), half),
        cx.accent(),
    );
}

fn offline(f: &mut Frame<'_>, cx: &Ctx<'_>, rect: Rect) {
    small(f, cx, rect, "Off: turn on online widgets in Settings");
}

/// Draw one widget into `rect`. `index` is its place in the shown list (for pop-out).
pub fn paint_tile(
    f: &mut Frame<'_>,
    cx: &Ctx<'_>,
    kind: WidgetKind,
    index: usize,
    rect: Rect,
    hits: &mut Vec<Hit>,
) {
    let k = cx.k;
    let d = &cx.ui.widgets.data;
    let (l, t, r, b) = rect;
    match kind {
        WidgetKind::Clock => {
            title(f, cx, rect, "\u{E823}", "Clock");
            big(
                f,
                cx,
                rect,
                &d.time,
                d.clock_color.map_or(cx.text(), |c| c.fade(cx.alpha)),
            );
            small(f, cx, rect, &d.date);
        }
        WidgetKind::DayClock => {
            // A 24-hour dial: midnight at the bottom, noon at the top, the sun's arc shaded.
            let cy = (t + b) / 2.0;
            let rad = (b - t) / 2.0 - 8.0 * k;
            let cxp = l + 12.0 * k + rad;
            f.fill_ellipse(cxp, cy, rad, rad, cx.wash(0.1));
            for h in 0..24 {
                let a = (h as f32 / 24.0) * std::f32::consts::TAU + std::f32::consts::PI / 2.0;
                let (ca, sa) = (a.cos(), a.sin());
                let inner = if h % 6 == 0 { rad * 0.78 } else { rad * 0.88 };
                f.line(
                    cxp + ca * inner,
                    cy + sa * inner,
                    cxp + ca * rad * 0.96,
                    cy + sa * rad * 0.96,
                    cx.dim(),
                    k,
                );
            }
            let a = (d.day_hours / 24.0) * std::f32::consts::TAU + std::f32::consts::PI / 2.0;
            let color = d.clock_color.map_or(cx.accent(), |c| c.fade(cx.alpha));
            f.line(
                cxp,
                cy,
                cxp + a.cos() * rad * 0.8,
                cy + a.sin() * rad * 0.8,
                color,
                2.5 * k,
            );
            f.fill_ellipse(cxp, cy, 3.0 * k, 3.0 * k, color);
            let tl = cxp + rad + 16.0 * k;
            f.text(
                &d.time,
                tl,
                t + 10.0 * k,
                r - 8.0 * k,
                cy,
                label(26.0 * k, true, color, Align::Left),
            );
            let sub = match &d.sun {
                Some((rise, set)) => format!("{}  ·  ☀ {rise} – {set}", d.date),
                None => d.date.clone(),
            };
            f.text(
                &sub,
                tl,
                cy,
                r - 8.0 * k,
                b - 10.0 * k,
                label(12.0 * k, false, cx.dim(), Align::Left),
            );
        }
        WidgetKind::WorldClock => {
            title(f, cx, rect, "\u{E909}", "World clock");
            if d.world.is_empty() {
                small(f, cx, rect, "Add time zones in Settings");
            }
            let n = d.world.len().clamp(1, 3);
            let w = (r - l - 16.0 * k) / n as f32;
            for (i, z) in d.world.iter().take(3).enumerate() {
                let x = l + 8.0 * k + i as f32 * w;
                f.text(
                    &z.time,
                    x,
                    t + 24.0 * k,
                    x + w,
                    t + 52.0 * k,
                    label(20.0 * k, true, cx.text(), Align::Left),
                );
                f.text(
                    &z.name,
                    x,
                    t + 52.0 * k,
                    x + w - 4.0 * k,
                    t + 68.0 * k,
                    label(11.0 * k, true, cx.dim(), Align::Left),
                );
                f.text(
                    &z.offset,
                    x,
                    t + 67.0 * k,
                    x + w - 4.0 * k,
                    b - 4.0 * k,
                    label(10.5 * k, false, cx.dim(), Align::Left),
                );
            }
        }
        WidgetKind::Weather => {
            title(f, cx, rect, "\u{E753}", "Weather");
            match (&d.weather, d.weather_note) {
                (Some(w), _) if !d.weather_lines.is_empty() => {
                    // Detailed look: temperature on the left, a few lines on the right.
                    let mid = l + (r - l) * 0.42;
                    f.text(
                        w,
                        l + 10.0 * k,
                        t + 22.0 * k,
                        mid,
                        b - 8.0 * k,
                        label(30.0 * k, true, cx.text(), Align::Left),
                    );
                    let line_h = 19.0 * k;
                    let top = t + 24.0 * k;
                    for (i, line) in d.weather_lines.iter().take(3).enumerate() {
                        let y = top + line_h * i as f32;
                        let (size, bold, color) = if i == 0 {
                            (13.0 * k, true, cx.text())
                        } else {
                            (11.0 * k, false, cx.dim())
                        };
                        f.text(
                            line,
                            mid,
                            y,
                            r - 8.0 * k,
                            y + line_h,
                            label(size, bold, color, Align::Left),
                        );
                    }
                }
                (Some(w), _) => {
                    big(f, cx, rect, w, cx.text());
                    if let Some(detail) = &d.weather_detail {
                        small(f, cx, rect, detail);
                    }
                }
                (None, Some(note)) => small(f, cx, rect, note),
                (None, None) => small(f, cx, rect, "…"),
            }
        }
        WidgetKind::AirQuality => {
            title(f, cx, rect, "\u{E9CA}", "Air quality");
            if d.offline {
                offline(f, cx, rect);
            } else {
                match &d.air {
                    Some(a) => big(f, cx, rect, a, cx.text()),
                    None => small(f, cx, rect, "Set a location in Settings → Weather"),
                }
            }
        }
        WidgetKind::SunMoon => {
            title(f, cx, rect, "\u{E706}", "Sun and moon");
            let (phase, name) = d.moon;
            // The moon: a lit disc with a shadow disc slid across it.
            let rad = 14.0 * k;
            let (mx, my) = (r - 12.0 * k - rad, t + 26.0 * k + rad);
            f.fill_ellipse(mx, my, rad, rad, Rgba::rgb(0xF2EBD3).fade(cx.alpha));
            let shift = (phase * 2.0 - 1.0) * 2.0 * rad;
            if (phase - 0.5).abs() > 0.03 {
                f.push_clip(mx - rad, my - rad, mx + rad, my + rad);
                f.fill_ellipse(
                    mx + shift,
                    my,
                    rad * 1.02,
                    rad * 1.02,
                    Rgba::new(0.08, 0.08, 0.1, 0.92 * cx.alpha),
                );
                f.pop_clip();
            }
            match &d.sun {
                Some((rise, set)) => {
                    f.text(
                        &format!("↑ {rise}"),
                        l + 10.0 * k,
                        t + 24.0 * k,
                        r - 50.0 * k,
                        t + 46.0 * k,
                        label(15.0 * k, true, cx.text(), Align::Left),
                    );
                    f.text(
                        &format!("↓ {set}"),
                        l + 10.0 * k,
                        t + 46.0 * k,
                        r - 50.0 * k,
                        t + 66.0 * k,
                        label(15.0 * k, true, cx.text(), Align::Left),
                    );
                }
                None => {
                    f.text(
                        "Set a location",
                        l + 10.0 * k,
                        t + 26.0 * k,
                        r - 50.0 * k,
                        t + 60.0 * k,
                        label(12.0 * k, false, cx.dim(), Align::Left),
                    );
                }
            }
            small(f, cx, rect, name);
        }
        WidgetKind::NextEvents => {
            title(f, cx, rect, "\u{EA8F}", "Next reminders");
            if d.next.is_empty() {
                small(f, cx, rect, "Nothing coming up");
            }
            for (i, (text, when)) in d.next.iter().take(3).enumerate() {
                let y = t + 24.0 * k + i as f32 * 20.0 * k;
                f.text(
                    when,
                    l + 10.0 * k,
                    y,
                    l + 110.0 * k,
                    y + 20.0 * k,
                    label(11.5 * k, true, cx.accent(), Align::Left),
                );
                f.text(
                    text,
                    l + 112.0 * k,
                    y,
                    r - 8.0 * k,
                    y + 20.0 * k,
                    label(12.0 * k, false, cx.text(), Align::Left),
                );
            }
            hits.push(Hit {
                rect,
                action: Action::Tab(crate::scene::Tab::Reminders),
            });
        }
        WidgetKind::Todo => {
            title(f, cx, rect, "\u{E9D5}", "To-do");
            let field = (l + 8.0 * k, t + 24.0 * k, r - 8.0 * k, t + 46.0 * k);
            tabs::text_field(
                f,
                cx,
                field,
                Field::Todo,
                "Add a to-do and press Enter",
                None,
                hits,
            );
            let open: Vec<&Todo> = d.todos.iter().take(2).collect();
            for (i, item) in open.iter().enumerate() {
                let y = t + 50.0 * k + i as f32 * 18.0 * k;
                let glyph = if item.done { "\u{E73A}" } else { "\u{E739}" };
                let row = (l + 8.0 * k, y, r - 30.0 * k, y + 18.0 * k);
                f.text(
                    glyph,
                    row.0,
                    y,
                    row.0 + 18.0 * k,
                    y + 18.0 * k,
                    icon(11.0 * k, cx.dim()),
                );
                let color = if item.done { cx.dim() } else { cx.text() };
                f.text(
                    &item.text,
                    row.0 + 22.0 * k,
                    y,
                    row.2,
                    y + 18.0 * k,
                    label(12.0 * k, false, color, Align::Left),
                );
                hits.push(Hit {
                    rect: row,
                    action: Action::Widget(WidgetAction::TodoToggle(item.id)),
                });
                let del = (r - 28.0 * k, y, r - 8.0 * k, y + 18.0 * k);
                f.text(
                    "\u{E711}",
                    del.0,
                    del.1,
                    del.2,
                    del.3,
                    icon(9.0 * k, cx.dim()),
                );
                hits.push(Hit {
                    rect: del,
                    action: Action::Widget(WidgetAction::TodoDelete(item.id)),
                });
            }
            if d.todos.len() > 2 {
                f.text(
                    &format!("+{}", d.todos.len() - 2),
                    r - 60.0 * k,
                    t + 6.0 * k,
                    r - 10.0 * k,
                    t + 22.0 * k,
                    label(11.0 * k, false, cx.dim(), Align::Right),
                );
            }
        }
        WidgetKind::QuickNote => {
            title(f, cx, rect, "\u{E70B}", "Quick note");
            let area = (l + 6.0 * k, t + 24.0 * k, r - 6.0 * k, b - 6.0 * k);
            let focused = cx.ui.focus == Some(Field::Note);
            if focused {
                tabs::text_field(
                    f,
                    cx,
                    (area.0, area.1, area.2, area.1 + 26.0 * k),
                    Field::Note,
                    "",
                    None,
                    hits,
                );
            } else {
                let text = &cx.ui.edits.note.text;
                if text.is_empty() {
                    f.text(
                        "Click to write a note",
                        area.0 + 4.0 * k,
                        area.1,
                        area.2,
                        area.1 + 24.0 * k,
                        label(12.5 * k, false, cx.dim(), Align::Left),
                    );
                } else {
                    f.text_wrapped(
                        text,
                        area.0 + 4.0 * k,
                        area.1 + 2.0 * k,
                        area.2,
                        area.3,
                        label(12.5 * k, false, cx.text(), Align::Left),
                    );
                }
                hits.push(Hit {
                    rect: area,
                    action: Action::Focus(Field::Note),
                });
            }
        }
        WidgetKind::Calculator => {
            title(f, cx, rect, "\u{E8EF}", "Calculator");
            let field = (l + 8.0 * k, t + 24.0 * k, r - 8.0 * k, t + 48.0 * k);
            tabs::text_field(f, cx, field, Field::Calc, "2 + 2", None, hits);
            if let Some(res) = &d.calc_result {
                f.text(
                    &format!("= {res}"),
                    l + 10.0 * k,
                    t + 50.0 * k,
                    r - 8.0 * k,
                    b - 6.0 * k,
                    label(17.0 * k, true, cx.accent(), Align::Left),
                );
            }
        }
        WidgetKind::FocusTimer => {
            title(f, cx, rect, "\u{E916}", "Focus timer");
            let (left, total, running) = d.focus;
            let text = format!("{}:{:02}", left / 60, left % 60);
            big(
                f,
                cx,
                (l, t, r - 60.0 * k, b),
                &text,
                if running { cx.accent() } else { cx.text() },
            );
            let glyph = if running { "\u{E769}" } else { "\u{E768}" };
            tabs::icon_button(
                f,
                cx,
                (r - 22.0 * k, t + 40.0 * k),
                30.0 * k,
                glyph,
                cx.text(),
                Action::Widget(WidgetAction::FocusTimer),
                hits,
            );
            if left != total {
                tabs::icon_button(
                    f,
                    cx,
                    (r - 50.0 * k, t + 40.0 * k),
                    26.0 * k,
                    "\u{E72C}",
                    cx.dim(),
                    Action::Widget(WidgetAction::FocusReset),
                    hits,
                );
            }
            bar(f, cx, rect, 1.0 - left as f32 / total.max(1) as f32);
        }
        WidgetKind::Countdown => {
            title(f, cx, rect, "\u{E787}", "Countdown");
            match &d.countdown {
                Some((name, days)) => {
                    let text = match days {
                        0 => "Today".to_string(),
                        1 => "1 day".to_string(),
                        n if *n < 0 => format!("{} days ago", -n),
                        n => format!("{n} days"),
                    };
                    big(f, cx, rect, &text, cx.text());
                    small(f, cx, rect, name);
                }
                None => small(f, cx, rect, "Set a date in Settings"),
            }
        }
        WidgetKind::DayProgress => {
            title(f, cx, rect, "\u{E916}", "Day");
            big(
                f,
                cx,
                rect,
                &format!("{:.0}%", d.day_progress * 100.0),
                cx.text(),
            );
            bar(f, cx, rect, d.day_progress);
        }
        WidgetKind::YearProgress => {
            title(f, cx, rect, "\u{E787}", "Year");
            big(
                f,
                cx,
                rect,
                &format!("{:.1}%", d.year_progress * 100.0),
                cx.text(),
            );
            bar(f, cx, rect, d.year_progress);
        }
        WidgetKind::PcStats => {
            title(f, cx, rect, "\u{E9D9}", "PC");
            let cpu = d
                .cpu
                .map_or("CPU …".to_string(), |c| format!("CPU {c:.0}%"));
            let ram = d.ram.map_or("RAM …".to_string(), |(u, tot)| {
                format!("RAM {u:.1}/{tot:.0} GB")
            });
            f.text(
                &cpu,
                l + 10.0 * k,
                t + 24.0 * k,
                r - 8.0 * k,
                t + 42.0 * k,
                label(13.0 * k, true, cx.text(), Align::Left),
            );
            f.text(
                &ram,
                l + 10.0 * k,
                t + 42.0 * k,
                r - 8.0 * k,
                t + 60.0 * k,
                label(12.0 * k, false, cx.text(), Align::Left),
            );
            if let Some((down, up)) = &d.net {
                small(f, cx, rect, &format!("↓ {down}  ↑ {up}"));
            }
        }
        // Not a Home tile any more: the System window draws it.
        WidgetKind::System => {}
        WidgetKind::Battery => {
            title(f, cx, rect, "\u{E83F}", "Battery");
            match d.battery {
                Some((p, plugged)) => {
                    big(f, cx, rect, &format!("{p}%"), cx.text());
                    // The status sits above the bar, not on it.
                    f.text(
                        if plugged { "Plugged in" } else { "On battery" },
                        l + 10.0 * k,
                        b - 32.0 * k,
                        r - 8.0 * k,
                        b - 18.0 * k,
                        label(11.0 * k, false, cx.dim(), Align::Left),
                    );
                    bar(f, cx, rect, f32::from(p) / 100.0);
                }
                None => small(f, cx, rect, "No battery"),
            }
        }
        WidgetKind::PlayTime if cx.ui.widgets.play_open && !d.play_days.is_empty() => {
            paint_play_open(f, cx, rect, hits);
            // The collapse button is the handle here; no pop-out.
            return;
        }
        WidgetKind::PlayTime => {
            // The whole tile opens the day list; the pop-out handle (drawn last) wins its corner.
            hits.push(Hit {
                rect,
                action: Action::Widget(WidgetAction::PlayToggle),
            });
            f.text(
                "\u{E70D}",
                r - 48.0 * k,
                t + 4.0 * k,
                r - 26.0 * k,
                t + 24.0 * k,
                icon(10.0 * k, cx.dim()),
            );
            match &d.play_now {
                Some(game) => title(f, cx, rect, "\u{E7FC}", &format!("Playing {game}")),
                None => title(f, cx, rect, "\u{E7FC}", "Play time"),
            }
            big(
                f,
                cx,
                rect,
                &format!("{} today", d.play_today),
                if d.play_now.is_some() {
                    cx.accent()
                } else {
                    cx.text()
                },
            );
            let week = match &d.play_top {
                Some(top) => format!("Week {} · most {top}", d.play_week),
                None => format!("Week {}", d.play_week),
            };
            small(f, cx, rect, &week);
        }
        WidgetKind::ScreenTime => {
            title(f, cx, rect, "\u{E7F4}", "Screen time today");
            let (h, m) = (d.screen_time / 3600, (d.screen_time / 60) % 60);
            big(f, cx, rect, &format!("{h} h {m:02} min"), cx.text());
        }
        WidgetKind::Ping => {
            title(f, cx, rect, "\u{E968}", "Ping");
            if d.offline {
                offline(f, cx, rect);
            } else {
                match &d.ping {
                    Some(Ok(ms)) => big(f, cx, rect, &format!("{ms} ms"), cx.text()),
                    Some(Err(e)) => small(f, cx, rect, e),
                    None => small(f, cx, rect, "…"),
                }
            }
        }
        WidgetKind::WebsiteCheck => {
            title(f, cx, rect, "\u{E774}", "Websites");
            if d.offline {
                offline(f, cx, rect);
            } else if d.websites.is_empty() {
                small(f, cx, rect, "Add websites in Settings");
            }
            for (i, (url, up)) in d.websites.iter().take(3).enumerate() {
                let y = t + 24.0 * k + i as f32 * 20.0 * k;
                let color = match up {
                    Some(true) => Rgba::rgb(0x34C759),
                    Some(false) => Rgba::rgb(0xFF5C5C),
                    None => Rgba::rgb(0x888888),
                };
                f.fill_ellipse(
                    l + 15.0 * k,
                    y + 10.0 * k,
                    4.0 * k,
                    4.0 * k,
                    color.fade(cx.alpha),
                );
                f.text(
                    url,
                    l + 26.0 * k,
                    y,
                    r - 8.0 * k,
                    y + 20.0 * k,
                    label(12.0 * k, false, cx.text(), Align::Left),
                );
            }
        }
        WidgetKind::Currency => {
            title(f, cx, rect, "\u{E8C7}", &format!("1 {}", d.currency_base));
            if d.offline {
                offline(f, cx, rect);
            } else if d.currency.is_empty() {
                small(f, cx, rect, "…");
            }
            for (i, (code, rate)) in d.currency.iter().take(3).enumerate() {
                let y = t + 24.0 * k + i as f32 * 19.0 * k;
                f.text(
                    &format!("{rate:.4} {code}"),
                    l + 10.0 * k,
                    y,
                    r - 8.0 * k,
                    y + 19.0 * k,
                    label(13.0 * k, i == 0, cx.text(), Align::Left),
                );
            }
        }
        WidgetKind::Photos => {
            let img = rounded(
                (l + 4.0 * k, t + 4.0 * k, r - 4.0 * k, b - 4.0 * k),
                11.0 * k,
            );
            if d.photo && f.fill_pill_image(ImageSlot::Thumb(PHOTO_KEY), &img, cx.alpha) {
                hits.push(Hit {
                    rect,
                    action: Action::Widget(WidgetAction::PhotoNext),
                });
            } else {
                title(f, cx, rect, "\u{EB9F}", "Photos");
                small(f, cx, rect, "No pictures found in the folder");
            }
        }
    }
    // Pop-out handle in the top-right corner, shown on hover.
    if cx.hovered(rect) && !matches!(kind, WidgetKind::Photos) {
        tabs::icon_button(
            f,
            cx,
            (r - 14.0 * k, t + 14.0 * k),
            22.0 * k,
            "\u{E8A7}",
            cx.dim(),
            Action::Widget(WidgetAction::PopOut(index)),
            hits,
        );
    }
}

/// The expanded Play time card: a chip per day, and what was played on the chosen day.
fn paint_play_open(f: &mut Frame<'_>, cx: &Ctx<'_>, rect: Rect, hits: &mut Vec<Hit>) {
    let k = cx.k;
    let w = &cx.ui.widgets;
    let d = w.data;
    let (l, t, r, _) = rect;
    let sel = w.play_day.min(d.play_days.len().saturating_sub(1));
    let day = &d.play_days[sel];
    // Title row: what is being played, the chosen day's total, and the collapse button.
    let heading = match &d.play_now {
        Some(game) => format!("Playing {game}"),
        None => "Play time".to_string(),
    };
    f.text(
        "\u{E7FC}",
        l + 8.0 * k,
        t + 6.0 * k,
        l + 24.0 * k,
        t + 22.0 * k,
        icon(10.5 * k, cx.dim()),
    );
    let split = l + (r - l) * 0.4;
    f.text(
        &heading,
        l + 26.0 * k,
        t + 6.0 * k,
        split,
        t + 22.0 * k,
        label(11.0 * k, true, cx.dim(), Align::Left),
    );
    let summary = if day.total.is_empty() {
        day.title.clone()
    } else {
        format!("{} · {}", day.title, day.total)
    };
    f.text(
        &summary,
        split,
        t + 5.0 * k,
        r - 34.0 * k,
        t + 23.0 * k,
        label(12.0 * k, true, cx.text(), Align::Right),
    );
    tabs::icon_button(
        f,
        cx,
        (r - 16.0 * k, t + 14.0 * k),
        22.0 * k,
        "\u{E70E}",
        cx.dim(),
        Action::Widget(WidgetAction::PlayToggle),
        hits,
    );
    // One chip per day.
    let n = d.play_days.len();
    let gap = 3.0 * k;
    let chip_w = (r - l - 16.0 * k - gap * (n as f32 - 1.0)) / n as f32;
    let (chip_t, chip_b) = (t + 28.0 * k, t + 50.0 * k);
    for (i, day_i) in d.play_days.iter().enumerate() {
        let cl = l + 8.0 * k + i as f32 * (chip_w + gap);
        let chip = (cl, chip_t, cl + chip_w, chip_b);
        let selected = i == sel;
        let bg = if selected {
            cx.accent().with_alpha(0.85 * cx.alpha)
        } else {
            cx.wash(if cx.hovered(chip) { 0.16 } else { 0.08 })
        };
        f.fill_pill(&rounded(chip, 11.0 * k), bg);
        let color = if selected {
            cx.on_accent()
        } else if day_i.games.is_empty() {
            cx.dim()
        } else {
            cx.text()
        };
        f.text(
            &day_i.chip,
            chip.0,
            chip.1,
            chip.2,
            chip.3,
            label(10.5 * k, selected, color, Align::Center),
        );
        hits.push(Hit {
            rect: chip,
            action: Action::Widget(WidgetAction::PlayDay(i)),
        });
    }
    // The games of that day, longest first, each with a bar against the longest.
    let list_l = l + 10.0 * k;
    let list_r = r - 10.0 * k;
    if day.games.is_empty() {
        f.text(
            "No games played",
            list_l,
            t + PLAY_LIST_TOP * k,
            list_r,
            t + (PLAY_LIST_TOP + 20.0) * k,
            label(12.0 * k, false, cx.dim(), Align::Left),
        );
        return;
    }
    let (shown, more) = play_shown(day);
    let live_today = sel == 0;
    for (i, (name, time, share)) in day.games.iter().take(shown).enumerate() {
        let y = t + (PLAY_LIST_TOP + PLAY_ROW * i as f32) * k;
        let live = live_today && d.play_now.as_deref() == Some(name.as_str());
        let name_color = if live { cx.accent() } else { cx.text() };
        f.text(
            name,
            list_l,
            y,
            list_r - 64.0 * k,
            y + 17.0 * k,
            label(12.0 * k, live, name_color, Align::Left),
        );
        f.text(
            time,
            list_r - 64.0 * k,
            y,
            list_r,
            y + 17.0 * k,
            label(12.0 * k, false, cx.dim(), Align::Right),
        );
        let half = 1.5 * k;
        let bar_y = y + 21.0 * k;
        f.fill_pill(
            &rounded((list_l, bar_y - half, list_r, bar_y + half), half),
            cx.wash(0.1),
        );
        let head = (list_l + (list_r - list_l) * share.clamp(0.0, 1.0)).max(list_l + 2.0 * half);
        let strength = if live { 1.0 } else { 0.7 };
        f.fill_pill(
            &rounded((list_l, bar_y - half, head, bar_y + half), half),
            cx.accent().with_alpha(strength * cx.alpha),
        );
    }
    if more > 0 {
        let y = t + (PLAY_LIST_TOP + PLAY_ROW * shown as f32) * k;
        f.text(
            &format!("+{more} more"),
            list_l,
            y,
            list_r,
            y + 17.0 * k,
            label(11.0 * k, false, cx.dim(), Align::Left),
        );
    }
}

// ---- the System window ---------------------------------------------------------------------------

/// Height of the header strip of the System window ("System" and the close button), logical px.
const SYS_HEADER: f32 = 20.0;
/// Padding around and gap between the cards, logical px.
const SYS_PAD: f32 = 8.0;
/// Height of a card, logical px (the same as a Home tile).
pub const SYS_CARD_H: f32 = 88.0;
/// The narrowest a card may get, and the width a card starts with, logical px.
pub const SYS_CARD_MIN_W: f32 = 100.0;
pub const SYS_CARD_W: f32 = 128.0;

/// Window height in logical px (multiply by the scale).
pub const SYS_WINDOW_H: f32 = SYS_HEADER + SYS_CARD_H + SYS_PAD;

/// Window width for `cards` cards that are each `card_w` logical px wide.
pub fn system_window_width(cards: usize, card_w: f32) -> f32 {
    let n = cards.max(1) as f32;
    n * card_w + (n - 1.0) * SYS_PAD + 2.0 * SYS_PAD
}

/// The close button of the System window (client pixels).
pub fn system_close_rect(width: f32, k: f32) -> Rect {
    let (r, s) = (width - 8.0 * k, 16.0 * k);
    (r - s, 3.0 * k, r, 3.0 * k + s)
}

/// One component's card: its icon, name, the main figure, a second line, and a bar.
struct SysCard {
    glyph: &'static str,
    title: String,
    value: String,
    caption: String,
    /// A second line under the first (the disk's write speed under its read speed).
    caption2: Option<String>,
    fraction: Option<f32>,
}

fn percent_text(v: Option<f32>) -> String {
    v.map_or("…".to_string(), |v| format!("{v:.0}%"))
}

/// The cards the window shows, in order: CPU, GPU, RAM, disk, network, and the battery only on
/// a PC that has one.
fn system_cards(d: &WidgetData) -> Vec<SysCard> {
    let mut cards = vec![SysCard {
        glyph: "\u{E9D9}",
        title: "CPU".into(),
        value: percent_text(d.cpu),
        caption: "Processor load".into(),
        fraction: d.cpu.map(|c| c / 100.0),
        caption2: None,
    }];
    if d.sys.has_gpu {
        cards.push(SysCard {
            glyph: "\u{E7F4}",
            title: "GPU".into(),
            value: percent_text(d.sys.gpu),
            // The card's own name; "Graphics" if it could not be asked for.
            caption: d.sys.gpu_name.clone().unwrap_or_else(|| "Graphics".into()),
            fraction: d.sys.gpu.map(|g| g / 100.0),
            caption2: None,
        });
    }
    cards.push(match d.ram {
        Some((used, total)) => SysCard {
            glyph: "\u{E964}",
            title: "RAM".into(),
            value: format!("{:.0}%", used / total.max(0.001) * 100.0),
            caption: format!("{used:.1} of {total:.0} GB"),
            fraction: Some(used / total.max(0.001)),
            caption2: None,
        },
        None => SysCard {
            glyph: "\u{E964}",
            title: "RAM".into(),
            value: "…".into(),
            caption: "Memory".into(),
            fraction: None,
            caption2: None,
        },
    });
    cards.push(match &d.sys.disk {
        Some((name, active, read, write)) => SysCard {
            glyph: "\u{EDA2}",
            title: format!("Disk {name}"),
            value: format!("{active:.0}%"),
            caption: format!("Read {read}"),
            caption2: Some(format!("Write {write}")),
            fraction: Some(active / 100.0),
        },
        None => SysCard {
            glyph: "\u{EDA2}",
            title: "Disk".into(),
            value: "…".into(),
            caption: "Activity".into(),
            caption2: None,
            fraction: None,
        },
    });
    cards.push(match &d.sys.net {
        Some((name, rates, unit)) => {
            // "↓ 12.3  ↑ 1.2": the download is the figure, the upload the second line.
            let (down, up) = rates
                .as_deref()
                .and_then(|r| r.split_once("  "))
                .unwrap_or(("…", ""));
            SysCard {
                glyph: "\u{E839}",
                title: name.clone(),
                value: down.to_string(),
                caption: format!("{up} {unit}").trim().to_string(),
                caption2: None,
                fraction: None,
            }
        }
        None => SysCard {
            glyph: "\u{EB5E}",
            title: "Network".into(),
            value: "Offline".into(),
            caption: "No connection".into(),
            caption2: None,
            fraction: None,
        },
    });
    if let Some((p, plugged)) = d.battery {
        cards.push(SysCard {
            glyph: "\u{E83F}",
            title: "Battery".into(),
            value: format!("{p}%"),
            caption: if plugged { "Plugged in" } else { "On battery" }.into(),
            caption2: None,
            fraction: Some(f32::from(p) / 100.0),
        });
    }
    cards
}

/// Draw the System window's content into `rect` (the whole window): a header with the close
/// button, then one card per component sharing the width equally.
pub fn paint_system_window(f: &mut Frame<'_>, cx: &Ctx<'_>, rect: Rect) {
    let k = cx.k;
    let (l, t, r, b) = rect;
    f.text(
        "System",
        l + SYS_PAD * k + 2.0 * k,
        t + 2.0 * k,
        l + 200.0 * k,
        t + SYS_HEADER * k,
        label(11.0 * k, true, cx.dim(), Align::Left),
    );
    let close = system_close_rect(r - l, k);
    f.text(
        "\u{E711}",
        close.0,
        close.1,
        close.2,
        close.3,
        icon(10.0 * k, cx.dim()),
    );
    let gap = SYS_PAD * k;
    paint_system_cards(
        f,
        cx,
        (l + gap, t + SYS_HEADER * k, r - gap, b - gap * 0.5),
        1,
    );
}

/// The PC component cards across `rect`, in `rows` rows (1 for the desktop window, 2 for the
/// System tab). The first row gets the larger share of the cards (3 + 2 for five); each row
/// shares its width equally and the rows share the height (a card is at most `SYS_CARD_H`).
pub fn paint_system_cards(f: &mut Frame<'_>, cx: &Ctx<'_>, rect: Rect, rows: usize) {
    let k = cx.k;
    let (l, top, r, b) = rect;
    let cards = system_cards(cx.ui.widgets.data);
    let rows = rows.clamp(1, 2);
    let gap = SYS_PAD * k;
    let per_row = cards.len().div_ceil(rows).max(1);
    let card_h = (((b - top) - (rows as f32 - 1.0) * gap) / rows as f32).min(SYS_CARD_H * k);
    for (row, chunk) in cards.chunks(per_row).enumerate() {
        let n = chunk.len() as f32;
        let card_w = ((r - l) - (n - 1.0) * gap) / n;
        let y0 = top + row as f32 * (card_h + gap);
        for (i, c) in chunk.iter().enumerate() {
            let x = l + i as f32 * (card_w + gap);
            paint_sys_card(f, cx, (x, y0, x + card_w, y0 + card_h), c);
        }
    }
}

fn paint_sys_card(f: &mut Frame<'_>, cx: &Ctx<'_>, card: Rect, c: &SysCard) {
    let k = cx.k;
    let (x, top, r, bottom) = card;
    let card_w = r - x;
    f.fill_pill(&rounded(card, 12.0 * k), cx.wash(0.08));
    title(f, cx, card, c.glyph, &c.title);
    let size = if c.value.chars().count() > 6 {
        19.0
    } else {
        26.0
    };
    f.text(
        &c.value,
        x + 10.0 * k,
        top + 20.0 * k,
        r - 6.0 * k,
        top + 48.0 * k,
        label(size * k, true, cx.text(), Align::Left),
    );
    // Long names (a graphics card) get a smaller size so they fit the card.
    let long = c.caption.chars().count() as f32 * 6.0 > card_w / k - 14.0;
    let size = if long { 9.5 } else { 11.0 };
    f.text(
        &c.caption,
        x + 10.0 * k,
        top + 48.0 * k,
        r - 6.0 * k,
        top + 62.0 * k,
        label(size * k, false, cx.dim(), Align::Left),
    );
    if let Some(second) = &c.caption2 {
        f.text(
            second,
            x + 10.0 * k,
            top + 60.0 * k,
            r - 6.0 * k,
            top + 74.0 * k,
            label(11.0 * k, false, cx.dim(), Align::Left),
        );
    }
    if let Some(fraction) = c.fraction {
        // A thin bar along the bottom of the card, under the captions.
        let (bl, br, y, half) = (x + 10.0 * k, r - 10.0 * k, bottom - 8.0 * k, 2.5 * k);
        f.fill_pill(&rounded((bl, y - half, br, y + half), half), cx.wash(0.16));
        let head = bl + (br - bl) * fraction.clamp(0.0, 1.0);
        f.fill_pill(
            &rounded((bl, y - half, head.max(bl + 2.0 * half), y + half), half),
            cx.accent(),
        );
    }
}

/// How many cards the System tab or window shows for this data (CPU, GPU, RAM, disk, network,
/// and the battery on a laptop).
pub fn system_card_count(d: &WidgetData) -> usize {
    system_cards(d).len()
}
