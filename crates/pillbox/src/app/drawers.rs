//! M5b: the chat drawer. A stream's chat below Pillbox (or beside it on a side edge).
//! Opened and closed with its shortcut or the button on the Controls tab; it stays open over
//! fullscreen games unless that setting is off. (The general web drawer was removed after M5.)

use pillbox_core::settings::Side;

use super::{App, Mode, TOAST_MS, Toast};
use crate::platform::drawer::Drawer;

#[derive(Default)]
pub(super) struct Drawers {
    open: Option<Drawer>,
}

impl Drawers {
    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }
}

impl App {
    pub(super) fn toggle_chat_drawer(&mut self) {
        if self.drawers.is_open() {
            self.close_drawer();
            return;
        }
        let d = &self.settings.drawers;
        if !d.chat_enabled {
            return self.show_toast(Toast::text("Turn on the chat drawer in Settings"), 2500);
        }
        let Some(url) = d.chat_url() else {
            return self.show_toast(Toast::text("Set a channel for the chat drawer"), 2500);
        };
        let title = format!("Chat · {}", d.chat_channel);
        match Drawer::open(&title, &url, self.drawer_rect(), self.monitor.dpi) {
            Ok(drawer) => {
                tracing::info!("chat drawer opened");
                self.drawers.open = Some(drawer);
            }
            Err(e) => {
                tracing::warn!("chat drawer: {e:#}");
                self.show_toast(Toast::text("The drawer could not open"), TOAST_MS);
            }
        }
        self.dirty = true;
    }

    pub(super) fn close_drawer(&mut self) {
        if self.drawers.open.take().is_some() {
            tracing::info!("chat drawer closed");
            self.dirty = true;
        }
    }

    /// Screen rectangle for the drawer: below Pillbox's window (beside it on a side edge),
    /// kept on Pillbox's monitor.
    fn drawer_rect(&self) -> (i32, i32, i32, i32) {
        // The drawer's size is its own setting, in DIPs: Pillbox's size does not change it.
        let k = self.monitor.dpi;
        let d = &self.settings.drawers;
        let (w, h) = ((d.width * k).round() as i32, (d.height * k).round() as i32);
        let gap = (6.0 * self.monitor.scale).round() as i32;
        let (wx, wy) = self.win_pos;
        let (ww, wh) = (self.win_size.0 as i32, self.win_size.1 as i32);
        let (x, y) = match self.placement.side {
            Side::Top => (wx + ww / 2 - w / 2, wy + wh + gap),
            Side::Left => (wx + ww + gap, wy + wh / 2 - h / 2),
            Side::Right => (wx - w - gap, wy + wh / 2 - h / 2),
        };
        let m = self.monitor.rect;
        let x = x.clamp(m.left, (m.right - w).max(m.left));
        let y = y.clamp(m.top, (m.bottom - h).max(m.top));
        (x, y, w.min(m.width()), h.min(m.height()))
    }

    pub(super) fn on_drawer_close(&mut self, id: isize) {
        if self.drawers.open.as_ref().is_some_and(|d| d.is_window(id)) {
            self.close_drawer();
        }
    }

    pub(super) fn on_drawer_resized(&mut self, id: isize) {
        if let Some(d) = &self.drawers.open
            && d.is_window(id)
        {
            d.fit();
        }
    }

    /// A game came to the front (or went away): the drawer closes unless it may stay over games.
    pub(super) fn sync_drawers_for_mode(&mut self) {
        if self.mode != Mode::Normal && !self.settings.drawers.over_games {
            self.close_drawer();
        }
    }

    /// Settings changed: if the chat was switched off or its channel cleared, the drawer closes
    /// (reopen to apply a new channel); otherwise it is moved and resized to match.
    pub(super) fn sync_drawers_for_settings(&mut self) {
        if !self.drawers.is_open() {
            return;
        }
        let d = &self.settings.drawers;
        if !(d.chat_enabled && d.chat_url().is_some()) {
            self.close_drawer();
        } else if let Some(drawer) = &self.drawers.open {
            let (x, y, w, h) = self.drawer_rect();
            drawer.set_rect(x, y, w, h);
        }
    }
}
