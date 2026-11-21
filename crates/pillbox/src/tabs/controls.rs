//! The Controls tab: the volume and brightness sliders, the Wi-Fi and Bluetooth cards (each
//! opens a list of networks or paired devices with Connect and Disconnect buttons right on it),
//! the microphone, sound and mirror tiles, the call line and the launcher. A child module of
//! `tabs`, so it shares its helpers.

use std::path::Path;

use pillbox_core::{
    links::{self, BtKind},
    theme::Rgba,
};

use super::{
    AMBER, Ctx, G_ADD, G_APP, G_BLUETOOTH, G_BRIGHTNESS, G_CAMERA, G_CHAT, G_MIC, G_MIC_OFF,
    G_MUTED, G_PHONE, G_SOUND, G_VOLUME, G_WIFI, RED, Scroller, button, file_thumb_key,
    icon_button, mirror, note, push_clipped, rounded, slider,
};
use crate::{
    platform::gfx::{Align, Frame, ImageSlot},
    scene::{Action, Hit, LinkPanel, Rect, icon, label},
};

// More Segoe Fluent Icons glyphs.
const G_CHEVRON_DOWN: &str = "\u{E70D}";
const G_CHEVRON_UP: &str = "\u{E70E}";
const G_HEADPHONES: &str = "\u{E7F6}";
const G_MOUSE: &str = "\u{E962}";
const G_KEYBOARD: &str = "\u{E765}";
const G_GAMEPAD: &str = "\u{E7FC}";
const G_PC: &str = "\u{E7F8}";

// Layout, in logical px from the top of the content area.
/// A slider row.
const SLIDER_ROW: f32 = 34.0;
/// Top and height of the Wi-Fi and Bluetooth cards.
const CARD_TOP: f32 = 74.0;
const CARD_H: f32 = 58.0;
/// The line above a list, and one list row with the gap under it.
const LIST_HEAD: f32 = 24.0;
const LIST_ROW: f32 = 38.0;

pub fn controls(
    f: &mut Frame<'_>,
    cx: &Ctx<'_>,
    (cl, ct, cr, cb): Rect,
    hits: &mut Vec<Hit>,
) -> f32 {
    let k = cx.k;
    let c = &cx.ui.controls;
    if let Some(m) = c.mirror {
        mirror(f, cx, (cl, ct, cr, cb), m, hits);
        return 0.0;
    }
    sliders(f, cx, (cl, ct, cr), hits);
    let cards_top = ct + CARD_TOP * k;
    link_cards(f, cx, (cl, cards_top, cr, cards_top + CARD_H * k), hits);
    let below = cards_top + (CARD_H + 8.0) * k;
    if let Some(panel) = c.links.panel {
        return link_list(f, cx, panel, (cl, below, cr, cb), hits);
    }
    let mut top = quick_tiles(f, cx, (cl, below, cr), hits) + 8.0 * k;
    top = call_line(f, cx, (cl, top, cr), hits);
    launcher(f, cx, (cl, top, cr, cb), hits);
    0.0
}

// ---- sliders ---------------------------------------------------------------------------------

fn sliders(f: &mut Frame<'_>, cx: &Ctx<'_>, (cl, ct, cr): (f32, f32, f32), hits: &mut Vec<Hit>) {
    let k = cx.k;
    let c = &cx.ui.controls;
    let row = SLIDER_ROW * k;
    let pct_w = 40.0 * k;
    let track = (cl + 40.0 * k, cr - pct_w - 6.0 * k);
    // Volume.
    let y = ct + row / 2.0;
    match c.volume {
        Some((level, muted)) => {
            let (glyph, color) = if muted {
                (G_MUTED, RED.fade(cx.alpha))
            } else {
                (G_VOLUME, cx.text())
            };
            icon_button(
                f,
                cx,
                (cl + 14.0 * k, y),
                28.0 * k,
                glyph,
                color,
                Action::SpeakerMute,
                hits,
            );
            slider(
                f,
                cx,
                track,
                y,
                level,
                Action::Volume {
                    left: track.0,
                    right: track.1,
                },
                hits,
            );
            percent(f, cx, (cr - pct_w, y, cr), level);
        }
        None => f.text(
            "No speakers found",
            cl,
            ct,
            cr,
            ct + row,
            label(12.5 * k, false, cx.dim(), Align::Left),
        ),
    }
    // Brightness.
    let y = ct + row * 1.5;
    f.text(
        G_BRIGHTNESS,
        cl,
        y - 14.0 * k,
        cl + 28.0 * k,
        y + 14.0 * k,
        icon(13.0 * k, cx.text()),
    );
    match c.brightness {
        Some(level) => {
            slider(
                f,
                cx,
                track,
                y,
                level,
                Action::Brightness {
                    left: track.0,
                    right: track.1,
                },
                hits,
            );
            percent(f, cx, (cr - pct_w, y, cr), level);
        }
        None => f.text(
            "No display accepts brightness changes",
            cl + 40.0 * k,
            y - 12.0 * k,
            cr,
            y + 12.0 * k,
            label(12.0 * k, false, cx.dim(), Align::Left),
        ),
    }
}

fn percent(f: &mut Frame<'_>, cx: &Ctx<'_>, (l, y, r): (f32, f32, f32), level: f32) {
    let k = cx.k;
    f.text(
        &format!("{:.0}%", level * 100.0),
        l,
        y - 10.0 * k,
        r,
        y + 10.0 * k,
        label(12.0 * k, false, cx.dim(), Align::Right),
    );
}

// ---- the Wi-Fi and Bluetooth cards -----------------------------------------------------------

/// Two cards side by side. The round switch on a card turns the radio on or off; the rest of the
/// card opens or folds its list.
fn link_cards(f: &mut Frame<'_>, cx: &Ctx<'_>, (cl, t, cr, b): Rect, hits: &mut Vec<Hit>) {
    let k = cx.k;
    let c = &cx.ui.controls;
    let l = &c.links;
    let gap = 8.0 * k;
    let w = (cr - cl - gap) / 2.0;
    for (i, panel) in [LinkPanel::Wifi, LinkPanel::Bluetooth]
        .into_iter()
        .enumerate()
    {
        let (glyph, name, radio, toggle, summary) = match panel {
            LinkPanel::Wifi => (
                G_WIFI,
                "Wi-Fi",
                c.wifi,
                Action::Wifi,
                links::wifi_summary(c.wifi, l.wifi_adapter, l.wifi),
            ),
            LinkPanel::Bluetooth => (
                G_BLUETOOTH,
                "Bluetooth",
                c.bluetooth,
                Action::Bluetooth,
                links::bt_summary(c.bluetooth, l.bluetooth),
            ),
        };
        let left = cl + i as f32 * (w + gap);
        let rect = (left, t, left + w, b);
        let open = l.panel == Some(panel);
        let on = radio == Some(true);
        let wash = if open {
            0.16
        } else if cx.hovered(rect) {
            0.13
        } else {
            0.09
        };
        let body = rounded(rect, 14.0 * k);
        f.fill_pill(&body, cx.wash(wash));
        if open {
            f.stroke_pill(&body, cx.accent().with_alpha(0.8 * cx.alpha), 1.5 * k);
        }
        hits.push(Hit {
            rect,
            action: Action::LinkPanel(panel),
        });
        // The round switch.
        let d = 36.0 * k;
        let (dx, dy) = (left + 12.0 * k + d / 2.0, (t + b) / 2.0);
        let (disc, fg) = if on {
            (cx.accent().with_alpha(0.95 * cx.alpha), cx.on_accent())
        } else {
            (cx.wash(0.15), cx.text())
        };
        f.fill_ellipse(dx, dy, d / 2.0, d / 2.0, disc);
        f.text(
            glyph,
            dx - d / 2.0,
            dy - d / 2.0,
            dx + d / 2.0,
            dy + d / 2.0,
            icon(16.0 * k, fg),
        );
        if radio.is_some() {
            hits.push(Hit {
                rect: (dx - d / 2.0 - 3.0 * k, t, dx + d / 2.0 + 5.0 * k, b),
                action: toggle,
            });
        }
        // Name and state.
        let text_l = left + 12.0 * k + d + 10.0 * k;
        let text_r = left + w - 26.0 * k;
        f.text(
            name,
            text_l,
            t + 9.0 * k,
            text_r,
            t + 29.0 * k,
            label(12.5 * k, true, cx.text(), Align::Left),
        );
        f.text(
            &summary,
            text_l,
            t + 29.0 * k,
            text_r,
            b - 8.0 * k,
            label(10.5 * k, false, cx.dim(), Align::Left),
        );
        f.text(
            if open { G_CHEVRON_UP } else { G_CHEVRON_DOWN },
            left + w - 26.0 * k,
            t,
            left + w - 6.0 * k,
            b,
            icon(10.0 * k, cx.dim()),
        );
    }
}

// ---- the lists -------------------------------------------------------------------------------

/// What a list row shows at its left end.
enum RowIcon {
    /// Wi-Fi signal bars.
    Signal(u8),
    Glyph(&'static str),
}

struct RowView<'a> {
    icon: RowIcon,
    title: &'a str,
    sub: String,
    /// Drawn bold: the connected one.
    strong: bool,
    /// The button at the right end: label, action, filled.
    button: Option<(&'static str, Action, bool)>,
    /// A switch is in progress: show that instead of the button.
    busy: bool,
}

/// The open list under the cards. Returns how far it can scroll.
fn link_list(
    f: &mut Frame<'_>,
    cx: &Ctx<'_>,
    panel: LinkPanel,
    (cl, ct, cr, cb): Rect,
    hits: &mut Vec<Hit>,
) -> f32 {
    let k = cx.k;
    let c = &cx.ui.controls;
    let l = &c.links;
    let (title, settings) = match panel {
        LinkPanel::Wifi => ("Wi-Fi networks", "More in Windows"),
        LinkPanel::Bluetooth => ("Paired devices", "Add in Windows"),
    };
    f.text(
        title,
        cl + 4.0 * k,
        ct,
        cr - 120.0 * k,
        ct + LIST_HEAD * k,
        label(11.0 * k, true, cx.dim(), Align::Left),
    );
    let more = (cr - 124.0 * k, ct, cr, ct + LIST_HEAD * k);
    let hover = cx.hovered(more);
    f.text(
        settings,
        more.0,
        more.1,
        more.2 - 4.0 * k,
        more.3,
        label(
            11.0 * k,
            false,
            if hover { cx.text() } else { cx.dim() },
            Align::Right,
        ),
    );
    hits.push(Hit {
        rect: more,
        action: Action::LinkSettings(panel),
    });
    // The last switch that went wrong, under the list.
    let mut bottom = cb;
    if let Some(text) = l.note {
        bottom -= 20.0 * k;
        f.text(
            text,
            cl + 4.0 * k,
            bottom,
            cr - 4.0 * k,
            cb,
            label(10.5 * k, false, AMBER.fade(cx.alpha), Align::Left),
        );
    }
    let area = (cl, ct + LIST_HEAD * k, cr, bottom);
    let (rows, empty) = match panel {
        LinkPanel::Wifi => wifi_rows(cx),
        LinkPanel::Bluetooth => bluetooth_rows(cx),
    };
    if rows.is_empty() {
        note(f, cx, area, empty);
        return 0.0;
    }
    list_rows(f, cx, area, &rows, hits)
}

fn wifi_rows<'a>(cx: &Ctx<'a>) -> (Vec<RowView<'a>>, &'static str) {
    let c = &cx.ui.controls;
    let l = &c.links;
    let empty = if c.wifi == Some(false) {
        "Wi-Fi is off. Use the round switch on the card to turn it on."
    } else if !l.wifi_loaded {
        "Looking for networks…"
    } else if !l.wifi_adapter {
        "No Wi-Fi adapter was found on this PC."
    } else {
        "No networks found yet. If it stays empty, Windows may need Location turned on to list Wi-Fi."
    };
    if c.wifi == Some(false) {
        return (Vec::new(), empty);
    }
    let rows = l
        .wifi
        .iter()
        .enumerate()
        .map(|(i, n)| {
            let sub = if n.connected {
                "Connected".to_string()
            } else if n.saved && n.secure {
                "Saved · secured".to_string()
            } else if n.saved {
                "Saved · open".to_string()
            } else if n.secure {
                "Secured · join it in Windows first".to_string()
            } else {
                "Open network · join it in Windows".to_string()
            };
            let button = if n.connected {
                Some(("Disconnect", Action::WifiLeave, false))
            } else if n.saved {
                Some(("Connect", Action::WifiJoin(i), true))
            } else {
                None
            };
            RowView {
                icon: RowIcon::Signal(links::signal_bars(n.signal)),
                title: &n.ssid,
                sub,
                strong: n.connected,
                button,
                busy: l.busy == Some(n.ssid.as_str()),
            }
        })
        .collect();
    (rows, empty)
}

fn bluetooth_rows<'a>(cx: &Ctx<'a>) -> (Vec<RowView<'a>>, &'static str) {
    let c = &cx.ui.controls;
    let l = &c.links;
    let empty = match c.bluetooth {
        None => "No Bluetooth adapter was found on this PC.",
        Some(false) => "Bluetooth is off. Use the round switch on the card to turn it on.",
        Some(true) if !l.bluetooth_loaded => "Looking for devices…",
        Some(true) => "No paired devices. Pair one in Windows (Add in Windows).",
    };
    if c.bluetooth != Some(true) {
        return (Vec::new(), empty);
    }
    let rows = l
        .bluetooth
        .iter()
        .map(|d| {
            let glyph = match d.kind {
                BtKind::Audio => G_HEADPHONES,
                BtKind::Phone => G_PHONE,
                BtKind::Computer => G_PC,
                BtKind::Input => {
                    let n = d.name.to_lowercase();
                    if n.contains("mouse") {
                        G_MOUSE
                    } else if n.contains("keyboard") {
                        G_KEYBOARD
                    } else {
                        G_GAMEPAD
                    }
                }
                BtKind::Other => G_BLUETOOTH,
            };
            let button = if !d.can_toggle() {
                None
            } else if d.connected {
                Some(("Disconnect", Action::BtDisconnect(d.address), false))
            } else {
                Some(("Connect", Action::BtConnect(d.address), true))
            };
            RowView {
                icon: RowIcon::Glyph(glyph),
                title: &d.name,
                sub: if d.connected {
                    "Connected"
                } else {
                    "Not connected"
                }
                .to_string(),
                strong: d.connected,
                button,
                busy: l.busy == Some(d.name.as_str()),
            }
        })
        .collect();
    (rows, empty)
}

/// Draw the rows in a scrolling area. Returns how far it can scroll.
fn list_rows(
    f: &mut Frame<'_>,
    cx: &Ctx<'_>,
    area: Rect,
    rows: &[RowView<'_>],
    hits: &mut Vec<Hit>,
) -> f32 {
    let k = cx.k;
    let (cl, top, cr, bottom) = area;
    let pitch = LIST_ROW * k;
    let row_h = pitch - 4.0 * k;
    let (sc, max) = Scroller::new(top, bottom, rows.len() as f32 * pitch, cx.ui.scroll);
    sc.clip(f, cl - 4.0 * k, cr + 4.0 * k, max, k);
    for (i, row) in rows.iter().enumerate() {
        let y = sc.y(i as f32 * pitch);
        if !sc.visible(y, y + row_h) {
            continue;
        }
        let rect = (cl, y, cr, y + row_h);
        let wash = if row.strong { 0.12 } else { 0.07 };
        f.fill_pill(&rounded(rect, 12.0 * k), cx.wash(wash));
        // Icon.
        let (ix, iy) = (cl + 10.0 * k, y + row_h / 2.0);
        match row.icon {
            RowIcon::Signal(bars) => signal(f, cx, (ix, iy), bars, row.strong),
            RowIcon::Glyph(glyph) => f.text(
                glyph,
                ix - 2.0 * k,
                iy - 12.0 * k,
                ix + 18.0 * k,
                iy + 12.0 * k,
                icon(14.0 * k, if row.strong { cx.accent() } else { cx.dim() }),
            ),
        }
        // The button, or what is going on.
        let text_r = if row.button.is_some() || row.busy {
            cr - 96.0 * k
        } else {
            cr - 10.0 * k
        };
        if row.busy {
            f.text(
                "Working…",
                cr - 94.0 * k,
                y,
                cr - 10.0 * k,
                y + row_h,
                label(11.0 * k, false, cx.dim(), Align::Right),
            );
        } else if let Some((text, action, filled)) = row.button {
            let mut tmp = Vec::new();
            button(
                f,
                cx,
                (
                    cr - 88.0 * k,
                    y + 5.0 * k,
                    cr - 8.0 * k,
                    y + row_h - 5.0 * k,
                ),
                "",
                text,
                filled,
                action,
                &mut tmp,
            );
            for h in tmp {
                push_clipped(&sc, hits, h.rect, h.action);
            }
        }
        let text_l = cl + 38.0 * k;
        f.text(
            row.title,
            text_l,
            y + 4.0 * k,
            text_r,
            y + 20.0 * k,
            label(12.5 * k, row.strong, cx.text(), Align::Left),
        );
        f.text(
            &row.sub,
            text_l,
            y + 19.0 * k,
            text_r,
            y + row_h - 2.0 * k,
            label(10.5 * k, false, cx.dim(), Align::Left),
        );
    }
    f.pop_fade_clip();
    max
}

/// Four signal bars, `lit` of them bright.
fn signal(f: &mut Frame<'_>, cx: &Ctx<'_>, (x, y): (f32, f32), lit: u8, connected: bool) {
    let k = cx.k;
    for i in 0..4u8 {
        let h = (4.0 + 3.0 * f32::from(i)) * k;
        let l = x + f32::from(i) * 5.0 * k;
        let color = if i < lit {
            if connected { cx.accent() } else { cx.text() }
        } else {
            cx.wash(0.25)
        };
        f.fill_pill(
            &rounded((l, y + 8.0 * k - h, l + 3.0 * k, y + 8.0 * k), 1.0 * k),
            color,
        );
    }
}

// ---- tiles, call line, launcher --------------------------------------------------------------

/// The microphone, sound settings, mirror and chat drawer tiles in one row. Returns its bottom.
fn quick_tiles(
    f: &mut Frame<'_>,
    cx: &Ctx<'_>,
    (cl, top, cr): (f32, f32, f32),
    hits: &mut Vec<Hit>,
) -> f32 {
    let k = cx.k;
    let c = &cx.ui.controls;
    let muted = c.mic_muted == Some(true);
    let (chat_on, chat_open) = c.chat_drawer;
    let mut tiles: Vec<(&str, &str, bool, Rgba, Action)> = vec![
        (
            if muted { G_MIC_OFF } else { G_MIC },
            if muted { "Mic muted" } else { "Mic on" },
            c.mic_muted == Some(false),
            if muted {
                RED.fade(cx.alpha)
            } else {
                cx.on_accent()
            },
            Action::MicMute,
        ),
        (G_SOUND, "Sound", false, cx.text(), Action::SoundSettings),
        (G_CAMERA, "Mirror", false, cx.text(), Action::Mirror),
    ];
    if chat_on {
        tiles.push((
            G_CHAT,
            "Chat",
            chat_open,
            cx.on_accent(),
            Action::ChatDrawer,
        ));
    }
    let gap = 6.0 * k;
    let h = 38.0 * k;
    let w = (cr - cl - gap * (tiles.len() as f32 - 1.0)) / tiles.len() as f32;
    for (i, (glyph, name, on, glyph_color, action)) in tiles.into_iter().enumerate() {
        let l = cl + i as f32 * (w + gap);
        let rect = (l, top, l + w, top + h);
        let bg = if on {
            cx.accent().with_alpha(0.85 * cx.alpha)
        } else {
            cx.wash(if cx.hovered(rect) { 0.16 } else { 0.09 })
        };
        f.fill_pill(&rounded(rect, 12.0 * k), bg);
        let fg = if on { cx.on_accent() } else { cx.text() };
        // A tile that is not "on" keeps the glyph's own colour only for the red muted mic.
        let glyph_fg = if on || glyph_color == cx.text() {
            fg
        } else {
            glyph_color
        };
        f.text(
            glyph,
            l + 8.0 * k,
            top,
            l + 28.0 * k,
            top + h,
            icon(14.0 * k, glyph_fg),
        );
        f.text(
            name,
            l + 28.0 * k,
            top,
            l + w - 4.0 * k,
            top + h,
            label(10.5 * k, false, fg, Align::Left),
        );
        hits.push(Hit { rect, action });
    }
    top + h
}

/// "On a call" with a Mute button, when there is a call. Returns where the next thing starts.
fn call_line(
    f: &mut Frame<'_>,
    cx: &Ctx<'_>,
    (cl, top, cr): (f32, f32, f32),
    hits: &mut Vec<Hit>,
) -> f32 {
    let k = cx.k;
    let Some(call) = cx.ui.controls.call else {
        return top;
    };
    let rect = (cl, top, cr, top + 30.0 * k);
    f.fill_pill(&rounded(rect, 15.0 * k), cx.wash(0.08));
    let (glyph, color) = if call.muted {
        (G_MIC_OFF, RED)
    } else {
        (G_MIC, Rgba::rgb(0x34C759))
    };
    f.text(
        glyph,
        cl + 6.0 * k,
        top,
        cl + 30.0 * k,
        top + 30.0 * k,
        icon(12.5 * k, color.fade(cx.alpha)),
    );
    let text = format!(
        "On a call · {} · {}",
        call.app,
        pillbox_core::music::format_clock(call.seconds as f32)
    );
    f.text(
        &text,
        cl + 34.0 * k,
        top,
        cr - 80.0 * k,
        top + 30.0 * k,
        label(12.0 * k, false, cx.text(), Align::Left),
    );
    let label_text = if call.muted { "Unmute" } else { "Mute" };
    button(
        f,
        cx,
        (cr - 76.0 * k, top + 3.0 * k, cr - 4.0 * k, top + 27.0 * k),
        "",
        label_text,
        call.muted,
        Action::MicMute,
        hits,
    );
    top + 38.0 * k
}

/// Six launcher slots, as tall as the room allows (nothing when there is too little).
fn launcher(f: &mut Frame<'_>, cx: &Ctx<'_>, (cl, top, cr, cb): Rect, hits: &mut Vec<Hit>) {
    let k = cx.k;
    let c = &cx.ui.controls;
    let slots = 6usize;
    let gap = 6.0 * k;
    let w = (cr - cl - gap * (slots as f32 - 1.0)) / slots as f32;
    let h = (cb - top).clamp(0.0, 64.0 * k);
    if h < 40.0 * k {
        return;
    }
    for i in 0..slots {
        let l = cl + i as f32 * (w + gap);
        let rect = (l, top, l + w, top + h);
        let hovered = cx.hovered(rect);
        f.fill_pill(
            &rounded(rect, 12.0 * k),
            cx.wash(if hovered { 0.14 } else { 0.06 }),
        );
        match c.launcher.get(i) {
            Some(slot) => {
                let side = 28.0 * k;
                let mid = (rect.0 + rect.2) / 2.0;
                let img = rounded(
                    (
                        mid - side / 2.0,
                        top + 7.0 * k,
                        mid + side / 2.0,
                        top + 7.0 * k + side,
                    ),
                    5.0 * k,
                );
                if !f.fill_pill_image(
                    ImageSlot::Thumb(file_thumb_key(Path::new(&slot.path))),
                    &img,
                    cx.alpha,
                ) {
                    f.text(
                        G_APP,
                        img.left,
                        img.top,
                        img.right,
                        img.bottom,
                        icon(side * 0.6, cx.dim()),
                    );
                }
                let name = if slot.name.is_empty() {
                    Path::new(&slot.path)
                        .file_stem()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_default()
                } else {
                    slot.name.clone()
                };
                f.text(
                    &name,
                    rect.0 + 2.0 * k,
                    top + h - 20.0 * k,
                    rect.2 - 2.0 * k,
                    top + h - 4.0 * k,
                    label(10.0 * k, false, cx.text(), Align::Center),
                );
                hits.push(Hit {
                    rect,
                    action: Action::Launch(i),
                });
            }
            None => {
                f.text(
                    G_ADD,
                    rect.0,
                    rect.1,
                    rect.2,
                    rect.3,
                    icon(13.0 * k, cx.dim()),
                );
                hits.push(Hit {
                    rect,
                    action: Action::OpenSettings,
                });
            }
        }
    }
}
