//! M5: fullscreen games. Detection decides between the normal Pillbox, the stats strip and
//! hiding; the strip's numbers are sampled ten times a second, and only while it shows.

use std::time::Instant;

use pillbox_core::{
    calendar,
    games::{self, Smooth},
    playtime::{self, PlayLog},
    settings::{GameBehavior, RuleBehavior, StripBackground},
};

use super::{App, Mode};
use crate::{
    platform::{
        foreground,
        fullscreen::{self, FullscreenApp},
        gpu::GpuMonitor,
        info,
        system::{self, CpuMeter},
    },
    scene::StripView,
    store,
};

/// Time in fullscreen games: the log and the game being played now (since when).
pub(super) struct Play {
    pub log: PlayLog,
    pub current: Option<(String, Instant)>,
}

impl Play {
    pub fn load() -> Self {
        let mut log: PlayLog = store::load_json("playtime.json");
        // Browsers and launcher helpers were counted before play time knew they are not games.
        log.remove_non_games();
        Self { log, current: None }
    }
}

pub(super) const TIMER_STRIP: usize = 30;
pub(super) const TIMER_FULLSCREEN: usize = 31;
/// Strip refresh (CPU and RAM): ten times a second.
const STRIP_MS: u32 = 100;
/// Network speeds on the strip: twice a second (see sample_strip).
const NET_INTERVAL: std::time::Duration = std::time::Duration::from_millis(490);
/// Going fullscreen does not always change the foreground window (F11, a game switching
/// from windowed to borderless), so the check also runs on this slow timer. It costs a few
/// window-manager calls and draws nothing.
const FULLSCREEN_RECHECK_MS: u32 = 2000;

/// Readings behind the strip.
#[derive(Default)]
pub(super) struct Strip {
    cpu: CpuMeter,
    cpu_avg: Smooth,
    /// When and what the network counters were at the last sample.
    net_last: Option<(Instant, u64, u64)>,
    net: info::NetCounter,
    /// GPU load and temperature, read on a worker thread while the strip shows.
    gpu: Option<GpuMonitor>,
    down: Smooth,
    up: Smooth,
    pub view: StripView,
}

impl Strip {
    fn reset(&mut self) {
        *self = Self::default();
    }
}

impl App {
    /// Which mode the foreground app asks for: per-app rules first, then fullscreen games.
    /// A ringing reminder shows over everything. Fullscreen detection always runs first, so
    /// play time is counted whatever Pillbox ends up showing.
    pub(super) fn mode_for_foreground(&mut self) -> Mode {
        let found = if self.settings.games.enabled {
            fullscreen::detect(self.monitor.handle)
        } else {
            None
        };
        if found != self.fullscreen {
            match &found {
                Some(app) => tracing::info!(
                    "fullscreen app in front: {} ({})",
                    app.exe.as_deref().unwrap_or("unknown"),
                    if app.exclusive {
                        "exclusive"
                    } else {
                        "borderless"
                    }
                ),
                None => tracing::info!("no fullscreen app in front"),
            }
            self.fullscreen = found;
        }
        let behavior = self
            .fullscreen
            .as_ref()
            .and_then(|FullscreenApp { exe, exclusive }| {
                games::behavior_for(&self.settings, exe.as_deref(), *exclusive)
            });
        // Play time counts games only: a browser or a launcher's helper that is fullscreen
        // changes how Pillbox behaves (when "every fullscreen app" is on) but is not play.
        let game = behavior.and_then(|_| {
            self.fullscreen
                .as_ref()
                .filter(|f| !f.exe.as_deref().is_some_and(games::is_non_game_exe))
                .map(|f| {
                    f.exe
                        .as_deref()
                        .map_or_else(|| "Game".to_string(), playtime::game_name)
                })
        });
        self.track_play(game);
        if !self.t.ringing.is_empty() {
            return Mode::Normal;
        }
        let exe = foreground::foreground_exe();
        if let Some(rule) = exe.as_deref().and_then(|e| self.settings.rule_for(e)) {
            return match rule {
                RuleBehavior::Hide => Mode::Hidden,
                RuleBehavior::Strip => Mode::Strip,
                RuleBehavior::Show => Mode::Normal,
            };
        }
        match behavior {
            Some(GameBehavior::Strip) => Mode::Strip,
            Some(GameBehavior::Hide) => Mode::Hidden,
            None => Mode::Normal,
        }
    }

    /// The game in front changed (or not): close the old session, start the new one.
    fn track_play(&mut self, game: Option<String>) {
        if self.play.current.as_ref().map(|(g, _)| g) == game.as_ref() {
            return;
        }
        self.flush_play();
        if let Some(g) = &game {
            tracing::info!("play time: {g} started");
        }
        self.play.current = game.map(|g| (g, Instant::now()));
        self.refresh_widgets(false);
    }

    /// Add the time since the last flush to today's log and save it. Called when a game
    /// ends, every minute, and on quit.
    pub(super) fn flush_play(&mut self) {
        let Some((game, since)) = &mut self.play.current else {
            return;
        };
        let seconds = since.elapsed().as_secs();
        if seconds == 0 {
            return;
        }
        // Keep the fraction of a second for the next flush.
        *since += std::time::Duration::from_secs(seconds);
        let today = calendar::date_of(system::local_time());
        self.play.log.add(today, game, seconds);
        if let Err(e) = store::save_json("playtime.json", &self.play.log) {
            tracing::warn!("play time not saved: {e:#}");
        }
    }
    /// Re-check the foreground app and switch modes if needed.
    pub(super) fn refresh_mode(&mut self) {
        let mode = self.mode_for_foreground();
        if mode == self.mode {
            return;
        }
        tracing::info!("Pillbox mode: {mode:?}");
        self.mode = mode;
        if mode != Mode::Normal {
            self.hovering = false;
            self.open = false;
            self.scrub = None;
            self.unfocus(true);
        }
        self.retarget();
        self.sync_visibility();
        self.sync_strip();
        self.sync_drawers_for_mode();
        self.sync_notes_pause();
        self.dirty = true;
    }

    /// The slow fullscreen re-check runs while detection is on.
    pub(super) fn sync_fullscreen_timer(&mut self) {
        if self.settings.games.enabled {
            self.window
                .set_timer(TIMER_FULLSCREEN, FULLSCREEN_RECHECK_MS);
        } else {
            self.window.kill_timer(TIMER_FULLSCREEN);
            self.fullscreen = None;
        }
    }

    /// Sample the strip's numbers only while the strip is on screen.
    pub(super) fn sync_strip(&mut self) {
        let g = &self.settings.games;
        self.strip.view.usage_bars = g.usage_bars;
        self.strip.view.numbers_only = g.strip_background == StripBackground::NumbersOnly;
        if self.mode == Mode::Strip && self.effective_visible() {
            if self.strip.gpu.is_none() {
                self.strip.gpu = Some(GpuMonitor::start());
            }
            self.sample_strip();
            self.window.set_timer(TIMER_STRIP, STRIP_MS);
        } else {
            self.window.kill_timer(TIMER_STRIP);
            let keep = self.strip.view.clone();
            self.strip.reset();
            self.strip.view = StripView {
                cpu: None,
                ram: None,
                gpu: None,
                gpu_temp: None,
                down: None,
                up: None,
                ..keep
            };
        }
    }

    pub(super) fn sample_strip(&mut self) {
        let before = self.strip.view.clone();
        let st = &mut self.strip;
        if let Some(c) = st.cpu.sample() {
            // Whole percent: what is shown, so an unchanged reading costs no redraw.
            st.view.cpu = Some((st.cpu_avg.push(f64::from(c)) as f32).round());
        }
        st.view.ram = info::memory().map(|(used, total)| (used / total.max(0.001) * 100.0).round());
        if let Some(g) = st.gpu.as_ref().map(GpuMonitor::stats) {
            st.view.gpu = g.load.map(f32::round);
            st.view.gpu_temp = g.temp_c.map(f32::round);
        }
        // Network speeds change on nearly every tenth of a second, and each change costs a
        // redraw, so they are read twice a second, averaged over that half second.
        let net_due = st
            .net_last
            .is_none_or(|(then, _, _)| then.elapsed() >= NET_INTERVAL);
        if net_due && let Some((down, up)) = st.net.bytes() {
            let now = Instant::now();
            if let Some((then, d0, u0)) = st.net_last {
                let secs = now.duration_since(then).as_secs_f64().max(0.01);
                let bits = self.settings.widgets.network_mbps;
                let d = st.down.push(down.saturating_sub(d0) as f64 / secs);
                let u = st.up.push(up.saturating_sub(u0) as f64 / secs);
                st.view.down = Some(games::format_rate(d, bits));
                st.view.up = Some(games::format_rate(u, bits));
            }
            st.net_last = Some((now, down, up));
        }
        // Redraw only when something on the strip changed.
        if self.strip.view != before {
            self.dirty = true;
        }
    }
}
