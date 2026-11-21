//! The Notifications tab (M8): the toast notifications Windows holds, newest first. A child
//! module of `tabs`, so it shares its helpers. A click on a notification opens the app that
//! posted it; notifications cannot be dismissed from here.

use pillbox_core::{
    cliphistory,
    notes::{self, Note},
    theme::Rgba,
};

use super::{Ctx, Scroller, note, push_clipped, rounded};
use crate::{
    platform::gfx::{Align, Frame, ImageSlot},
    scene::{Action, Hit, Rect, icon, label},
};

const ROW_H: f32 = 54.0;

/// An app's badge centred at `(x, y)`, `rad` pixels in radius: its icon on a light plate when
/// the icon has been loaded, else a coloured disc with the app's first letter.
pub fn badge(f: &mut Frame<'_>, (x, y): (f32, f32), rad: f32, app_id: &str, app: &str, alpha: f32) {
    let slot = ImageSlot::Thumb(notes::icon_key(app_id));
    if f.has_image(slot) {
        // The plate keeps dark or transparent logos readable on the dark Pillbox.
        f.fill_ellipse(x, y, rad, rad, Rgba::rgb(0xF2F2F2).fade(alpha));
        let r = rad * 0.78;
        f.fill_pill_image(slot, &rounded((x - r, y - r, x + r, y + r), r * 0.3), alpha);
        return;
    }
    f.fill_ellipse(
        x,
        y,
        rad,
        rad,
        Rgba::rgb(notes::badge_color(app_id)).fade(alpha),
    );
    f.text(
        &notes::badge_letter(app).to_string(),
        x - rad,
        y - rad,
        x + rad,
        y + rad,
        label(
            rad * 1.05,
            true,
            Rgba::rgb(0xFFFFFF).fade(alpha),
            Align::Center,
        ),
    );
}

pub fn notifications(
    f: &mut Frame<'_>,
    cx: &Ctx<'_>,
    (cl, ct, cr, cb): Rect,
    hits: &mut Vec<Hit>,
) -> f32 {
    let k = cx.k;
    let n = &cx.ui.notes;
    let area = (cl - 6.0 * k, ct, cr + 6.0 * k, cb);
    let list: Vec<&Note> = n.store.notes().collect();
    if list.is_empty() {
        note(f, cx, area, n.message.unwrap_or("No notifications."));
        return 0.0;
    }
    let (row_h, gap) = (ROW_H * k, 5.0 * k);
    let total = list.len() as f32 * (row_h + gap);
    let (sc, max) = Scroller::new(area.1, area.3, total, cx.ui.scroll);
    sc.clip(f, area.0, area.2, max, cx.k);
    for (i, item) in list.iter().enumerate() {
        let t = sc.y(i as f32 * (row_h + gap));
        if !sc.visible(t, t + row_h) {
            continue;
        }
        let rect = (area.0, t, area.2, t + row_h);
        row(f, cx, rect, item, n.now_ms);
        push_clipped(&sc, hits, rect, Action::NoteOpen(item.id));
    }
    f.pop_fade_clip();
    max
}

fn row(f: &mut Frame<'_>, cx: &Ctx<'_>, rect: Rect, item: &Note, now_ms: i64) {
    let k = cx.k;
    let (l, t, r, b) = rect;
    let hovered = cx.hovered(rect);
    f.fill_pill(
        &rounded(rect, 10.0 * k),
        cx.wash(if hovered { 0.12 } else { 0.06 }),
    );
    let rad = 14.0 * k;
    let (bx, by) = (l + 10.0 * k + rad, (t + b) / 2.0);
    badge(f, (bx, by), rad, &item.app_id, &item.app, cx.alpha);
    let text_l = bx + rad + 10.0 * k;
    let when = cliphistory::ago((now_ms - item.created_ms) / 1000);
    let when = if when == "now" {
        when
    } else {
        format!("{when} ago")
    };
    f.text(
        &when,
        r - 90.0 * k,
        t + 5.0 * k,
        r - 10.0 * k,
        t + 21.0 * k,
        label(10.5 * k, false, cx.dim(), Align::Right),
    );
    f.text(
        &item.app,
        text_l,
        t + 5.0 * k,
        r - 96.0 * k,
        t + 21.0 * k,
        label(10.5 * k, false, cx.dim(), Align::Left),
    );
    let title = if item.title.is_empty() {
        &item.body
    } else {
        &item.title
    };
    f.text(
        title,
        text_l,
        t + 19.0 * k,
        r - 10.0 * k,
        t + 37.0 * k,
        label(12.5 * k, true, cx.text(), Align::Left),
    );
    if !item.title.is_empty() && !item.body.is_empty() {
        f.text(
            &item.body,
            text_l,
            t + 35.0 * k,
            r - 10.0 * k,
            b - 3.0 * k,
            label(11.0 * k, false, cx.dim(), Align::Left),
        );
    }
    if hovered {
        // "Open the app" hint at the right end of the row.
        f.text(
            "\u{E8A7}",
            r - 26.0 * k,
            b - 20.0 * k,
            r - 8.0 * k,
            b - 4.0 * k,
            icon(10.5 * k, cx.dim()),
        );
    }
}
