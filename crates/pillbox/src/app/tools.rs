//! The M4 daily tools on top of Pillbox: clipboard history, shelf, reminders, controls,
//! text boxes, scrolling and the Home widgets. A child module of `app`, so it works on the
//! same `App` state. No `unsafe` here.

use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    time::Instant,
};

use pillbox_core::{
    astro, calc,
    calendar::{self, Minutes},
    cliphistory::{self, ClipFilter, ClipHistory, ClipItem, ClipKind, Recorded},
    face,
    image::Bgra,
    links::BtDevice,
    playtime,
    reminders::{self, Reminders},
    settings::WidgetKind,
    sysinfo::NetPicker,
    textedit::{EditKey, TextEdit},
    theme,
};
use serde::{Deserialize, Serialize};

use super::{App, Job, TOAST_MS, Toast};
use crate::{
    lyrics::LyricsState,
    platform::{
        clipboard::{self, Content, Skip},
        controls::{self, Controls},
        devices,
        diskio::DiskMonitor,
        dpapi, dragdrop,
        gpu::{self, GpuMonitor},
        info,
        links::{self, Links, WifiList},
        ocr, screen,
        system::{self, CpuMeter},
        thumbs::{self, ThumbLoader},
        widgetwin::WidgetWindow,
    },
    scene::{Action, Field, LinkPanel, Tab, WidgetAction},
    store,
    tabs::{Edits, clip_thumb_key, file_thumb_key},
    widgets::{self, PHOTO_KEY, PlayDayView, WidgetData, WorldTime},
};

pub(super) const TIMER_STATS: usize = 20;
pub(super) const TIMER_FOCUS: usize = 21;
pub(super) const TIMER_CLIP_RETRY: usize = 22;
pub(super) const TIMER_ONLINE: usize = 23;
pub(super) const TIMER_PHOTO: usize = 24;
/// While a Wi-Fi or Bluetooth list is open: look again at the networks or devices.
pub(super) const TIMER_LINKS: usize = 25;
const LINKS_REFRESH_MS: u32 = 4000;

/// Unpinned pictures kept in clipboard history (they are the large items).
const MAX_PICTURES: usize = 30;
/// Days the expanded Play time card offers: today and the six before.
const PLAY_DAYS: usize = 7;
/// Longest text kept per clipboard item.
const MAX_CLIP_TEXT: usize = 100_000;
/// Longest text in a text box.
const MAX_FIELD: usize = 500;
const THUMB_SIDE: u32 = 96;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct TodoItem {
    id: u64,
    text: String,
    done: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct TodoList {
    next_id: u64,
    items: Vec<TodoItem>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct ScreenTime {
    /// `YYYY-MM-DD` the count belongs to.
    date: String,
    seconds: u64,
}

/// What a slider drag is moving.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum SliderKind {
    Volume,
    Brightness,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct SliderDrag {
    kind: SliderKind,
    left: f32,
    right: f32,
}

/// A press on a shelf file that may become a drag.
#[derive(Debug, Clone, Copy)]
struct Press {
    index: usize,
    x: f32,
    y: f32,
}

/// Results of the network widgets' fetches.
#[derive(Debug, Clone, Default)]
pub(super) struct Online {
    ping: Option<Result<u32, String>>,
    currency: Vec<(String, f64)>,
    air: Option<String>,
    websites: Vec<(String, Option<bool>)>,
}

#[derive(Default)]
struct FocusTimer {
    /// Seconds left when last paused (or the full length).
    left: u32,
    running_since: Option<Instant>,
}

pub(super) struct Tools {
    pub(super) edits: Edits,
    pub(super) focus: Option<Field>,
    /// The desktop widget window whose text box has the keyboard (None = Pillbox's).
    pub(super) focus_widget: Option<isize>,
    /// The other app's window that had the keyboard before a text box took it.
    keyboard_return: Option<isize>,
    pub(super) pointer: Option<(f32, f32)>,
    /// Where the left button went down on the card and is still held (the pressed look).
    pub(super) pressed: Option<(f32, f32)>,
    hovered_hit: Option<usize>,
    scroll: HashMap<Tab, f32>,
    pub(super) scroll_max: f32,
    /// Wheel travel over the tab row not yet turned into a tab switch (touchpads send small
    /// steps; one notch is 1.0).
    tab_wheel: pillbox_core::wheel::WheelSteps,
    pub(super) slider: Option<SliderDrag>,
    // Clipboard.
    pub(super) clip: ClipHistory,
    pub(super) clip_filter: ClipFilter,
    clip_thumbs: HashMap<u64, Bgra>,
    clip_retries: u32,
    // Shelf.
    pub(super) shelf_selected: Vec<bool>,
    /// The right-click menu is open on this shelf file.
    pub(super) shelf_menu: Option<usize>,
    press: Option<Press>,
    // Reminders.
    pub(super) reminders: Reminders,
    pub(super) ringing: Vec<u64>,
    pub(super) reminder_error: Option<String>,
    // Controls.
    controls: Option<Controls>,
    pub(super) ctl: controls::State,
    pub(super) volume: Option<(f32, bool)>,
    /// Wi-Fi networks and paired Bluetooth devices (see `platform/links.rs`).
    links: Option<Links>,
    pub(super) wifi: WifiList,
    pub(super) bluetooth: Vec<BtDevice>,
    pub(super) wifi_loaded: bool,
    pub(super) bluetooth_loaded: bool,
    /// The list open under the cards of the Controls tab.
    pub(super) link_panel: Option<LinkPanel>,
    /// The network or device being switched, and what went wrong with the last switch.
    pub(super) link_busy: Option<String>,
    pub(super) link_note: Option<String>,
    // Widgets.
    pub(super) wdata: WidgetData,
    /// The Play time card on the Home tab is expanded, and shows this day (0 = today).
    pub(super) play_open: bool,
    pub(super) play_day: usize,
    todos: TodoList,
    focus_timer: FocusTimer,
    cpu: CpuMeter,
    net_last: Option<(Instant, u64, u64)>,
    /// System card: which connection to show, and its rates; `sys_clock` is its time base.
    sys_net: NetPicker,
    sys_clock: Instant,
    /// GPU and disk readings for the System card; they exist only while the card is on screen.
    gpu: Option<GpuMonitor>,
    /// The graphics card the System cards show (name, integrated), asked for each time the
    /// monitor starts. The outer `None` = not asked yet; `Some(None)` = the PC has no card.
    pub(super) gpu_card: Option<Option<(String, bool)>>,
    disk: Option<DiskMonitor>,
    screen_time: ScreenTime,
    online: Online,
    online_at: Option<Instant>,
    photos: Vec<PathBuf>,
    photo_index: usize,
    // Thumbnails of files and photos.
    pub(super) thumbs: ThumbLoader,
    requested: HashSet<u64>,
    /// Widgets popped out onto the desktop.
    popped: Vec<(WidgetKind, WidgetWindow)>,
    /// The System window (see `app/sysview.rs`).
    pub(super) sys_win: Option<WidgetWindow>,
    /// The buttons each popped-out widget drew last, by window id.
    popped_hits: HashMap<isize, Vec<crate::scene::Hit>>,
    pub(super) widgets_dirty: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PoppedSaved {
    kind: WidgetKind,
    x: i32,
    y: i32,
}

fn clip_dir() -> Option<PathBuf> {
    let d = store::data_dir().ok()?.join("clipboard");
    std::fs::create_dir_all(&d).ok()?;
    Some(d)
}

fn picture_path(id: u64) -> Option<PathBuf> {
    Some(clip_dir()?.join(format!("pic-{id}.bin")))
}

fn load_clip_history() -> ClipHistory {
    let Some(path) = clip_dir().map(|d| d.join("index.bin")) else {
        return ClipHistory::default();
    };
    let Ok(sealed) = std::fs::read(&path) else {
        return ClipHistory::default();
    };
    match dpapi::unprotect(&sealed).map(|plain| serde_json::from_slice::<ClipHistory>(&plain)) {
        Ok(Ok(h)) => h,
        Ok(Err(e)) => {
            tracing::warn!("clipboard history is damaged ({e}); starting empty");
            ClipHistory::default()
        }
        Err(e) => {
            tracing::warn!("clipboard history cannot be decrypted ({e:#}); starting empty");
            ClipHistory::default()
        }
    }
}

/// Read a stored clipboard picture back (decrypt, decode).
fn load_picture(id: u64) -> Option<Bgra> {
    Bgra::from_png(&picture_png(id)?).ok()
}

/// A stored clipboard picture as the PNG it was saved as (decrypted).
fn picture_png(id: u64) -> Option<Vec<u8>> {
    let sealed = std::fs::read(picture_path(id)?).ok()?;
    dpapi::unprotect(&sealed).ok()
}

/// Where right-click "open" writes a clipboard text or picture so another app can open it:
/// `%TEMP%\Pillbox\opened`. Copies older than a day are removed each time one is written
/// (the history itself stays encrypted; these copies are plain, for the app that opens them).
fn opened_copy(name: &str, bytes: &[u8]) -> anyhow::Result<PathBuf> {
    let dir = std::env::temp_dir().join("Pillbox").join("opened");
    std::fs::create_dir_all(&dir)?;
    if let Ok(entries) = std::fs::read_dir(&dir) {
        let day = std::time::Duration::from_secs(24 * 60 * 60);
        for e in entries.flatten() {
            let old = e
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.elapsed().ok())
                .is_some_and(|age| age > day);
            if old {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    let path = dir.join(name);
    std::fs::write(&path, bytes)?;
    Ok(path)
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

fn today_string() -> String {
    let t = system::local_time();
    format!("{:04}-{:02}-{:02}", t.year, t.month, t.day)
}

fn now_minutes() -> Minutes {
    calendar::now_minutes(system::local_time())
}

impl Tools {
    pub(super) fn new(waker: crate::platform::window::Waker) -> Self {
        let mut edits = Edits::default();
        if let Some(note) = store::load_bytes("note.txt") {
            edits.note = TextEdit::new(&String::from_utf8_lossy(&note));
        }
        let mut screen_time: ScreenTime = store::load_json("screentime.json");
        if screen_time.date != today_string() {
            screen_time = ScreenTime {
                date: today_string(),
                seconds: 0,
            };
        }
        Self {
            edits,
            focus: None,
            focus_widget: None,
            keyboard_return: None,
            pointer: None,
            pressed: None,
            hovered_hit: None,
            scroll: HashMap::new(),
            scroll_max: 0.0,
            tab_wheel: pillbox_core::wheel::WheelSteps::default(),
            slider: None,
            clip: load_clip_history(),
            clip_filter: ClipFilter::All,
            clip_thumbs: HashMap::new(),
            clip_retries: 0,
            shelf_selected: Vec::new(),
            shelf_menu: None,
            press: None,
            reminders: store::load_json("reminders.json"),
            ringing: Vec::new(),
            reminder_error: None,
            controls: None,
            ctl: controls::State::default(),
            volume: None,
            links: None,
            wifi: WifiList::default(),
            bluetooth: Vec::new(),
            wifi_loaded: false,
            bluetooth_loaded: false,
            link_panel: None,
            link_busy: None,
            link_note: None,
            wdata: WidgetData::default(),
            play_open: false,
            play_day: 0,
            todos: store::load_json("todos.json"),
            focus_timer: FocusTimer::default(),
            cpu: CpuMeter::default(),
            net_last: None,
            sys_net: NetPicker::default(),
            sys_clock: Instant::now(),
            gpu: None,
            gpu_card: None,
            disk: None,
            screen_time,
            online: Online::default(),
            online_at: None,
            photos: Vec::new(),
            photo_index: 0,
            thumbs: ThumbLoader::start(waker),
            requested: HashSet::new(),
            popped: Vec::new(),
            sys_win: None,
            popped_hits: HashMap::new(),
            widgets_dirty: true,
        }
    }

    pub(super) fn scroll(&self, tab: Tab) -> f32 {
        self.scroll.get(&tab).copied().unwrap_or(0.0)
    }
}

impl App {
    // ---- start-up --------------------------------------------------------------------------

    /// Work that needs the window: clipboard thumbnails, the clipboard listener, timers.
    pub(super) fn start_tools(&mut self) {
        self.sync_clipboard_listener();
        // Decrypt and shrink stored clipboard pictures in the background.
        let ids: Vec<u64> = self
            .t
            .clip
            .items
            .iter()
            .filter(|i| i.kind == ClipKind::Picture)
            .map(|i| i.id)
            .collect();
        if !ids.is_empty() {
            let tx = self.jobs_tx.clone();
            let waker = self.window.waker();
            let _ = std::thread::Builder::new()
                .name("clip-thumbs".into())
                .spawn(move || {
                    for id in ids {
                        if let Some(img) = load_picture(id) {
                            let _ = tx.send(Job::ClipThumb(id, img.thumbnail(THUMB_SIDE)));
                        }
                    }
                    waker.wake();
                });
        }
        self.t.focus_timer.left = self.settings.widgets.focus_minutes * 60;
        let saved: Vec<PoppedSaved> = store::load_json("widgets.json");
        for w in saved {
            self.open_widget_window(w.kind, w.x, w.y);
        }
        self.check_reminders();
        self.refresh_widgets(false);
        self.window.set_timer(TIMER_ONLINE, 10 * 60 * 1000);
        self.window.set_timer(TIMER_PHOTO, 10 * 60 * 1000);
    }

    pub(super) fn sync_clipboard_listener(&mut self) {
        self.window
            .listen_clipboard(self.settings.clipboard.enabled);
        self.expire_clips();
    }

    /// Drop unpinned clipboard items older than `clipboard.expire_days`. Runs at start, when
    /// the settings change, on each new copy and when the Clipboard tab opens (the history is
    /// only seen there), so an expired item never shows.
    pub(super) fn expire_clips(&mut self) {
        let removed = self
            .t
            .clip
            .expire(unix_now(), self.settings.clipboard.expire_days);
        if !removed.is_empty() {
            tracing::info!("{} clipboard items expired", removed.len());
            self.forget_pictures(&removed);
            self.save_clip_index();
            self.dirty = true;
        }
    }

    /// Tabs shown in the card, in order.
    pub(super) fn tab_list(&self) -> Vec<Tab> {
        let mut tabs = vec![Tab::Home, Tab::Music];
        if self.settings.clipboard.enabled {
            tabs.push(Tab::Clipboard);
        }
        tabs.extend([Tab::Shelf, Tab::Reminders, Tab::Controls, Tab::System]);
        if self.settings.script.enabled {
            tabs.push(Tab::Activity);
        }
        if self.settings.notifications.enabled {
            tabs.push(Tab::Notifications);
        }
        tabs
    }

    /// A tab came into view: load what it shows.
    pub(super) fn tab_shown(&mut self) {
        // The Music tab's glow needs the audio capture; other tabs do not.
        self.sync_music();
        match self.tab {
            Tab::Activity => self.refresh_agent_presence(),
            Tab::Clipboard => self.expire_clips(),
            Tab::System => {
                // Start the GPU and disk readers first, so this first refresh already knows
                // which cards there are (the card count must not change after opening).
                self.sync_stats_timer();
                self.refresh_widgets(true);
            }
            Tab::Controls => {
                if self.t.controls.is_none() {
                    self.t.controls = Controls::start(self.window.waker());
                }
                if let Some(c) = &self.t.controls {
                    c.refresh();
                }
                if self.t.links.is_none() {
                    self.t.links = Links::start(self.window.waker());
                }
                if let Some(l) = &self.t.links {
                    l.refresh_wifi(false);
                    l.refresh_bluetooth();
                }
                self.t.volume = devices::speaker();
                self.mic_muted = devices::mic_muted().unwrap_or(self.mic_muted);
                let paths: Vec<PathBuf> = self
                    .settings
                    .launcher
                    .slots
                    .iter()
                    .map(|s| PathBuf::from(&s.path))
                    .collect();
                for p in paths {
                    self.request_file_thumb(&p, 48);
                }
            }
            Tab::Shelf => {
                for p in self.shelf.clone() {
                    self.request_file_thumb(&p, 64);
                }
            }
            Tab::Home => {
                self.refresh_widgets(true);
                if self.settings.widgets.shown.contains(&WidgetKind::Photos)
                    && self.t.photos.is_empty()
                {
                    self.next_photo();
                }
            }
            _ => {}
        }
        self.sync_stats_timer();
    }

    /// PC stats refresh only while they are on screen: every `widgets.system_refresh_ms` while
    /// the System tab or window shows them, else every two seconds.
    pub(super) fn sync_stats_timer(&mut self) {
        // Called whenever the card opens, closes or changes tab: the Activity tab's own check
        // follows the same moments.
        self.sync_presence_timer();
        self.sync_links_timer();
        let stat_kind = |k: &WidgetKind| matches!(k, WidgetKind::PcStats);
        // The GPU and disk counters are only read while the System tab or window is open.
        let system_shown = self.t.sys_win.is_some() || (self.open && self.tab == Tab::System);
        let shown = system_shown
            || (self.open
                && self.tab == Tab::Home
                && self.settings.widgets.shown.iter().any(stat_kind))
            || self.t.popped.iter().any(|(k, _)| stat_kind(k));
        if shown {
            let ms = if system_shown {
                self.settings.widgets.system_refresh_ms
            } else {
                2000
            };
            // Setting a running timer again just restarts it with the new period.
            self.window.set_timer(TIMER_STATS, ms);
        } else {
            self.window.kill_timer(TIMER_STATS);
        }
        if system_shown {
            if self.t.gpu.is_none() {
                self.t.gpu = Some(GpuMonitor::start());
                self.t.gpu_card = Some(gpu::shown_card());
            }
            if self.t.disk.is_none() {
                self.t.disk = DiskMonitor::new();
            }
        } else if self.t.gpu.is_some() || self.t.disk.is_some() {
            self.t.gpu = None;
            self.t.disk = None;
            // Old figures would show for the first frame of the next visit and then jump; show
            // "…" until fresh ones arrive instead. The cards themselves stay (same count).
            let sys = &mut self.t.wdata.sys;
            sys.gpu = None;
            sys.disk = None;
            if let Some((_, rates, _)) = &mut sys.net {
                *rates = None;
            }
        }
    }

    pub(super) fn request_file_thumb(&mut self, path: &Path, size: u32) {
        let key = file_thumb_key(path);
        if path.as_os_str().is_empty() || !self.t.requested.insert(key) {
            return;
        }
        self.t.thumbs.request(thumbs::Job::File {
            key,
            path: path.to_path_buf(),
            size: (size as f32 * self.monitor.scale) as u32,
        });
    }

    pub(super) fn poll_thumbs(&mut self) {
        for done in self.t.thumbs.poll() {
            if let Some(img) = done.image {
                self.renderer.set_image(
                    crate::platform::gfx::ImageSlot::Thumb(done.key),
                    &img.pixels,
                    img.width,
                    img.height,
                );
                self.icon_ready(done.key, &img);
                if done.key == PHOTO_KEY {
                    self.t.wdata.photo = true;
                }
                self.dirty = true;
            }
        }
    }

    /// After the GPU device was rebuilt every uploaded thumbnail is gone.
    pub(super) fn reupload_thumbs(&mut self) {
        self.t.requested.clear();
        for (id, img) in &self.t.clip_thumbs {
            self.renderer.set_image(
                crate::platform::gfx::ImageSlot::Thumb(clip_thumb_key(*id)),
                &img.pixels,
                img.width,
                img.height,
            );
        }
        self.reupload_icons();
        self.t.wdata.photo = false;
        self.tab_shown();
    }

    // ---- pointer, wheel, keyboard ---------------------------------------------------------

    /// Pointer moved over the window while the card is open: hover, slider drags, shelf drags.
    pub(super) fn tools_mouse_move(&mut self, x: f32, y: f32) {
        self.t.pointer = Some((x, y));
        if let Some(drag) = self.t.slider {
            self.slider_set(drag, x);
            return;
        }
        if let Some(p) = self.t.press
            && ((x - p.x).powi(2) + (y - p.y).powi(2)).sqrt() > 6.0 * self.monitor.scale
        {
            self.t.press = None;
            self.shelf_drag_out(p.index);
            return;
        }
        // Rows, tiles and buttons light up under the pointer, and some buttons only exist
        // while their row is hovered, so the list tabs redraw as the pointer moves over them
        // (only while it moves; nothing runs when it rests).
        let hovered = self.hits.iter().rposition(|h| h.contains(x, y));
        if hovered != self.t.hovered_hit || self.tab.tall() {
            self.t.hovered_hit = hovered;
            self.dirty = true;
        }
    }

    pub(super) fn tools_mouse_left(&mut self) {
        self.t.pressed = None;
        if self.t.pointer.take().is_some() {
            self.t.hovered_hit = None;
            self.dirty = true;
        }
    }

    /// Button released: ends slider drags; a shelf press that did not move selects the file.
    pub(super) fn tools_mouse_up(&mut self) {
        self.t.slider = None;
        if self.t.pressed.take().is_some() {
            self.dirty = true;
        }
        if let Some(p) = self.t.press.take()
            && let Some(sel) = self.t.shelf_selected.get_mut(p.index)
        {
            *sel = !*sel;
            self.dirty = true;
        }
    }

    pub(super) fn on_wheel(&mut self, x: f32, y: f32, delta: f32) {
        if !self.open {
            // On the closed pill the wheel flips through stacked notifications.
            self.flip_stack(delta);
            return;
        }
        if self.on_tab_row(x, y) {
            self.wheel_tabs(delta);
            return;
        }
        self.t.tab_wheel.reset();
        // Over the lyrics the wheel scrolls the words, not the card.
        if self.tab == Tab::Music
            && self
                .hits
                .iter()
                .any(|h| matches!(h.action, Action::LyricsArea) && h.contains(x, y))
        {
            self.scroll_lyrics(delta);
            return;
        }
        let step = 48.0 * self.monitor.scale;
        let s = self.t.scroll.entry(self.tab).or_insert(0.0);
        *s = (*s - delta * step).clamp(0.0, self.t.scroll_max.max(0.0));
        self.dirty = true;
    }

    /// The wheel over the lyrics: move the words by hand (two lines a notch). The view stays
    /// where it was put until "Return to live" is clicked, or the wheel brings it back to the
    /// line being sung.
    fn scroll_lyrics(&mut self, delta: f32) {
        let (Some(track), LyricsState::Found(lyrics)) = (&self.track, &self.lyrics.state) else {
            return;
        };
        let live = lyrics
            .smooth_index(track.position_now() as f32, 0.35)
            .max(0.0);
        let last = lyrics.lines.len().saturating_sub(1) as f32;
        let view = (self.lyrics_view.unwrap_or(live) - delta * 2.0).clamp(0.0, last);
        // Back on the line being sung: follow the song again.
        self.lyrics_view = ((view - live).abs() > 0.3).then_some(view);
        self.dirty = true;
    }

    /// Switch the open card to `tab` (a click on its icon, or the wheel over the tab row).
    pub(super) fn show_tab(&mut self, tab: Tab) {
        if self.pillbox_typing() {
            self.unfocus(true);
        }
        self.tab = tab;
        self.dirty = true;
        self.tab_shown();
        self.retarget();
        if self.ambient_running() {
            self.start_animating();
        }
    }

    /// Whether the pointer is on the tab row: the height band of the tab icons, across the card
    /// (the gear and the X share that band).
    fn on_tab_row(&self, x: f32, y: f32) -> bool {
        let mut band: Option<(f32, f32, f32, f32)> = None;
        for h in self
            .hits
            .iter()
            .filter(|h| matches!(h.action, Action::Tab(_)))
        {
            let (l, t, r, b) = h.rect;
            band = Some(match band {
                None => (l, t, r, b),
                Some((bl, bt, br, bb)) => (bl.min(l), bt.min(t), br.max(r), bb.max(b)),
            });
        }
        // The band starts a little left of the first icon and runs to the window's right edge
        // (the window is only as wide as the widest card, so that is the card's edge too).
        let Some((l, t, _r, b)) = band else {
            return false;
        };
        y >= t && y <= b && x >= l - 12.0 * self.monitor.scale
    }

    /// The wheel over the tab row: down goes to the next tab, up to the previous. Stops at the
    /// ends instead of wrapping, so a long flick lands on the first or last tab.
    fn wheel_tabs(&mut self, delta: f32) {
        let steps = self.t.tab_wheel.add(delta);
        if steps == 0 {
            return;
        }
        let tabs = self.tab_list();
        let Some(at) = tabs.iter().position(|t| *t == self.tab) else {
            return;
        };
        // Wheel down (negative delta) moves right, to the next tab.
        let to = pillbox_core::wheel::step_clamped(at, tabs.len(), -steps);
        if to != at {
            self.show_tab(tabs[to]);
        }
    }

    /// Whether the card must stay open even though the pointer left it.
    pub(super) fn holds_open(&self) -> bool {
        self.pillbox_typing()
            || !self.t.ringing.is_empty()
            || self.t.slider.is_some()
            || self.phone.showing()
    }

    /// A text box in Pillbox itself (not in a desktop widget) has the keyboard.
    pub(super) fn pillbox_typing(&self) -> bool {
        self.t.focus.is_some() && self.t.focus_widget.is_none()
    }

    /// Give a text box the keyboard. `widget` is the desktop widget window it is in, or None
    /// for Pillbox.
    fn focus_field_in(&mut self, field: Field, widget: Option<isize>) {
        if self.t.focus == Some(field) && self.t.focus_widget == widget {
            return;
        }
        if self.t.focus.is_some() {
            // Moving between Pillbox and a widget: keep the keyboard, just switch owner.
            self.unfocus(false);
        }
        let ret = &mut self.t.keyboard_return;
        let taken = match widget {
            None => self.window.take_keyboard(ret),
            Some(id) => self
                .t
                .popped
                .iter()
                .find(|(_, w)| w.id() == id)
                .is_some_and(|(_, w)| w.take_keyboard(ret)),
        };
        if !taken {
            tracing::warn!("could not take the keyboard for a text box");
        }
        self.t.focus = Some(field);
        self.t.focus_widget = widget;
        self.t.widgets_dirty = true;
        self.dirty = true;
    }

    fn focus_field(&mut self, field: Field) {
        self.focus_field_in(field, None);
    }

    /// Leave the text box. `give_back` returns the keyboard to the app that had it.
    pub(super) fn unfocus(&mut self, give_back: bool) {
        let Some(field) = self.t.focus.take() else {
            return;
        };
        if field == Field::Note {
            self.save_note();
        }
        self.t.focus_widget = None;
        if give_back {
            self.window.give_back_keyboard(&mut self.t.keyboard_return);
        }
        self.t.widgets_dirty = true;
        self.dirty = true;
    }

    pub(super) fn on_focus_lost(&mut self) {
        if self.t.focus.is_none() {
            return;
        }
        self.unfocus(false);
        // The user clicked elsewhere; if the pointer is not on the card, let it close.
        if !self.pointer_over_pillbox() {
            self.pointer_left();
        }
    }

    pub(super) fn pointer_over_pillbox(&self) -> bool {
        let Some((x, y)) = screen::cursor_pos() else {
            return false;
        };
        let (lx, ly) = ((x - self.win_pos.0) as f32, (y - self.win_pos.1) as f32);
        self.pill().contains(lx, ly)
    }

    pub(super) fn on_char(&mut self, c: char) {
        let Some(field) = self.t.focus else { return };
        let mut buf = [0u8; 4];
        self.t
            .edits
            .get_mut(field)
            .insert(c.encode_utf8(&mut buf), MAX_FIELD);
        self.after_edit(field);
    }

    pub(super) fn on_key(&mut self, vk: u16, ctrl: bool) {
        let Some(field) = self.t.focus else { return };
        let key = match (vk, ctrl) {
            (0x08, true) => Some(EditKey::DeleteWordBack),
            (0x08, false) => Some(EditKey::Backspace),
            (0x2E, _) => Some(EditKey::Delete),
            (0x25, true) => Some(EditKey::WordLeft),
            (0x25, false) => Some(EditKey::Left),
            (0x27, true) => Some(EditKey::WordRight),
            (0x27, false) => Some(EditKey::Right),
            (0x24, _) => Some(EditKey::Home),
            (0x23, _) => Some(EditKey::End),
            _ => None,
        };
        if let Some(key) = key {
            self.t.edits.get_mut(field).key(key);
            self.after_edit(field);
            return;
        }
        match (vk, ctrl) {
            (0x0D, _) => self.submit(field),
            (0x1B, _) => {
                if field == Field::ClipSearch && !self.t.edits.clip_search.text.is_empty() {
                    self.t.edits.clip_search.clear();
                    self.after_edit(field);
                } else {
                    self.unfocus(true);
                }
            }
            (0x56, true) => {
                if let Some(text) = clipboard::read_text(self.window.hwnd()) {
                    self.t.edits.get_mut(field).insert(&text, MAX_FIELD);
                    self.after_edit(field);
                }
            }
            _ => {}
        }
    }

    fn after_edit(&mut self, field: Field) {
        match field {
            Field::ClipSearch => {
                self.t.scroll.insert(Tab::Clipboard, 0.0);
            }
            Field::ReminderAdd => self.t.reminder_error = None,
            Field::Calc => self.refresh_widgets(false),
            _ => {}
        }
        self.t.widgets_dirty = true;
        self.dirty = true;
    }

    fn submit(&mut self, field: Field) {
        match field {
            Field::ClipSearch => {
                // Enter copies the first match.
                let first = self
                    .t
                    .clip
                    .view(self.t.clip_filter, &self.t.edits.clip_search.text)
                    .first()
                    .map(|i| i.id);
                if let Some(id) = first {
                    self.clip_copy(id);
                }
            }
            Field::ReminderAdd => self.add_reminder(),
            Field::Todo => {
                let text = self.t.edits.todo.text.trim().to_string();
                if !text.is_empty() {
                    self.t.todos.next_id += 1;
                    let id = self.t.todos.next_id;
                    self.t.todos.items.insert(
                        0,
                        TodoItem {
                            id,
                            text,
                            done: false,
                        },
                    );
                    self.t.edits.todo.clear();
                    self.save_todos();
                    self.refresh_widgets(false);
                }
            }
            Field::Note => self.unfocus(true),
            Field::Calc => {
                if let Some(res) = self.t.wdata.calc_result.clone() {
                    self.copy_text(&res, "Result copied");
                }
            }
        }
        self.dirty = true;
    }

    // ---- clicks ----------------------------------------------------------------------------

    /// Clicks on the M4 parts of the card. Returns false for actions handled elsewhere.
    pub(super) fn act_tool(&mut self, action: Action, x: f32, y: f32) -> bool {
        // A click anywhere but the focused box ends typing (the click still does its thing).
        if self.t.focus.is_some() && !matches!(action, Action::Focus(_)) {
            self.unfocus(true);
        }
        match action {
            Action::Focus(field) => self.focus_field(field),
            Action::ClipFilter(f) => {
                self.t.clip_filter = f;
                self.t.scroll.insert(Tab::Clipboard, 0.0);
            }
            Action::ClipCopy(id) => self.clip_copy(id),
            Action::ClipPin(id) => {
                let pinned = self.t.clip.get(id).is_some_and(|i| i.pinned);
                self.t.clip.set_pinned(id, !pinned);
                self.save_clip_index();
            }
            Action::ClipDelete(id) => {
                if self.t.clip.remove(id) {
                    self.forget_pictures(&[id]);
                    self.save_clip_index();
                }
            }
            Action::ClipOcr(id) => self.clip_ocr(id),
            Action::ClipClear => {
                let removed = self.t.clip.clear_unpinned();
                self.forget_pictures(&removed);
                self.save_clip_index();
                self.show_toast(
                    Toast::icon("\u{E74D}", "History cleared (pinned kept)"),
                    TOAST_MS,
                );
            }
            Action::SnippetCopy(i) => {
                if let Some(s) = self.settings.clipboard.snippets.get(i).cloned() {
                    self.copy_text(&s.text, "Snippet copied");
                }
            }
            Action::ShelfItem(i) => self.t.press = Some(Press { index: i, x, y }),
            Action::ShelfSelectAll => {
                let all = self.t.shelf_selected.iter().all(|s| *s);
                self.t.shelf_selected.iter_mut().for_each(|s| *s = !all);
            }
            Action::ShelfRemove => self.shelf_remove(),
            Action::ShelfPhone(i) => self.phone_send(i),
            Action::PhoneReceive => self.phone_receive(),
            Action::PhoneClose => self.phone_close("closed"),
            Action::ShelfMenu(i, cmd) => self.shelf_command(i, cmd),
            Action::ShelfMenuClose => {}
            Action::ShelfCopy => {
                let files = self.shelf_targets(None);
                if !files.is_empty() {
                    match clipboard::write(self.window.hwnd(), &Content::Files(files)) {
                        Ok(()) => {
                            self.show_toast(Toast::icon("\u{E8C8}", "Files copied"), TOAST_MS)
                        }
                        Err(e) => tracing::warn!("shelf copy: {e:#}"),
                    }
                }
            }
            Action::ReminderAdd => self.add_reminder(),
            Action::ReminderDone(id) => self.answer_reminder(id, false),
            Action::ReminderSnooze(id) => self.answer_reminder(id, true),
            Action::ReminderDelete(id) => {
                self.t.reminders.remove(id);
                self.t.ringing.retain(|r| *r != id);
                self.save_reminders();
            }
            Action::Volume { left, right } => {
                let drag = SliderDrag {
                    kind: SliderKind::Volume,
                    left,
                    right,
                };
                self.t.slider = Some(drag);
                self.slider_set(drag, x);
            }
            Action::Brightness { left, right } => {
                let drag = SliderDrag {
                    kind: SliderKind::Brightness,
                    left,
                    right,
                };
                self.t.slider = Some(drag);
                self.slider_set(drag, x);
            }
            Action::SpeakerMute => {
                let muted = self.t.volume.is_some_and(|(_, m)| m);
                if let Err(e) = devices::set_speaker_muted(!muted) {
                    tracing::warn!("{e:#}");
                }
                self.t.volume = devices::speaker();
            }
            Action::MicMute => self.toggle_mic_mute(),
            Action::Wifi => {
                if let (Some(c), Some(on)) = (&self.t.controls, self.t.ctl.wifi) {
                    c.set_wifi(!on);
                }
            }
            Action::Bluetooth => {
                if let (Some(c), Some(on)) = (&self.t.controls, self.t.ctl.bluetooth) {
                    c.set_bluetooth(!on);
                }
            }
            Action::SoundSettings => {
                if let Err(e) = system::open_path("ms-settings:sound", "") {
                    tracing::warn!("{e:#}");
                }
            }
            Action::LinkPanel(panel) => self.toggle_link_panel(panel),
            Action::WifiJoin(i) => {
                let net = self.t.wifi.nets.get(i).filter(|n| !n.profile.is_empty());
                if let (Some(l), Some(net)) = (&self.t.links, net) {
                    l.wifi_join(&net.profile);
                    self.t.link_busy = Some(net.ssid.clone());
                    self.t.link_note = None;
                }
            }
            Action::WifiLeave => {
                if let Some(l) = &self.t.links {
                    l.wifi_leave();
                    self.t.link_busy = self
                        .t
                        .wifi
                        .nets
                        .iter()
                        .find(|n| n.connected)
                        .map(|n| n.ssid.clone());
                    self.t.link_note = None;
                }
            }
            Action::BtConnect(address) | Action::BtDisconnect(address) => {
                let connect = matches!(action, Action::BtConnect(_));
                if let Some(l) = &self.t.links {
                    if connect {
                        l.bt_connect(address);
                    } else {
                        l.bt_disconnect(address);
                    }
                    self.t.link_busy = self
                        .t
                        .bluetooth
                        .iter()
                        .find(|d| d.address == address)
                        .map(|d| d.name.clone());
                    self.t.link_note = None;
                }
            }
            Action::LinkSettings(panel) => {
                let page = match panel {
                    LinkPanel::Wifi => "ms-settings:network-wifi",
                    LinkPanel::Bluetooth => "ms-settings:bluetooth",
                };
                if let Err(e) = system::open_path(page, "") {
                    tracing::warn!("{e:#}");
                }
            }
            Action::Mirror => self.toggle_mirror(),
            Action::ChatDrawer => self.toggle_chat_drawer(),
            Action::Launch(i) => self.launch_slot(i),
            Action::OpenSettings => super::open_settings_window(),
            Action::Widget(w) => self.widget_action(w),
            _ => return false,
        }
        self.dirty = true;
        true
    }

    /// Right-click shortcuts: pin/unpin a clipboard item, the menu on a shelf file.
    pub(super) fn act_secondary(&mut self, action: Action) {
        match action {
            // A right-click on a clipboard item opens it (pinning is the row's pin button).
            Action::ClipCopy(id) => self.clip_open(id),
            Action::ShelfItem(i) | Action::ShelfPhone(i) | Action::ShelfMenu(i, _)
                if i < self.shelf.len() =>
            {
                self.t.shelf_menu = Some(i);
            }
            Action::ShelfMenuClose => self.t.shelf_menu = None,
            _ => return,
        }
        self.dirty = true;
    }

    /// The shelf file menu for painting, if it is open on a file that still exists in the list.
    pub(super) fn shelf_menu_ui(&self) -> Option<crate::tabs::ShelfMenuUi> {
        let index = self.t.shelf_menu?;
        let path = self.shelf.get(index)?;
        Some(crate::tabs::ShelfMenuUi {
            index,
            from_phone: self.came_from_phone(path),
        })
    }

    /// A line of the shelf file menu was clicked.
    pub(super) fn shelf_command(&mut self, index: usize, cmd: crate::scene::ShelfCmd) {
        use crate::scene::ShelfCmd as C;
        let Some(path) = self.shelf.get(index).cloned() else {
            return;
        };
        let text = path.to_string_lossy().to_string();
        let failed = |what: &str, e: anyhow::Error| {
            tracing::warn!("shelf {what}: {e:#}");
        };
        match cmd {
            C::Open => {
                if let Err(e) = system::open_path(&text, "") {
                    failed("open", e);
                    self.show_toast(Toast::text("Could not open the file"), TOAST_MS);
                }
            }
            C::OpenWith => system::open_with(path),
            C::ShowInFolder => {
                if let Err(e) = system::open_path("explorer.exe", &format!("/select,\"{text}\"")) {
                    failed("show in folder", e);
                }
            }
            C::Download => self.download_copy(path),
            C::SendToPhone => self.phone_send(index),
            C::Remove => {
                self.shelf.remove(index);
                self.sync_shelf_selection();
                self.save_shelf();
            }
        }
        self.dirty = true;
    }

    /// The file is in the folder phone uploads are saved in.
    fn came_from_phone(&self, path: &Path) -> bool {
        let custom = self.settings.phone.receive_folder.trim();
        let dir = if custom.is_empty() {
            match crate::platform::net::downloads_dir() {
                Ok(d) => d.join("Pillbox"),
                Err(_) => return false,
            }
        } else {
            PathBuf::from(custom)
        };
        path.parent().is_some_and(|p| {
            p.to_string_lossy()
                .eq_ignore_ascii_case(&dir.to_string_lossy())
        })
    }

    /// "Download…": Windows' Save dialog (starting in Downloads), then a copy of the file there.
    /// Both run on a worker thread so Pillbox keeps drawing; the result comes back as a job.
    fn download_copy(&mut self, path: PathBuf) {
        let tx = self.jobs_tx.clone();
        let waker = self.window.waker();
        let spawned = std::thread::Builder::new()
            .name("save-copy".into())
            .spawn(move || {
                let result = (|| -> std::result::Result<Option<PathBuf>, String> {
                    let name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default();
                    let start = crate::platform::net::downloads_dir().ok();
                    let Some(target) = system::save_dialog(&name, start.as_deref())
                        .map_err(|e| format!("{e:#}"))?
                    else {
                        return Ok(None);
                    };
                    let target = PathBuf::from(target);
                    if target != path {
                        std::fs::copy(&path, &target).map_err(|e| e.to_string())?;
                    }
                    Ok(Some(target))
                })();
                let _ = tx.send(super::Job::Saved(result));
                waker.wake();
            });
        if let Err(e) = spawned {
            tracing::warn!("could not start the save dialog: {e}");
        }
    }

    fn slider_set(&mut self, drag: SliderDrag, x: f32) {
        let v = ((x - drag.left) / (drag.right - drag.left).max(1.0)).clamp(0.0, 1.0);
        match drag.kind {
            SliderKind::Volume => {
                if let Err(e) = devices::set_speaker_volume(v) {
                    tracing::warn!("{e:#}");
                }
                self.t.volume = devices::speaker();
            }
            SliderKind::Brightness => {
                self.t.ctl.brightness = Some(v);
                if let Some(c) = &self.t.controls {
                    c.set_brightness(v);
                }
            }
        }
        self.dirty = true;
    }

    /// Open the Wi-Fi or Bluetooth list under the cards, or fold it when it is the open one.
    fn toggle_link_panel(&mut self, panel: LinkPanel) {
        self.t.link_panel = if self.t.link_panel == Some(panel) {
            None
        } else {
            Some(panel)
        };
        self.t.link_note = None;
        self.t.scroll.insert(Tab::Controls, 0.0);
        self.refresh_links();
        self.sync_links_timer();
        self.retarget();
        self.start_animating();
    }

    /// Ask for the open list again (Wi-Fi also asks the adapter to scan).
    fn refresh_links(&self) {
        let Some(l) = &self.t.links else { return };
        match self.t.link_panel {
            Some(LinkPanel::Wifi) => l.refresh_wifi(true),
            Some(LinkPanel::Bluetooth) => l.refresh_bluetooth(),
            None => {}
        }
    }

    /// The open list looks for changes every few seconds, only while it is on screen.
    pub(super) fn sync_links_timer(&mut self) {
        if self.open && self.tab == Tab::Controls && self.t.link_panel.is_some() {
            self.window.set_timer(TIMER_LINKS, LINKS_REFRESH_MS);
        } else {
            self.window.kill_timer(TIMER_LINKS);
        }
    }

    pub(super) fn on_links_tick(&mut self) {
        self.refresh_links();
    }

    /// Fold the list when the card closes, so it opens on the usual Controls tab next time.
    pub(super) fn close_link_panel(&mut self) {
        self.t.link_panel = None;
        self.t.link_note = None;
        self.t.link_busy = None;
        self.t.scroll.insert(Tab::Controls, 0.0);
        self.sync_links_timer();
    }

    /// What the Wi-Fi and Bluetooth readings changed to, for the next draw.
    fn poll_links(&mut self) {
        let Some(updates) = self.t.links.as_ref().map(Links::poll) else {
            return;
        };
        if updates.is_empty() {
            return;
        }
        for update in updates {
            match update {
                links::Update::Wifi(list) => {
                    self.t.wifi = list;
                    self.t.wifi_loaded = true;
                }
                links::Update::Bluetooth(list) => {
                    self.t.bluetooth = list;
                    self.t.bluetooth_loaded = true;
                }
                links::Update::Done(note) => {
                    self.t.link_busy = None;
                    self.t.link_note = note;
                }
            }
        }
        self.dirty = true;
    }

    pub(super) fn poll_controls(&mut self) {
        self.poll_links();
        if let Some(state) = self.t.controls.as_ref().and_then(Controls::poll) {
            // A radio was switched (here or in Windows): its list changes with it.
            if let Some(l) = &self.t.links {
                if state.wifi != self.t.ctl.wifi {
                    l.refresh_wifi(true);
                }
                if state.bluetooth != self.t.ctl.bluetooth {
                    l.refresh_bluetooth();
                }
            }
            // While dragging, keep showing the value under the pointer.
            let brightness = if self
                .t
                .slider
                .is_some_and(|s| s.kind == SliderKind::Brightness)
            {
                self.t.ctl.brightness
            } else {
                state.brightness
            };
            self.t.ctl = controls::State {
                brightness,
                ..state
            };
            self.dirty = true;
        }
    }

    // ---- clipboard -------------------------------------------------------------------------

    fn copy_text(&mut self, text: &str, done: &str) {
        match clipboard::write(self.window.hwnd(), &Content::Text(text.to_string())) {
            Ok(()) => self.show_toast(Toast::icon("\u{E8C8}", done), TOAST_MS),
            Err(e) => {
                tracing::warn!("copy: {e:#}");
                self.show_toast(Toast::text("Clipboard is busy"), TOAST_MS);
            }
        }
    }

    pub(super) fn on_clipboard_changed(&mut self) {
        if !self.settings.clipboard.enabled {
            return;
        }
        let ignore = self.settings.clipboard.ignore_apps.clone();
        match clipboard::read(self.window.hwnd(), &ignore) {
            Err(e) => {
                // Another app still has it open; try again shortly, a few times.
                if self.t.clip_retries < 5 {
                    self.t.clip_retries += 1;
                    self.window.set_timer(TIMER_CLIP_RETRY, 120);
                } else {
                    tracing::warn!("clipboard not read: {e:#}");
                    self.t.clip_retries = 0;
                }
            }
            Ok(Err(skip)) => {
                self.t.clip_retries = 0;
                match skip {
                    Skip::Excluded => {
                        tracing::debug!("clipboard copy marked private; not recorded")
                    }
                    Skip::IgnoredApp(exe) => tracing::debug!("clipboard copy from {exe} ignored"),
                    Skip::Nothing => {}
                }
            }
            Ok(Ok((content, source))) => {
                self.t.clip_retries = 0;
                self.record_clip(content, source);
            }
        }
    }

    fn record_clip(&mut self, content: Content, source: String) {
        let at = unix_now();
        let (item, picture) = match content {
            Content::Text(text) => {
                let text: String = text.chars().take(MAX_CLIP_TEXT).collect();
                let kind = cliphistory::classify_text(&text);
                let hash = cliphistory::fingerprint(kind, text.as_bytes());
                (
                    ClipItem {
                        id: 0,
                        kind,
                        text,
                        size: (0, 0),
                        pinned: false,
                        at,
                        hash,
                        source,
                    },
                    None,
                )
            }
            Content::Files(files) => {
                let text = files
                    .iter()
                    .map(|f| f.to_string_lossy().to_string())
                    .collect::<Vec<_>>()
                    .join("\n");
                let hash = cliphistory::fingerprint(ClipKind::Files, text.as_bytes());
                (
                    ClipItem {
                        id: 0,
                        kind: ClipKind::Files,
                        text,
                        size: (0, 0),
                        pinned: false,
                        at,
                        hash,
                        source,
                    },
                    None,
                )
            }
            Content::Picture(img) => {
                if !self.settings.clipboard.pictures {
                    return;
                }
                let hash = cliphistory::fingerprint(ClipKind::Picture, &img.pixels);
                let item = ClipItem {
                    id: 0,
                    kind: ClipKind::Picture,
                    text: String::new(),
                    size: (img.width, img.height),
                    pinned: false,
                    at,
                    hash,
                    source,
                };
                (item, Some(img))
            }
        };
        let recorded = self.t.clip.record(item);
        if let (Recorded::New(id), Some(img)) = (recorded, picture) {
            let thumb = img.thumbnail(THUMB_SIDE);
            self.upload_clip_thumb(id, thumb);
            self.save_picture_in_background(id, img);
        }
        let mut removed = self
            .t
            .clip
            .trim(self.settings.clipboard.max_items as usize, MAX_PICTURES);
        removed.extend(
            self.t
                .clip
                .expire(unix_now(), self.settings.clipboard.expire_days),
        );
        self.forget_pictures(&removed);
        self.save_clip_index();
        self.dirty = true;
    }

    pub(super) fn upload_clip_thumb(&mut self, id: u64, thumb: Bgra) {
        self.renderer.set_image(
            crate::platform::gfx::ImageSlot::Thumb(clip_thumb_key(id)),
            &thumb.pixels,
            thumb.width,
            thumb.height,
        );
        self.t.clip_thumbs.insert(id, thumb);
        self.dirty = true;
    }

    fn save_picture_in_background(&self, id: u64, img: Bgra) {
        let Some(path) = picture_path(id) else { return };
        let _ = std::thread::Builder::new()
            .name("clip-save".into())
            .spawn(move || {
                let result = img
                    .to_png()
                    .map_err(anyhow::Error::msg)
                    .and_then(|png| dpapi::protect(&png))
                    .and_then(|sealed| std::fs::write(&path, sealed).map_err(anyhow::Error::from));
                if let Err(e) = result {
                    tracing::warn!("clipboard picture not saved: {e:#}");
                }
            });
    }

    fn forget_pictures(&mut self, ids: &[u64]) {
        for id in ids {
            if self.t.clip_thumbs.remove(id).is_some() {
                self.renderer
                    .clear_image(crate::platform::gfx::ImageSlot::Thumb(clip_thumb_key(*id)));
            }
            if let Some(p) = picture_path(*id) {
                let _ = std::fs::remove_file(p);
            }
        }
    }

    fn save_clip_index(&self) {
        let result = serde_json::to_vec(&self.t.clip)
            .map_err(anyhow::Error::from)
            .and_then(|json| dpapi::protect(&json))
            .and_then(|sealed| store::save_bytes("clipboard/index.bin", &sealed));
        if let Err(e) = result {
            tracing::warn!("clipboard history not saved: {e:#}");
        }
    }

    fn clip_copy(&mut self, id: u64) {
        let Some(item) = self.t.clip.get(id).cloned() else {
            return;
        };
        let content = match item.kind {
            ClipKind::Text | ClipKind::Link => Content::Text(item.text),
            ClipKind::Files => Content::Files(item.text.lines().map(PathBuf::from).collect()),
            ClipKind::Picture => match load_picture(id) {
                Some(img) => Content::Picture(img),
                None => {
                    self.show_toast(Toast::text("Picture is no longer stored"), TOAST_MS);
                    return;
                }
            },
        };
        match clipboard::write(self.window.hwnd(), &content) {
            Ok(()) => self.show_toast(Toast::icon("\u{E8C8}", "Copied"), 1200),
            Err(e) => {
                tracing::warn!("clipboard: {e:#}");
                self.show_toast(Toast::text("Clipboard is busy"), TOAST_MS);
            }
        }
    }

    /// Open a clipboard item: files with their apps, a web link in the browser, text in the
    /// text editor and a picture in the picture viewer (both through a plain copy in
    /// `%TEMP%\Pillbox\opened`, see `opened_copy`).
    fn clip_open(&mut self, id: u64) {
        use cliphistory::OpenTarget;
        let Some(item) = self.t.clip.get(id).cloned() else {
            return;
        };
        let opened = match cliphistory::open_target(&item) {
            OpenTarget::Paths(paths) => {
                let mut ok = 0;
                for p in &paths {
                    if Path::new(p).exists() {
                        match system::open_path(p, "") {
                            Ok(()) => ok += 1,
                            Err(e) => tracing::warn!("clipboard open: {e:#}"),
                        }
                    }
                }
                if ok == 0 {
                    return self.show_toast(Toast::text("Those files are gone"), TOAST_MS);
                }
                Ok(())
            }
            OpenTarget::Link(url) => system::open_path(&url, ""),
            OpenTarget::TextFile => {
                opened_copy(&format!("clipboard-{id}.txt"), item.text.as_bytes())
                    .and_then(|p| system::open_path(&p.to_string_lossy(), ""))
            }
            OpenTarget::PictureFile => match picture_png(id) {
                Some(png) => opened_copy(&format!("clipboard-{id}.png"), &png)
                    .and_then(|p| system::open_path(&p.to_string_lossy(), "")),
                None => {
                    return self.show_toast(Toast::text("Picture is no longer stored"), TOAST_MS);
                }
            },
        };
        match opened {
            Ok(()) => self.show_toast(Toast::icon("\u{E8A7}", "Opened"), 1200),
            Err(e) => {
                tracing::warn!("clipboard open: {e:#}");
                self.show_toast(Toast::text("Could not open it"), TOAST_MS);
            }
        }
    }

    fn clip_ocr(&mut self, id: u64) {
        let tx = self.jobs_tx.clone();
        let waker = self.window.waker();
        self.show_toast(Toast::icon("\u{E8E9}", "Reading text…"), 4000);
        let _ = std::thread::Builder::new()
            .name("ocr".into())
            .spawn(move || {
                let result = load_picture(id)
                    .ok_or_else(|| "picture is no longer stored".to_string())
                    .and_then(|img| ocr::recognize(&img).map_err(|e| format!("{e:#}")));
                let _ = tx.send(Job::Ocr(result));
                waker.wake();
            });
    }

    pub(super) fn on_ocr(&mut self, result: Result<String, String>) {
        match result {
            Ok(text) if !text.trim().is_empty() => {
                self.copy_text(&text, "Text from picture copied")
            }
            Ok(_) => self.show_toast(Toast::text("No text found in the picture"), 2200),
            Err(e) => {
                tracing::warn!("OCR: {e}");
                self.show_toast(Toast::text("Could not read text"), 2200);
            }
        }
    }

    // ---- shelf -----------------------------------------------------------------------------

    pub(super) fn sync_shelf_selection(&mut self) {
        self.t.shelf_selected.resize(self.shelf.len(), false);
    }

    /// The files an action applies to: the selection, or everything when nothing is selected.
    /// With `pressed`, a press on an unselected file means just that file.
    fn shelf_targets(&self, pressed: Option<usize>) -> Vec<PathBuf> {
        if let Some(i) = pressed
            && !self.t.shelf_selected.get(i).copied().unwrap_or(false)
        {
            return self.shelf.get(i).cloned().into_iter().collect();
        }
        let selected: Vec<PathBuf> = self
            .shelf
            .iter()
            .zip(&self.t.shelf_selected)
            .filter(|(_, s)| **s)
            .map(|(p, _)| p.clone())
            .collect();
        if selected.is_empty() {
            self.shelf.clone()
        } else {
            selected
        }
    }

    fn shelf_remove(&mut self) {
        if self.t.shelf_selected.iter().any(|s| *s) {
            let keep: Vec<PathBuf> = self
                .shelf
                .iter()
                .zip(&self.t.shelf_selected)
                .filter(|(_, s)| !**s)
                .map(|(p, _)| p.clone())
                .collect();
            self.shelf = keep;
        } else {
            self.shelf.clear();
        }
        self.t.shelf_selected = vec![false; self.shelf.len()];
        self.save_shelf();
    }

    fn shelf_drag_out(&mut self, index: usize) {
        let files = self.shelf_targets(Some(index));
        if files.is_empty() {
            return;
        }
        tracing::info!("dragging {} file(s) out of the shelf", files.len());
        match dragdrop::drag_out(&files) {
            Ok(true) => tracing::info!("files dropped"),
            Ok(false) => tracing::info!("drag cancelled"),
            Err(e) => {
                tracing::warn!("drag out: {e:#}");
                self.show_toast(Toast::text("Those files are gone"), TOAST_MS);
            }
        }
        // OLE ran its own loop; the pointer may be anywhere now.
        if !self.pointer_over_pillbox() {
            self.pointer_left();
        }
        self.dirty = true;
    }

    pub(super) fn on_drag_enter(&mut self) {
        if self.mode != super::Mode::Normal {
            return;
        }
        self.tab = Tab::Shelf;
        self.hovering = true;
        self.window.kill_timer(super::TIMER_CLOSE);
        self.set_open(true);
        self.tab_shown();
        self.retarget();
    }

    pub(super) fn on_files_dropped(&mut self) {
        let files = dragdrop::take_dropped();
        let n = files.len();
        if n == 0 {
            return;
        }
        self.shelf_add(files);
        self.tab_shown();
        self.show_toast(
            Toast::icon(
                "\u{E7B8}",
                format!("{n} file{} on the shelf", if n == 1 { "" } else { "s" }),
            ),
            TOAST_MS,
        );
        if !self.pointer_over_pillbox() {
            self.pointer_left();
        }
    }

    pub(super) fn on_drag_leave(&mut self) {
        if !self.pointer_over_pillbox() {
            self.pointer_left();
        }
    }

    // ---- reminders -------------------------------------------------------------------------

    fn add_reminder(&mut self) {
        let text = self.t.edits.reminder_add.text.clone();
        if text.trim().is_empty() {
            return;
        }
        match reminders::parse_quick(&text, now_minutes()) {
            Ok(p) => {
                let when =
                    reminders::describe(p.due, now_minutes(), self.settings.behavior.clock_24h);
                self.t.reminders.add(p.text, p.due, p.repeat);
                self.t.edits.reminder_add.clear();
                self.t.reminder_error = None;
                self.save_reminders();
                self.show_toast(
                    Toast::icon("\u{EA8F}", format!("Reminder set · {when}")),
                    2000,
                );
                self.refresh_widgets(false);
            }
            Err(e) => self.t.reminder_error = Some(e),
        }
        self.dirty = true;
    }

    fn save_reminders(&self) {
        if let Err(e) = store::save_json("reminders.json", &self.t.reminders) {
            tracing::warn!("reminders not saved: {e:#}");
        }
    }

    /// Called every minute (and after changes): ring anything that is due.
    pub(super) fn check_reminders(&mut self) {
        let now = now_minutes();
        let due: Vec<u64> = self
            .t
            .reminders
            .due(now)
            .map(|r| r.id)
            .filter(|id| !self.t.ringing.contains(id))
            .collect();
        if due.is_empty() {
            return;
        }
        for id in &due {
            if let Some(r) = self.t.reminders.items.iter().find(|r| r.id == *id) {
                tracing::info!("reminder due: {}", r.text);
            }
        }
        let first_ring = self.t.ringing.is_empty();
        self.t.ringing.extend(due);
        if first_ring && self.settings.reminders.chime {
            info::stop_chime();
            info::chime(true);
        }
        // Show it even over an app that normally hides Pillbox.
        if self.mode != super::Mode::Normal {
            tracing::info!("Pillbox mode: Normal (reminder ringing)");
            self.mode = super::Mode::Normal;
            self.sync_visibility();
        }
        if !self.user_visible {
            self.set_user_visible(true);
        }
        self.tab = Tab::Reminders;
        self.t.scroll.insert(Tab::Reminders, 0.0);
        self.hovering = true;
        self.window.kill_timer(super::TIMER_CLOSE);
        self.set_open(true);
        self.retarget();
        self.dirty = true;
    }

    fn answer_reminder(&mut self, id: u64, snooze: bool) {
        let now = now_minutes();
        if snooze {
            self.t
                .reminders
                .snooze(id, now + i64::from(self.settings.reminders.snooze_minutes));
        } else {
            self.t.reminders.done(id, now);
        }
        let was_ringing = self.t.ringing.contains(&id);
        self.t.ringing.retain(|r| *r != id);
        if self.t.ringing.is_empty() {
            info::stop_chime();
            // The reminder may have come up over an app that hides Pillbox.
            if was_ringing {
                self.refresh_mode();
                if !self.pointer_over_pillbox() {
                    self.pointer_left();
                }
            }
        }
        self.save_reminders();
        self.refresh_widgets(false);
    }

    /// "Today 18:30 · call mum" for the Next event resting face.
    pub(super) fn next_event_text(&self) -> String {
        let now = now_minutes();
        match self.t.reminders.items.iter().find(|r| r.rings_at() >= now) {
            Some(r) => {
                let (_, h, m) = calendar::split(r.rings_at());
                let today = calendar::split(now).0 == calendar::split(r.rings_at()).0;
                let time = face::format_time(
                    face::LocalTime {
                        hour: h as u8,
                        minute: m as u8,
                        ..Default::default()
                    },
                    self.settings.behavior.clock_24h,
                );
                if today {
                    format!("{time} {}", r.text)
                } else {
                    r.text.clone()
                }
            }
            None => "No reminders".into(),
        }
    }

    // ---- widgets ---------------------------------------------------------------------------

    fn widget_action(&mut self, w: WidgetAction) {
        match w {
            WidgetAction::FocusTimer => {
                let ft = &mut self.t.focus_timer;
                match ft.running_since.take() {
                    Some(since) => {
                        ft.left = ft.left.saturating_sub(since.elapsed().as_secs() as u32);
                        self.window.kill_timer(TIMER_FOCUS);
                    }
                    None => {
                        if ft.left == 0 {
                            ft.left = self.settings.widgets.focus_minutes * 60;
                        }
                        ft.running_since = Some(Instant::now());
                        self.window.set_timer(TIMER_FOCUS, 1000);
                    }
                }
            }
            WidgetAction::FocusReset => {
                self.t.focus_timer = FocusTimer {
                    left: self.settings.widgets.focus_minutes * 60,
                    running_since: None,
                };
                self.window.kill_timer(TIMER_FOCUS);
            }
            WidgetAction::TodoToggle(id) => {
                if let Some(t) = self.t.todos.items.iter_mut().find(|t| t.id == id) {
                    t.done = !t.done;
                }
                // Open items first.
                self.t.todos.items.sort_by_key(|t| t.done);
                self.save_todos();
            }
            WidgetAction::TodoDelete(id) => {
                self.t.todos.items.retain(|t| t.id != id);
                self.save_todos();
            }
            WidgetAction::PopOut(i) => self.pop_out_widget(i),
            WidgetAction::PhotoNext => self.next_photo(),
            WidgetAction::PlayToggle => {
                self.t.play_open = !self.t.play_open;
                self.t.play_day = 0;
            }
            WidgetAction::PlayDay(i) => self.t.play_day = i,
        }
        self.refresh_widgets(false);
    }

    pub(super) fn on_focus_tick(&mut self) {
        let ft = &self.t.focus_timer;
        let left = ft.running_since.map_or(ft.left, |s| {
            ft.left.saturating_sub(s.elapsed().as_secs() as u32)
        });
        if left == 0 && ft.running_since.is_some() {
            self.t.focus_timer = FocusTimer {
                left: self.settings.widgets.focus_minutes * 60,
                running_since: None,
            };
            self.window.kill_timer(TIMER_FOCUS);
            info::chime(false);
            self.show_toast(Toast::icon("\u{E916}", "Focus session done"), 3000);
        }
        self.refresh_widgets(false);
    }

    fn save_todos(&self) {
        if let Err(e) = store::save_json("todos.json", &self.t.todos) {
            tracing::warn!("to-dos not saved: {e:#}");
        }
    }

    fn save_note(&self) {
        if let Err(e) = store::save_bytes("note.txt", self.t.edits.note.text.as_bytes()) {
            tracing::warn!("note not saved: {e:#}");
        }
    }

    /// Once a minute: count active time for the screen-time widget.
    pub(super) fn count_screen_time(&mut self) {
        let today = today_string();
        if self.t.screen_time.date != today {
            self.t.screen_time = ScreenTime {
                date: today,
                seconds: 0,
            };
        }
        if info::idle_seconds() < 60 {
            self.t.screen_time.seconds += 60;
            if let Err(e) = store::save_json("screentime.json", &self.t.screen_time) {
                tracing::debug!("screen time not saved: {e:#}");
            }
        }
    }

    fn photo_folder(&self) -> Option<PathBuf> {
        let custom = self.settings.widgets.photos_folder.trim();
        if !custom.is_empty() {
            return Some(PathBuf::from(custom));
        }
        screen::screenshots_dir()
            .ok()
            .and_then(|d| d.parent().map(Path::to_path_buf))
    }

    pub(super) fn next_photo(&mut self) {
        if !self.settings.widgets.shown.contains(&WidgetKind::Photos) {
            return;
        }
        if self.t.photos.is_empty() {
            let Some(dir) = self.photo_folder() else {
                return;
            };
            let is_image = |p: &Path| {
                p.extension()
                    .map(|e| e.to_string_lossy().to_lowercase())
                    .is_some_and(|e| {
                        matches!(
                            e.as_str(),
                            "jpg" | "jpeg" | "png" | "bmp" | "gif" | "webp" | "heic"
                        )
                    })
            };
            let mut found = Vec::new();
            let mut dirs = vec![(dir, 0)];
            while let Some((d, depth)) = dirs.pop() {
                let Ok(rd) = std::fs::read_dir(&d) else {
                    continue;
                };
                for e in rd.flatten() {
                    let p = e.path();
                    if p.is_dir() && depth < 1 {
                        dirs.push((p, depth + 1));
                    } else if is_image(&p) {
                        found.push(p);
                    }
                    if found.len() >= 500 {
                        break;
                    }
                }
            }
            self.t.photos = found;
            // Start somewhere different each time the app starts.
            self.t.photo_index = (unix_now() as usize) % self.t.photos.len().max(1);
        }
        if self.t.photos.is_empty() {
            return;
        }
        self.t.photo_index = (self.t.photo_index + 1) % self.t.photos.len();
        let path = self.t.photos[self.t.photo_index].clone();
        self.t.thumbs.request(thumbs::Job::Picture {
            key: PHOTO_KEY,
            path,
            max: (480.0 * self.monitor.scale) as u32,
        });
    }

    /// Fetch what the network widgets show, in the background. Only when they are switched
    /// on and on the Home tab.
    pub(super) fn fetch_online(&mut self, force: bool) {
        let w = &self.settings.widgets;
        let wanted: Vec<WidgetKind> = w
            .shown
            .iter()
            .copied()
            .filter(|k| k.online() && *k != WidgetKind::Weather)
            .collect();
        if !w.online || wanted.is_empty() {
            return;
        }
        if !force && self.t.online_at.is_some_and(|t| t.elapsed().as_secs() < 60) {
            return;
        }
        self.t.online_at = Some(Instant::now());
        let host = w.ping_host.clone();
        let sites = w.websites.clone();
        let base = w.currency_base.clone();
        let codes = w.currencies.clone();
        let (lat, lon) = (
            self.settings.weather.latitude,
            self.settings.weather.longitude,
        );
        let tx = self.jobs_tx.clone();
        let waker = self.window.waker();
        let _ = std::thread::Builder::new().name("online".into()).spawn(move || {
            let agent = ureq::AgentBuilder::new()
                .timeout(std::time::Duration::from_secs(8))
                .build();
            let mut out = Online::default();
            if wanted.contains(&WidgetKind::Ping) {
                out.ping = Some(info::ping(&host).map_err(|e| format!("{e}")));
            }
            if wanted.contains(&WidgetKind::Currency) && !codes.is_empty() {
                let url = format!("https://api.frankfurter.app/latest?from={base}&to={}", codes.join(","));
                if let Some(v) = agent.get(&url).call().ok().and_then(|r| r.into_string().ok()).and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok()) {
                    for c in &codes {
                        if let Some(rate) = v["rates"][c].as_f64() {
                            out.currency.push((c.clone(), rate));
                        }
                    }
                }
            }
            if wanted.contains(&WidgetKind::AirQuality) && (lat != 0.0 || lon != 0.0) {
                let url = format!(
                    "https://air-quality-api.open-meteo.com/v1/air-quality?latitude={lat:.3}&longitude={lon:.3}&current=european_aqi,pm2_5"
                );
                if let Some(v) = agent.get(&url).call().ok().and_then(|r| r.into_string().ok()).and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok()) {
                    let aqi = v["current"]["european_aqi"].as_f64();
                    let pm = v["current"]["pm2_5"].as_f64();
                    out.air = aqi.map(|a| {
                        let word = match a as u32 {
                            0..=20 => "Good",
                            21..=40 => "Fair",
                            41..=60 => "Moderate",
                            61..=80 => "Poor",
                            _ => "Very poor",
                        };
                        match pm {
                            Some(p) => format!("{a:.0} {word} · PM2.5 {p:.0}"),
                            None => format!("{a:.0} {word}"),
                        }
                    });
                }
            }
            if wanted.contains(&WidgetKind::WebsiteCheck) {
                for site in sites.iter().take(3) {
                    let url = if site.contains("://") { site.clone() } else { format!("https://{site}") };
                    let up = match agent.get(&url).call() {
                        Ok(_) => Some(true),
                        Err(ureq::Error::Status(code, _)) => Some(code < 500),
                        Err(_) => Some(false),
                    };
                    out.websites.push((site.clone(), up));
                }
            }
            let _ = tx.send(Job::Online(out));
            waker.wake();
        });
    }

    pub(super) fn on_online(&mut self, online: Online) {
        self.t.online = online;
        self.refresh_widgets(false);
    }

    /// Recompute what the widgets show. `stats` also samples CPU and network.
    pub(super) fn refresh_widgets(&mut self, stats: bool) {
        let s = &self.settings;
        let w = &s.widgets;
        let now = system::local_time();
        let now_min = calendar::now_minutes(now);
        let date = calendar::date_of(now);
        let mut d = std::mem::take(&mut self.t.wdata);
        d.time = face::format_time(now, s.behavior.clock_24h);
        d.date = face::format_date(now);
        d.day_hours = f32::from(now.hour) + f32::from(now.minute) / 60.0;
        d.clock_color =
            theme::parse_hex(&w.clock_color).filter(|c| *c != theme::Rgba::rgb(0xFFFFFF));
        d.world = w
            .world_clocks
            .iter()
            .map(|zone| match info::zone_minutes(zone) {
                Ok(m) => {
                    let (_, h, mi) = calendar::split(m);
                    let diff = m - now_min;
                    let offset = match diff {
                        0 => "same time".to_string(),
                        x if x % 60 == 0 => format!("{:+} h", x / 60),
                        x => format!("{:+} h {:02}", x / 60, (x % 60).abs()),
                    };
                    let name = zone.rsplit('/').next().unwrap_or(zone).replace('_', " ");
                    WorldTime {
                        name,
                        time: face::format_time(
                            face::LocalTime {
                                hour: h as u8,
                                minute: mi as u8,
                                ..Default::default()
                            },
                            s.behavior.clock_24h,
                        ),
                        offset,
                    }
                }
                Err(_) => WorldTime {
                    name: zone.clone(),
                    time: "?".into(),
                    offset: "unknown time zone".into(),
                },
            })
            .collect();
        d.weather = self.faces.weather_text(s);
        d.weather_note = if !s.weather.enabled {
            Some("Turn on weather in Settings")
        } else if s.weather.latitude == 0.0 && s.weather.longitude == 0.0 {
            Some("Set a location in Settings")
        } else {
            None
        };
        d.weather_detail = None;
        d.weather_lines = if s.widgets.weather_look == pillbox_core::settings::WeatherLook::Detailed
        {
            self.faces.weather_details(s)
        } else {
            Vec::new()
        };
        let located = s.weather.latitude != 0.0 || s.weather.longitude != 0.0;
        let fmt = |m: f64| {
            let m = m.round() as u32;
            face::format_time(
                face::LocalTime {
                    hour: (m / 60 % 24) as u8,
                    minute: (m % 60) as u8,
                    ..Default::default()
                },
                s.behavior.clock_24h,
            )
        };
        d.sun = located
            .then(|| {
                astro::sun_times(
                    date,
                    f64::from(s.weather.latitude),
                    f64::from(s.weather.longitude),
                    info::utc_offset_minutes() as f64,
                )
            })
            .flatten()
            .map(|(r, st)| (fmt(r), fmt(st)));
        let phase = astro::moon_phase(info::utc_minutes());
        d.moon = (phase as f32, astro::moon_name(phase));
        d.next = self
            .t
            .reminders
            .items
            .iter()
            .filter(|r| r.rings_at() >= now_min)
            .take(3)
            .map(|r| {
                (
                    r.text.clone(),
                    reminders::describe(r.rings_at(), now_min, s.behavior.clock_24h),
                )
            })
            .collect();
        d.todos = self
            .t
            .todos
            .items
            .iter()
            .map(|t| widgets::Todo {
                id: t.id,
                text: t.text.clone(),
                done: t.done,
            })
            .collect();
        d.calc_result = calc::eval(&self.t.edits.calc.text).ok().map(calc::format);
        let ft = &self.t.focus_timer;
        let total = w.focus_minutes * 60;
        let left = ft.running_since.map_or(ft.left, |since| {
            ft.left.saturating_sub(since.elapsed().as_secs() as u32)
        });
        d.focus = (
            if left == 0 && ft.running_since.is_none() {
                total
            } else {
                left
            },
            total,
            ft.running_since.is_some(),
        );
        d.countdown = calendar::parse_date(&w.countdown_date).map(|target| {
            let days = calendar::days_from_civil(target) - calendar::days_from_civil(date);
            let name = if w.countdown_name.is_empty() {
                w.countdown_date.clone()
            } else {
                w.countdown_name.clone()
            };
            (name, days)
        });
        d.day_progress = face::day_progress(now);
        let year_days = if calendar::days_in_month(date.year, 2) == 29 {
            366.0
        } else {
            365.0
        };
        d.year_progress = ((date.day_of_year() as f32 - 1.0) + d.day_progress) / year_days;
        if stats {
            if let Some(c) = self.t.cpu.sample() {
                d.cpu = Some(c);
            }
            d.ram = info::memory();
            if let Some((down, up)) = info::network_bytes() {
                let now_i = Instant::now();
                if let Some((then, d0, u0)) = self.t.net_last {
                    let secs = now_i.duration_since(then).as_secs_f64().max(0.1);
                    let rate = |b: u64| {
                        let per_s = b as f64 / secs;
                        if w.network_mbps {
                            format!("{:.1} Mbps", per_s * 8.0 / 1_000_000.0)
                        } else {
                            format!("{:.1} MB/s", per_s / 1_000_000.0)
                        }
                    };
                    d.net = Some((rate(down.saturating_sub(d0)), rate(up.saturating_sub(u0))));
                }
                self.t.net_last = Some((now_i, down, up));
            }
        }
        if d.cpu.is_none() {
            d.cpu = self.faces.cpu_percent();
        }
        d.battery = system::battery().map(|b| (b.percent, b.plugged_in));
        d.screen_time = self.t.screen_time.seconds;
        // Play time, including the part of the current session not yet written to the log.
        let today = calendar::date_of(system::local_time());
        let live = self
            .play
            .current
            .as_ref()
            .map_or(0, |(_, since)| since.elapsed().as_secs());
        let week = self.play.log.week(today);
        let week_total: u64 = week.iter().map(|(_, s)| s).sum::<u64>() + live;
        d.play_today = playtime::format_duration(self.play.log.day_total(today) + live);
        d.play_week = playtime::format_duration(week_total);
        d.play_top = week.first().map(|(g, _)| g.clone());
        d.play_now = self.play.current.as_ref().map(|(g, _)| g.clone());
        let live_game = self
            .play
            .current
            .as_ref()
            .map(|(g, since)| (g.as_str(), since.elapsed().as_secs()));
        d.play_days = self
            .play
            .log
            .recent_days(today, PLAY_DAYS, live_game)
            .iter()
            .map(|day| {
                let longest = day.games.first().map_or(1, |(_, s)| (*s).max(1));
                PlayDayView {
                    chip: day.chip(today),
                    title: day.title(today),
                    total: if day.total == 0 {
                        String::new()
                    } else {
                        playtime::format_duration(day.total)
                    },
                    games: day
                        .games
                        .iter()
                        .map(|(g, s)| {
                            let share = *s as f32 / longest as f32;
                            (g.clone(), playtime::format_duration(*s), share)
                        })
                        .collect(),
                }
            })
            .collect();
        d.ping = self.t.online.ping.clone();
        d.websites = if self.t.online.websites.is_empty() {
            w.websites
                .iter()
                .take(3)
                .map(|u| (u.clone(), None))
                .collect()
        } else {
            self.t.online.websites.clone()
        };
        d.currency = self.t.online.currency.clone();
        d.currency_base = w.currency_base.clone();
        d.air = self.t.online.air.clone();
        d.offline = !w.online;
        self.t.wdata = d;
        if stats && self.t.gpu.is_some() {
            let mut d = std::mem::take(&mut self.t.wdata);
            self.sample_system_card(&mut d);
            self.t.wdata = d;
        }
        self.t.widgets_dirty = true;
        self.dirty = true;
        // The System tab sizes itself to its cards, and the count can change with the samples.
        if self.open && self.tab == Tab::System {
            self.retarget();
        }
    }

    /// The readings only the System card needs: the GPU, the busiest disk and the connection in
    /// use. CPU, RAM and battery are filled in by `refresh_widgets`.
    fn sample_system_card(&mut self, d: &mut WidgetData) {
        let mbps = self.settings.widgets.network_mbps;
        let g = self
            .t
            .gpu
            .as_ref()
            .map(GpuMonitor::stats)
            .unwrap_or_default();
        // Whether there is a GPU card comes from the adapter list asked for when the readers
        // started, not from the monitor's samples: its first one arrives up to half a second
        // later, and the card must not pop in after the tab has opened.
        let card = self.t.gpu_card.clone().flatten();
        d.sys.has_gpu = card.is_some();
        d.sys.gpu_integrated = card.as_ref().is_some_and(|(_, integrated)| *integrated);
        d.sys.gpu_name = card.map(|(name, _)| name);
        // "…" until the first sample (or if the monitor found no card of its own).
        d.sys.gpu = g.card_load.filter(|_| g.has_card);
        // The busiest disk's active time and speed (empty until the second reading).
        let disks = self
            .t
            .disk
            .as_mut()
            .map(DiskMonitor::sample)
            .unwrap_or_default();
        d.sys.disk = pillbox_core::sysinfo::busiest_disk(&disks).map(|disk| {
            (
                disk.name.clone(),
                disk.active,
                pillbox_core::games::format_rate(disk.read_bps, false),
                pillbox_core::games::format_rate(disk.write_bps, false),
            )
        });
        let unit = if mbps { "Mbps" } else { "MB/s" };
        let now = self.t.sys_clock.elapsed().as_secs_f64();
        let reading = self
            .t
            .sys_net
            .update(now, info::up_connections(), info::routed_adapter());
        d.sys.net = reading.map(|r| {
            let show = |bytes_per_s: f64| {
                if mbps {
                    bytes_per_s * 8.0 / 1_000_000.0
                } else {
                    bytes_per_s / 1_000_000.0
                }
            };
            let rates = r
                .rates
                .map(|(down, up)| format!("↓ {:.1}  ↑ {:.1}", show(down), show(up)));
            (r.name, rates, unit)
        });
    }

    // ---- widgets on the desktop ---------------------------------------------------------------

    fn widget_size(&self, kind: WidgetKind) -> (u32, u32) {
        // Desktop widgets follow the screen's DPI, not Pillbox's size setting.
        let k = self.monitor.dpi;
        let w = if widgets::is_wide(kind, &self.t.wdata) {
            352.0
        } else {
            176.0
        };
        ((w * k).round() as u32, (88.0 * k).round() as u32)
    }

    fn open_widget_window(&mut self, kind: WidgetKind, x: i32, y: i32) {
        let (w, h) = self.widget_size(kind);
        match WidgetWindow::create(x, y, w, h) {
            Ok(win) => {
                win.show();
                self.t.popped.push((kind, win));
                self.t.widgets_dirty = true;
                self.dirty = true;
            }
            Err(e) => tracing::warn!("widget window: {e:#}"),
        }
    }

    fn save_popped(&self) {
        let list: Vec<PoppedSaved> = self
            .t
            .popped
            .iter()
            .map(|(kind, win)| {
                let (x, y) = win.position();
                PoppedSaved { kind: *kind, x, y }
            })
            .collect();
        if let Err(e) = store::save_json("widgets.json", &list) {
            tracing::warn!("widget positions not saved: {e:#}");
        }
    }

    fn pop_out_widget(&mut self, index: usize) {
        let Some(kind) = self.settings.widgets.shown.get(index).copied() else {
            return;
        };
        let (w, _) = self.widget_size(kind);
        // Just below Pillbox, centred on the pointer.
        let (cx, _) = screen::cursor_pos().unwrap_or((self.win_pos.0, 0));
        let y = self.win_pos.1 + self.win_size.1 as i32 + (8.0 * self.monitor.scale) as i32;
        self.open_widget_window(kind, cx - w as i32 / 2, y);
        self.save_popped();
        self.sync_stats_timer();
        self.show_toast(
            Toast::icon("\u{E8A7}", "Drag it anywhere · right-click closes"),
            2500,
        );
    }

    pub(super) fn on_widget_moved(&mut self, id: isize) {
        if self.is_system_window(id) {
            self.on_system_moved();
            return;
        }
        self.save_popped();
    }

    /// A button in a desktop widget was clicked.
    pub(super) fn on_widget_click(&mut self, id: isize, x: f32, y: f32) {
        if self.is_system_window(id) {
            self.on_system_click(x, y);
            return;
        }
        let action = self
            .t
            .popped_hits
            .get(&id)
            .and_then(|hits| hits.iter().rev().find(|h| h.contains(x, y)))
            .map(|h| h.action);
        match action {
            Some(Action::Focus(field)) => {
                tracing::debug!("desktop widget click: {field:?} text box");
                self.focus_field_in(field, Some(id));
            }
            Some(action) => {
                tracing::debug!("desktop widget click: {action:?}");
                self.act_tool(action, x, y);
            }
            // A click on the widget body while typing in it ends typing.
            None if self.t.focus_widget == Some(id) => self.unfocus(true),
            None => {}
        }
    }

    pub(super) fn on_widget_resized(&mut self, id: isize) {
        if self.is_system_window(id) {
            self.on_system_resized();
        }
    }

    pub(super) fn on_widget_close(&mut self, id: isize) {
        if self.is_system_window(id) {
            self.close_system_window();
            return;
        }
        if self.t.focus_widget == Some(id) {
            self.unfocus(true);
        }
        self.t.popped_hits.remove(&id);
        self.t.popped.retain(|(_, w)| w.id() != id);
        self.save_popped();
        self.sync_stats_timer();
    }

    /// Redraw the desktop widgets when their data changed.
    pub(super) fn draw_widget_windows(&mut self) {
        if !self.t.widgets_dirty || (self.t.popped.is_empty() && self.t.sys_win.is_none()) {
            return;
        }
        self.t.widgets_dirty = false;
        // The same scale the desktop windows were sized with (`widget_size`, `sys_scale`).
        let k = self.monitor.dpi;
        let theme = &self.theme;
        let ink = crate::scene::Ink {
            text: theme.text,
            dim: theme.text_dim,
            accent: theme.accent,
            chip: theme.chip,
            wash: theme.wash,
        };
        let empty_clip = ClipHistory::default();
        let empty_rem = Reminders::default();
        let empty_board = pillbox_core::script::Board::default();
        let empty_notes = pillbox_core::notes::NoteStore::default();
        let focus_widget = self.t.focus_widget;
        let focus = self.t.focus;
        let mut ui = crate::tabs::UiState {
            pointer: None,
            pressed: None,
            focus: None,
            edits: &self.t.edits,
            scroll: 0.0,
            now_secs: unix_now(),
            now_min: now_minutes(),
            clock_24h: self.settings.behavior.clock_24h,
            clip: crate::tabs::ClipUi {
                history: &empty_clip,
                filter: ClipFilter::All,
                snippets: &[],
                two_columns: false,
                enabled: false,
            },
            shelf: crate::tabs::ShelfUi {
                files: &[],
                selected: &[],
                phone_enabled: false,
                phone: None,
                menu: None,
            },
            reminders: crate::tabs::RemindersUi {
                list: &empty_rem,
                ringing: &[],
                error: None,
                snooze_minutes: 10,
            },
            controls: crate::tabs::ControlsUi {
                volume: None,
                brightness: None,
                wifi: None,
                bluetooth: None,
                mic_muted: None,
                call: None,
                launcher: &[],
                chat_drawer: (false, false),
                mirror: None,
                links: crate::tabs::LinksUi::EMPTY,
            },
            widgets: crate::tabs::WidgetsUi {
                shown: &[],
                data: &self.t.wdata,
                play_open: false,
                play_day: 0,
            },
            notes: crate::tabs::NotesUi {
                store: &empty_notes,
                now_ms: 0,
                message: None,
            },
            activity: crate::tabs::ActivityUi {
                board: &empty_board,
                hidden: &[],
                now_ms: 0,
                expanded: None,
                confirm_stop: None,
                claude_running: false,
                usage_note: "",
            },
        };
        for (kind, win) in &mut self.t.popped {
            // Only the widget being typed into shows its text box as focused.
            ui.focus = focus.filter(|_| focus_widget == Some(win.id()));
            let cx = crate::tabs::Ctx {
                k,
                alpha: 1.0,
                ink,
                ui: &ui,
            };
            let (w, h) = (win.size.0 as f32, win.size.1 as f32);
            let mut f = win.renderer.begin();
            let body = crate::tabs::rounded((0.0, 0.0, w, h), 16.0 * k);
            // Widget windows wear the theme still: they are not worth a frame of motion.
            crate::scene::paint_body(&mut f, &body, theme, 0.0, k, None);
            crate::scene::paint_rim(&mut f, &body, theme, k);
            let mut hits = Vec::new();
            widgets::paint_tile(&mut f, &cx, *kind, 0, (0.0, 0.0, w, h), &mut hits);
            // Buttons work on the desktop too; text boxes still need Pillbox (they need
            // the keyboard), and popping out again makes no sense.
            hits.retain(|h| {
                matches!(
                    h.action,
                    Action::Focus(Field::Note | Field::Todo | Field::Calc)
                        | Action::Widget(
                            WidgetAction::FocusTimer
                                | WidgetAction::FocusReset
                                | WidgetAction::TodoToggle(_)
                                | WidgetAction::TodoDelete(_)
                                | WidgetAction::PhotoNext
                        )
                )
            });
            if let Err(e) = f.finish() {
                tracing::warn!("widget window draw failed: {e:#}");
            }
            win.set_buttons(hits.iter().map(|h| h.rect).collect());
            self.t.popped_hits.insert(win.id(), hits);
        }
        // The System window: one card per PC component; only its close button is a button.
        if let Some(win) = &mut self.t.sys_win {
            ui.focus = None;
            let cx = crate::tabs::Ctx {
                k,
                alpha: 1.0,
                ink,
                ui: &ui,
            };
            let (w, h) = (win.size.0 as f32, win.size.1 as f32);
            let mut f = win.renderer.begin();
            let body = crate::tabs::rounded((0.0, 0.0, w, h), 16.0 * k);
            crate::scene::paint_body(&mut f, &body, theme, 0.0, k, None);
            crate::scene::paint_rim(&mut f, &body, theme, k);
            widgets::paint_system_window(&mut f, &cx, (0.0, 0.0, w, h));
            if let Err(e) = f.finish() {
                tracing::warn!("System window draw failed: {e:#}");
            }
            win.set_buttons(vec![widgets::system_close_rect(w, k)]);
        }
    }
}
