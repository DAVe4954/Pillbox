//! The System window: a desktop window (like a popped-out widget) with one card for each of CPU,
//! GPU, RAM, disk, network and, on a laptop, the battery. Its right edge can be dragged to make it
//! wider or narrower; the cards share the width. `widgets.system_window` says whether it is open;
//! its position and width are kept in `system_window.json`.

use serde::{Deserialize, Serialize};

use super::{App, Toast};
use crate::{
    platform::{system, tray, widgetwin::WidgetWindow},
    store, widgets,
};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct SysSaved {
    x: i32,
    y: i32,
    /// Width in logical pixels (at 100% scaling), so it survives a change of screen scale.
    w: f32,
}

impl App {
    /// The System window is a desktop window of its own: it follows the screen's DPI, not the
    /// Pillbox's size setting.
    fn sys_scale(&self) -> f32 {
        self.monitor.dpi
    }

    /// Cards the window will show on this PC: CPU, RAM, disk and network always, the GPU if
    /// there is a card to show, a battery only on a laptop. Not read from the samples (there are
    /// none yet when it opens, and they arrive at different times), so the count is the same
    /// from the first frame on.
    fn sys_expected_cards(&self) -> usize {
        // Before the adapters were asked for, assume a GPU (nearly every PC has one).
        let gpu = self.t.gpu_card.as_ref().is_none_or(Option::is_some);
        4 + usize::from(gpu) + usize::from(system::battery().is_some())
    }

    /// Width limits in pixels: every card at least its minimum, at most the screen.
    fn sys_limits(&self) -> (u32, u32) {
        let k = self.sys_scale();
        let min = widgets::system_window_width(self.sys_expected_cards(), widgets::SYS_CARD_MIN_W);
        let max = (self.monitor.rect.width() as f32).max(min * k);
        ((min * k).round() as u32, max.round() as u32)
    }

    /// Open or close the window to match the setting (called at start and on every reload).
    pub(super) fn sync_system_window(&mut self) {
        let want = self.settings.widgets.system_window;
        tray::set_system_window_open(want);
        match (want, self.t.sys_win.is_some()) {
            (true, false) => self.open_system_window(),
            (false, true) => {
                self.t.sys_win = None;
                self.sync_stats_timer();
            }
            _ => {}
        }
    }

    fn open_system_window(&mut self) {
        let k = self.sys_scale();
        let saved: Option<SysSaved> =
            Some(store::load_json::<SysSaved>("system_window.json")).filter(|s| s.w > 0.0);
        let (min_w, max_w) = self.sys_limits();
        let default_w =
            widgets::system_window_width(self.sys_expected_cards(), widgets::SYS_CARD_W);
        let w = saved
            .as_ref()
            .map_or(default_w, |s| s.w)
            .mul_add(k, 0.0)
            .round()
            .clamp(min_w as f32, max_w as f32) as u32;
        let h = (widgets::SYS_WINDOW_H * k).round() as u32;
        // First time: just below Pillbox, centred on it.
        let (x, y) = saved.as_ref().map_or_else(
            || {
                (
                    self.win_pos.0 + (self.win_size.0 as i32 - w as i32) / 2,
                    self.win_pos.1 + self.win_size.1 as i32 + (12.0 * k) as i32,
                )
            },
            |s| (s.x, s.y),
        );
        match WidgetWindow::create_resizable(x, y, w, h, (min_w, max_w)) {
            Ok(win) => {
                win.show();
                self.t.sys_win = Some(win);
                self.sync_stats_timer();
                self.refresh_widgets(true);
            }
            Err(e) => tracing::warn!("System window: {e:#}"),
        }
    }

    fn save_system_window(&self) {
        let Some(win) = &self.t.sys_win else { return };
        let (x, y) = win.position();
        let saved = SysSaved {
            x,
            y,
            w: win.current_size().0 as f32 / self.sys_scale(),
        };
        if let Err(e) = store::save_json("system_window.json", &saved) {
            tracing::warn!("System window position not saved: {e:#}");
        }
    }

    /// The setting changes because the user opened or closed the window from Pillbox: keep it
    /// in `settings.json` like any other setting.
    fn set_system_window_setting(&mut self, on: bool) {
        self.settings.widgets.system_window = on;
        if let Err(e) = store::save(&self.settings) {
            tracing::warn!("could not save the System window setting: {e:#}");
        }
        self.sync_system_window();
    }

    pub(super) fn toggle_system_window(&mut self) {
        let on = !self.settings.widgets.system_window;
        self.set_system_window_setting(on);
        if on {
            self.show_toast(
                Toast::icon("\u{E8A7}", "System window: drag it, pull its right edge"),
                2500,
            );
        }
    }

    pub(super) fn is_system_window(&self, id: isize) -> bool {
        self.t.sys_win.as_ref().is_some_and(|w| w.id() == id)
    }

    pub(super) fn close_system_window(&mut self) {
        self.save_system_window();
        self.set_system_window_setting(false);
    }

    /// A click inside the System window: only its close button does anything.
    pub(super) fn on_system_click(&mut self, x: f32, y: f32) {
        let Some(win) = &self.t.sys_win else { return };
        let (l, t, r, b) = widgets::system_close_rect(win.size.0 as f32, self.sys_scale());
        if x >= l && x < r && y >= t && y < b {
            self.close_system_window();
        }
    }

    /// The window's right edge was dragged: redraw at the new width.
    pub(super) fn on_system_resized(&mut self) {
        if let Some(win) = &mut self.t.sys_win
            && win.sync_size()
        {
            self.t.widgets_dirty = true;
            self.dirty = true;
        }
    }

    pub(super) fn on_system_moved(&mut self) {
        self.on_system_resized();
        self.save_system_window();
    }
}

impl App {
    /// How many cards the System tab will show. The battery comes from the latest reading when
    /// there is one (a battery can be pulled from some laptops); the GPU from the adapter list.
    pub(super) fn system_tab_cards(&self) -> usize {
        if self.t.wdata.cpu.is_some() && self.t.gpu_card.is_some() {
            widgets::system_card_count(&self.t.wdata)
        } else {
            self.sys_expected_cards()
        }
    }
}
