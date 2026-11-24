//! Application state and main loop. No `unsafe` here.

use std::{
    path::PathBuf,
    sync::mpsc::{self, Receiver, Sender},
    time::{Duration, Instant},
};

use anyhow::Result;
use pillbox_core::{
    bend, face,
    hotkey::Hotkey,
    image::{self, Bgra},
    music::{self, BANDS, GLOW_STOPS, Palette},
    notices::{self, Power, PowerNotice, PowerWatch},
    settings::{
        ClockColor, MusicReveal, ReopenTab, RestingFace, Settings, Side,
        Visualizer as VisualizerStyle,
    },
    shape::{
        Dims, MUSIC_TEXT, NOTICE_LEAD, NOTICE_PAD, NOTICE_TEXT, Pill, Placement, REST_TEXT,
        RegionKey, Resting,
    },
    spring::{FixedStep, STEP, Spring, SpringParams},
    theme::{self, Rgba, Theme},
};

use crate::{
    faces::{self, Faces},
    lyrics::{LyricsService, LyricsState},
    platform::{
        audio::Visualizer,
        clipboard, devices, dragdrop,
        foreground::ForegroundHook,
        fullscreen::FullscreenApp,
        gfx::{ImageSlot, Renderer},
        headphones::HeadphoneWatch,
        internet::InternetWatch,
        media::{ArtUpdate, MediaCommand, MediaService, MediaUpdate, Track},
        screen,
        sensors::{InUse, SensorWatch},
        system::{self, DirWatcher, FrameTimer, Monitor},
        tray::{self, Tray},
        window::{self, Event, PillboxWindow, Pumped, TrayCommand},
    },
    scene::{self, Action, CallView, Hit, MusicView, Scene, Tab, TextLook, Toast},
    store,
    tabs::{ClipUi, ControlsUi, RemindersUi, ShelfUi, UiState, WidgetsUi},
};

// Global shortcut ids (RegisterHotKey). Launcher slots use LAUNCH_HOTKEY + slot.
const HK_TOGGLE: i32 = 1;
const HK_PLAY: i32 = 2;
const HK_MUTE: i32 = 3;
const HK_HIDE: i32 = 4;
const HK_SHOT: i32 = 5;
const HK_COLOR: i32 = 6;
// 7 and 8 were the web drawer and the chat drawer (both removed).
pub(crate) const LAUNCH_HOTKEY: i32 = 100;

mod games;
mod mirror;
mod notes;
mod phone;
mod script;
mod sysview;
mod tools;

use games::{Strip, TIMER_FULLSCREEN, TIMER_STRIP};
use tools::{
    TIMER_CLIP_RETRY, TIMER_FOCUS, TIMER_LINKS, TIMER_ONLINE, TIMER_PHOTO, TIMER_STATS, Tools,
};

/// Results of work done on worker threads, collected on the UI thread.
enum Job {
    ScreenshotSaved(std::result::Result<PathBuf, String>),
    /// A stored clipboard picture, decrypted and shrunk.
    ClipThumb(u64, Bgra),
    Ocr(std::result::Result<String, String>),
    Online(tools::Online),
    /// The Claude account's usage limits, or why there are none.
    AccountUsage(std::result::Result<pillbox_core::script::Usage, &'static str>),
    /// "Download…" from the shelf menu: where the copy went (None = dialog cancelled).
    Saved(std::result::Result<Option<PathBuf>, String>),
}

/// Springs: shape (4), song colour amount (1), three song colours (3 × RGB), the tab
/// highlight's place (1) and the tab content's ease-in (1).
const SPRINGS: usize = 16;
/// Index of the first song colour channel.
const TINT_SPRINGS: usize = 5;
/// The active tab's highlight, as a (fractional) index into the tab list: it slides.
const CHIP_SPRING: usize = 14;
/// How far a newly shown tab's content has eased in, 0..1.
const TAB_IN_SPRING: usize = 15;
/// Closing runs this much faster than opening (an exit is quicker than an entrance).
const CLOSE_SPEED: f32 = 1.45;

const TIMER_OPEN: usize = 1;
const TIMER_CLOSE: usize = 2;
const TIMER_MINUTE: usize = 3;
const TIMER_CPU: usize = 4;
const TIMER_PET: usize = 5;
const TIMER_PET_EYES: usize = 6;
const TIMER_RELOAD: usize = 7;
const TIMER_BACKDROP: usize = 8;
const TIMER_WEATHER: usize = 9;
const TIMER_TOAST: usize = 10;
const TIMER_SHOT: usize = 11;
const TIMER_CALL: usize = 12;
/// One-shot: put Pillbox back on top shortly after the foreground changed.
const TIMER_RAISE: usize = 13;
/// Ticks while the clock shows seconds, to redraw when the second changes.
const TIMER_SECOND: usize = 14;
/// One-shot: a changed song's time in view is over (`layout.reveal.music` = on song change).
const TIMER_REVEAL: usize = 15;
/// Ends the start-up greeting (`App::begin_intro`).
const TIMER_INTRO: usize = 16;
/// How long Pillbox stays in view after it starts, even when it is set to hide.
const INTRO_MS: u32 = 2600;

/// Extra pixels around the shape in the window region, so its hard edge never clips the
/// anti-aliased outline.
const REGION_PAD: i32 = 1;
/// Glass surfaces re-sample the screen at most this often.
const BACKDROP_INTERVAL: Duration = Duration::from_millis(33);
/// Decorations, the visualizer, the progress bar and lyrics animate at this rate.
const AMBIENT_FPS: f64 = 30.0;
/// Opening, closing and other shape changes animate at this rate (was `behavior.fps`).
const MOTION_FPS: f64 = 60.0;
/// How long a lock-key notice stays up.
const TOAST_MS: u32 = 1600;

/// Device notice icons (Segoe Fluent Icons, the same in MDL2 Assets).
const G_CHARGING: &str = "\u{E945}";
const G_BATTERY: &str = "\u{E83F}";
const G_DRIVE: &str = "\u{E88E}";
const G_ONLINE: &str = "\u{E774}";
const G_OFFLINE: &str = "\u{EB55}";
const G_BATTERY_LOW: &str = "\u{E851}";
const G_HEADPHONES: &str = "\u{E7F6}";

/// The lock keys we announce: virtual-key code and name.
const LOCK_KEYS: [(u16, &str); 3] = [
    (0x14, "Caps Lock"),
    (0x90, "Num Lock"),
    (0x91, "Scroll Lock"),
];

/// What the per-app rule for the foreground app asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Normal,
    Strip,
    Hidden,
}

/// Album art kept in memory so it can be re-uploaded if the GPU device is rebuilt.
struct ArtPixels {
    width: u32,
    height: u32,
    bgra: Vec<u8>,
    /// Blurred, darkened copy for card backgrounds.
    soft: Vec<u8>,
}

/// A drag on the progress bar.
#[derive(Clone, Copy)]
struct Scrub {
    left: f32,
    right: f32,
    fraction: f32,
}

pub struct App {
    settings: Settings,
    placement: Placement,
    theme: Theme,
    scale_override: Option<f32>,
    monitor: Monitor,
    win_pos: (i32, i32),
    win_size: (u32, u32),
    window: PillboxWindow,
    renderer: Renderer,
    _tray: Tray,
    frame_timer: FrameTimer,
    watcher: Option<DirWatcher>,
    _foreground: Option<ForegroundHook>,
    /// width, height, radius (DIPs) and hide (0 = in place, 1 = slid off screen)
    /// Width, height, corner radius, hide (0..1), then the resting pill's song colours: how
    /// much of them shows (0..1) and red, green and blue of each of the three, so they fade in
    /// and blend from song to song.
    springs: [Spring; SPRINGS],
    params: SpringParams,
    stepper: FixedStep,
    hovering: bool,
    open: bool,
    mode: Mode,
    /// Hide/Show from the tray menu.
    user_visible: bool,
    animating: bool,
    dirty: bool,
    backdrop_failed: bool,
    last_backdrop: Instant,
    faces: Faces,
    bend_buf: Vec<u8>,
    // Music.
    media: MediaService,
    /// `PILLBOX_FAKE_TRACK` is set: the visualizer gets pretend levels.
    fake_levels: bool,
    track: Option<Track>,
    art: Option<ArtPixels>,
    palette: Palette,
    visualizer: Option<Visualizer>,
    lyrics: LyricsService,
    /// The lyrics scrolled by hand: the line (fractional) in view. None = following the song.
    lyrics_view: Option<f32>,
    /// The Music tab's background glow, smoothed from the audio bands, and the flash a new
    /// lyric line gives it (1 -> 0).
    glow: [f32; GLOW_STOPS],
    pulse: f32,
    /// The lyric line shown last frame, to see when a new one starts.
    last_line: Option<usize>,
    last_glow_at: Instant,
    /// A song that just started or changed stays in view until then (hidden Pillbox, `reveal`).
    reveal_song_until: Option<Instant>,
    /// Pillbox just started: it slides in and stays in view until then, hidden or not.
    intro_until: Option<Instant>,
    /// The second last drawn on the clock (with seconds on), to redraw only when it changes.
    clock_second: u8,
    tab: Tab,
    /// A hidden tab that shows anyway (`summon_tab`): on the open card, and on the next one.
    summoned: Option<Tab>,
    summon_next: Option<Tab>,
    hits: Vec<Hit>,
    scrub: Option<Scrub>,
    // Notices on the resting pill.
    toast: Option<Toast>,
    lock_on: [bool; 3],
    lock_down: [bool; 3],
    // Global shortcuts currently registered with Windows.
    hotkeys: Vec<i32>,
    jobs_tx: Sender<Job>,
    jobs_rx: Receiver<Job>,
    /// The capture-exclusion setting to restore after a screenshot.
    shot_restore: Option<bool>,
    /// Files held on the shelf, newest first.
    shelf: Vec<PathBuf>,
    // Device notices: the battery and headphones.
    power: PowerWatch,
    headphones: Option<HeadphoneWatch>,
    internet: Option<InternetWatch>,
    // Microphone and camera.
    sensors: Option<SensorWatch>,
    in_use: InUse,
    call_since: Option<Instant>,
    mic_muted: bool,
    /// Clipboard, shelf selection, reminders, controls, text boxes, widgets.
    t: Tools,
    drop_target: Option<dragdrop::DropRegistration>,
    // Games: the fullscreen app in front (if any) and the stats strip.
    fullscreen: Option<FullscreenApp>,
    strip: Strip,
    mirror: mirror::Mirror,
    play: games::Play,
    /// M6: the QR code on screen and its server.
    phone: phone::Phone,
    /// M7: the script API, flashes and the Activity tab's board.
    script: script::Script,
    notes: notes::Notes,
    /// The card's quit button was clicked once and waits for a second click.
    quit_armed: bool,
    /// The tab the card last drew, to notice a switch (its content then eases in).
    shown_tab: Tab,
    /// Set by the second click: the main loop ends after this event.
    quit_now: bool,
    // Timing.
    started: Instant,
    last_frame: Instant,
    next_frame: Instant,
    anim_started: Instant,
    anim_frames: u32,
}

fn make_theme(s: &Settings) -> Theme {
    let theme = Theme::from_appearance(&s.appearance);
    // Windows asked for less motion: the moving themes stand still.
    if theme.motion.is_some() && !system::animations_enabled() {
        theme.still()
    } else {
        theme
    }
}

fn read_monitor(s: &Settings, scale_override: Option<f32>) -> Result<Monitor> {
    let mut monitor = system::pick_monitor(&s.layout.monitor)?;
    if let Some(scale) = scale_override {
        monitor.dpi = scale;
    }
    monitor.scale = pillbox_core::shape::draw_scale(monitor.dpi, &s.layout);
    Ok(monitor)
}

impl App {
    pub fn new(scale_override: Option<f32>) -> Result<Self> {
        system::init_com();
        let settings = store::load().unwrap_or_default();
        let placement = Placement::from_layout(&settings.layout);
        let monitor = read_monitor(&settings, scale_override)?;
        let (x, y, w, h) = placement.window_rect(monitor.rect, monitor.scale);
        let window = PillboxWindow::create(x, y, w, h)?;
        let renderer = Renderer::new(window.hwnd(), w, h)?;
        let tray = Tray::new()?;
        let watcher = match store::dir().and_then(|d| DirWatcher::new(&d)) {
            Ok(w) => Some(w),
            Err(e) => {
                tracing::warn!("settings will not hot-reload: {e:#}");
                None
            }
        };
        let foreground = match ForegroundHook::install() {
            Ok(h) => Some(h),
            Err(e) => {
                tracing::warn!("per-app rules are off: {e:#}");
                None
            }
        };
        // Developer aid: a pretend track, so the music UI can be exercised without sound.
        let fake_track = std::env::var("PILLBOX_FAKE_TRACK")
            .ok()
            .filter(|s| !s.is_empty());
        let fake_levels = fake_track.is_some();
        let media = MediaService::start(window.waker(), fake_track);
        let now = Instant::now();
        let c = placement.closed;
        let (jobs_tx, jobs_rx) = mpsc::channel();
        let tools = Tools::new(window.waker());
        let mut app = Self {
            theme: make_theme(&settings),
            params: SpringParams::DEFAULT.with_speed(settings.behavior.animation_speed),
            settings,
            placement,
            scale_override,
            monitor,
            win_pos: (x, y),
            win_size: (w, h),
            window,
            renderer,
            _tray: tray,
            frame_timer: FrameTimer::new()?,
            watcher,
            _foreground: foreground,
            springs: std::array::from_fn(|i| match i {
                0 => Spring::new(c.w),
                1 => Spring::new(c.h),
                2 => Spring::new(c.r),
                _ => Spring::new(0.0),
            }),
            stepper: FixedStep::default(),
            hovering: false,
            open: false,
            mode: Mode::Normal,
            user_visible: true,
            animating: false,
            dirty: true,
            backdrop_failed: false,
            last_backdrop: now,
            faces: Faces::default(),
            bend_buf: Vec::new(),
            media,
            fake_levels,
            track: None,
            art: None,
            palette: Palette::default(),
            visualizer: None,
            lyrics: LyricsService::default(),
            lyrics_view: None,
            glow: [0.0; GLOW_STOPS],
            pulse: 0.0,
            last_line: None,
            last_glow_at: now,
            reveal_song_until: None,
            intro_until: None,
            clock_second: 0,
            tab: Tab::Home,
            summoned: None,
            summon_next: None,
            hits: Vec::new(),
            scrub: None,
            toast: None,
            // Read once at start-up, when this thread's view of the keyboard is fresh.
            lock_on: LOCK_KEYS.map(|(vk, _)| window::lock_key_is_on(vk)),
            lock_down: [false; 3],
            hotkeys: Vec::new(),
            jobs_tx,
            jobs_rx,
            shot_restore: None,
            shelf: store::load_json("shelf.json"),
            power: PowerWatch::default(),
            headphones: None,
            internet: None,
            sensors: None,
            in_use: InUse::default(),
            call_since: None,
            mic_muted: devices::mic_muted().unwrap_or(false),
            t: tools,
            drop_target: None,
            fullscreen: None,
            strip: Strip::default(),
            mirror: mirror::Mirror::default(),
            play: games::Play::load(),
            phone: phone::Phone::default(),
            script: script::Script::load(),
            notes: notes::Notes::default(),
            quit_armed: false,
            shown_tab: Tab::Home,
            quit_now: false,
            started: now,
            last_frame: now,
            next_frame: now,
            anim_started: now,
            anim_frames: 0,
        };
        app.log_layout();
        app.mode = app.mode_for_foreground();
        if app.mode != Mode::Normal {
            tracing::info!("Pillbox mode: {:?}", app.mode);
        }
        app.retarget();
        for spring in &mut app.springs {
            spring.snap();
        }
        app.animating = false;
        if app.ambient_running() {
            // A moving theme starts moving at once.
            app.start_animating();
        }
        app.sync_surface();
        app.sync_timers();
        app.sync_fullscreen_timer();
        app.sync_services();
        app.sync_hotkeys();
        app.sync_shelf_selection();
        app.start_tools();
        app.drop_target = match dragdrop::DropRegistration::register(app.window.hwnd()) {
            Ok(r) => Some(r),
            Err(e) => {
                tracing::warn!("files cannot be dropped on Pillbox: {e:#}");
                None
            }
        };
        if std::env::var_os("PILLBOX_FAKE_MIC").is_some() {
            app.set_in_use(InUse::default());
        }
        app.begin_intro();
        app.check_power();
        app.fake_notice();
        // Draw and shape the window before it becomes visible, so there is no flash.
        app.draw();
        app.sync_visibility();
        Ok(app)
    }

    /// The start-up greeting: Pillbox slides in from its screen edge, says it is running, and
    /// stays in view for `INTRO_MS` even when it is set to hide, so a start at sign-in is seen.
    /// Skipped while a game or a full-screen app has the screen (the mode is not Normal).
    fn begin_intro(&mut self) {
        if !self.settings.behavior.start_greeting
            || self.mode != Mode::Normal
            || !self.effective_visible()
        {
            return;
        }
        self.intro_until = Some(Instant::now() + Duration::from_millis(INTRO_MS.into()));
        self.window.set_timer(TIMER_INTRO, INTRO_MS);
        if system::animations_enabled() {
            // Starts out of sight; `retarget` then springs it in.
            self.springs[3].value = 1.0;
            self.springs[3].velocity = 0.0;
        }
        self.show_toast(
            Toast::icon("\u{E7E8}", "Pillbox is running"),
            INTRO_MS - 400,
        );
    }

    fn log_layout(&self) {
        tracing::info!(
            "Pillbox on {} at ({},{}) {}x{} px, scale {:.2}, {:?} {:?}, surface {:?}",
            self.monitor.name,
            self.win_pos.0,
            self.win_pos.1,
            self.win_size.0,
            self.win_size.1,
            self.monitor.scale,
            self.settings.layout.style,
            self.settings.layout.side,
            self.settings.appearance.surface,
        );
    }

    // ---- geometry -------------------------------------------------------------------------

    fn pill(&self) -> Pill {
        let d = Dims {
            w: self.springs[0].value,
            h: self.springs[1].value,
            r: self.springs[2].value,
        };
        self.placement.pill(
            self.win_size,
            self.monitor.scale,
            d,
            self.springs[3].value.clamp(0.0, 1.0),
        )
    }

    /// 0 = resting pill, 1 = fully open card. Measured along the axis the resting pill keeps
    /// fixed (its height on the top edge, its width on a side edge), since the other one now
    /// follows what the pill shows.
    fn openness(&self) -> f32 {
        let (c, o) = (self.placement.closed, self.placement.open);
        let (grown, span) = match self.placement.side {
            Side::Top => ((self.springs[1].value - c.h).abs(), (o.h - c.h).abs()),
            Side::Left | Side::Right => ((self.springs[0].value - c.w).abs(), (o.w - c.w).abs()),
        };
        (grown / span.max(1.0)).clamp(0.0, 1.0)
    }

    /// The face the resting pill shows when nothing else is on it.
    fn resting_face(&self, now: pillbox_core::face::LocalTime) -> scene::Face {
        if self.settings.appearance.resting_face == RestingFace::NextEvent {
            scene::Face::Text(self.next_event_text())
        } else {
            self.faces.face(&self.settings, now)
        }
    }

    /// Whether the resting pill shows the playing song (nothing in front of it). The order is
    /// the one `scene::paint_resting` draws in: notice, notifications, call, song, face.
    fn resting_shows_song(&self) -> bool {
        self.toast.is_none()
            && !self.note_stack_showing()
            && !self.call_showing()
            && self.settings.song_on_pill(self.music_playing())
    }

    /// The call timer has the closed pill (`Settings::call_on_pill`: music may keep it).
    fn call_showing(&self) -> bool {
        self.settings
            .call_on_pill(self.call_since.is_some(), self.music_playing())
    }

    /// The resting pill's size for what it shows now (see `Placement::resting`).
    fn resting_dims(&mut self) -> Dims {
        let what = if let Some(toast) = &self.toast {
            let h = self.placement.closed.h;
            let look = self.text_look();
            let text = if toast.icon.is_empty() && toast.swatch.is_none() {
                self.renderer
                    .measure(&toast.text, h * REST_TEXT * look.size, look.bold)
                    + h
            } else {
                self.renderer.measure(&toast.text, h * NOTICE_TEXT, true)
                    + h * (NOTICE_LEAD + NOTICE_PAD)
            };
            Resting::Notice { text }
        } else if self.note_stack_showing() || self.call_showing() {
            Resting::Notice { text: 0.0 }
        } else if self.resting_shows_song() {
            let art = self.art.is_some();
            let h = self.placement.closed.h;
            let title = match (&self.track, self.settings.music.title_in_pill) {
                (Some(t), true) => {
                    let title = music::pill_title(&t.title);
                    Some(self.renderer.measure(&title, h * MUSIC_TEXT, true))
                }
                _ => None,
            };
            let bars = self.settings.behavior.visualizer != VisualizerStyle::Off;
            Resting::Music { title, art, bars }
        } else {
            let h = self.placement.closed.h;
            let look = self.text_look();
            let text = match self.resting_face(system::local_time()) {
                scene::Face::Text(t) => {
                    self.renderer
                        .measure(&t, h * REST_TEXT * look.size, look.bold)
                }
                _ => 0.0,
            };
            Resting::Quiet { text }
        };
        self.placement.resting(what)
    }

    /// The song's colours on the resting pill: (how much of them shows, the three colours).
    /// Only while a song with a cover plays on the resting pill (and `music.pill_colors` is on);
    /// fades out otherwise.
    fn tint_target(&self) -> (f32, [Rgba; 3]) {
        let colours = self.palette.colors.map(music::pill_tint);
        let on = self.settings.music.pill_colors
            && self.resting_shows_song()
            && self.art.is_some()
            && !self.open;
        (if on { 1.0 } else { 0.0 }, colours)
    }

    fn hides_until_hover(&self) -> bool {
        self.settings.layout.hidden_until_hover && self.mode == Mode::Normal
    }

    /// Something that brings a hidden Pillbox into view right now (layout.reveal): a notice, a
    /// notification, a flash, a call, a ringing reminder, or music. Pillbox hides again as
    /// soon as none of them is left.
    fn revealed(&self) -> bool {
        let r = &self.settings.layout.reveal;
        self.intro_until.is_some_and(|until| Instant::now() < until)
            || (r.notices && self.toast.is_some())
            || (r.notifications && self.note_stack_showing())
            || (r.flashes && self.script.showing_flash.is_some())
            || (r.calls && self.call_showing())
            || (r.reminders && !self.t.ringing.is_empty())
            || match r.music {
                MusicReveal::Never => false,
                MusicReveal::WhilePlaying => self.music_playing(),
                MusicReveal::OnSongChange => self
                    .reveal_song_until
                    .is_some_and(|until| Instant::now() < until),
            }
    }

    /// How the text on the closed pill looks (ppearance.clock).
    fn text_look(&self) -> TextLook {
        let c = &self.settings.appearance.clock;
        let color = match c.color {
            ClockColor::Text => self.theme.text,
            ClockColor::Accent => self.theme.accent,
            ClockColor::Custom => theme::parse_hex(&c.custom_color).unwrap_or(self.theme.text),
        };
        TextLook {
            size: c.size,
            bold: c.bold,
            color,
        }
    }

    /// The Music tab shows the lyrics beside the player (the wider card; on the top edge only,
    /// a side edge has no room for two columns).
    fn music_with_lyrics(&self) -> bool {
        self.settings.music.enabled && self.settings.music.lyrics && self.track.is_some()
    }

    /// The interactive area for the current frame.
    fn region(&self, pill: &Pill) -> RegionKey {
        if self.mode == Mode::Strip {
            // The strip never takes the pointer: every click goes to the game underneath.
            return RegionKey {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
                ellipse: 0,
            };
        }
        let key = pill.region(REGION_PAD);
        if !self.hides_until_hover() {
            return key;
        }
        let band = self
            .placement
            .trigger_region(self.win_size, self.monitor.scale);
        if self.springs[3].target > 0.5 && self.springs[3].value > 0.97 {
            // Out of sight: only a thin band on the screen edge listens for the pointer.
            return band;
        }
        // In sight or sliding in: cover the pill, the gap up to the screen edge and the edge
        // band itself, so the pointer that summoned Pillbox at the edge stays "on" it.
        RegionKey {
            left: key.left.min(band.left),
            top: key.top.min(band.top),
            right: key.right.max(band.right),
            bottom: key.bottom.max(band.bottom),
            ellipse: key.ellipse,
        }
    }

    // ---- state ----------------------------------------------------------------------------

    fn effective_visible(&self) -> bool {
        self.user_visible && self.mode != Mode::Hidden
    }

    fn sync_visibility(&mut self) {
        let visible = self.effective_visible();
        self.window.set_visible(visible);
        self.sync_surface();
        self.sync_music();
        self.sync_strip();
        self.sync_mirror();
        self.sync_phone();
    }

    /// Point the springs at the shape the current state calls for and start animating.
    fn retarget(&mut self) {
        let dims = match (self.mode, self.open) {
            (Mode::Strip, _) => self.placement.strip,
            (_, true) if self.tab == Tab::System => {
                self.placement.system_tab(self.system_tab_cards())
            }
            (_, true) if self.tab == Tab::Music && self.music_with_lyrics() => {
                self.placement.lyrics
            }
            (_, true) if self.tab == Tab::Controls && self.t.link_panel.is_some() => {
                self.placement.links
            }
            (_, true) if self.tab.tall() => self.placement.tall,
            (_, true) => self.placement.open,
            (_, false) => self.resting_dims(),
        };
        // A reason to be seen (evealed) brings a hidden Pillbox into view for as long as it
        // lasts.
        let hide = if self.hides_until_hover() && !self.open && !self.hovering && !self.revealed() {
            1.0
        } else {
            0.0
        };
        let (tint, colours) = self.tint_target();
        let mut targets = [0.0f32; SPRINGS];
        targets[..5].copy_from_slice(&[dims.w, dims.h, dims.r, hide, tint]);
        for (i, c) in colours.iter().enumerate() {
            let at = TINT_SPRINGS + 3 * i;
            // Fading out keeps the colours it had, so it does not turn grey on the way.
            let rgb = if tint > 0.0 {
                [c.r, c.g, c.b]
            } else {
                [
                    self.springs[at].target,
                    self.springs[at + 1].target,
                    self.springs[at + 2].target,
                ]
            };
            targets[at..at + 3].copy_from_slice(&rgb);
        }
        let tab_at = self
            .tab_list()
            .iter()
            .position(|t| *t == self.tab)
            .unwrap_or(0) as f32;
        targets[CHIP_SPRING] = tab_at;
        targets[TAB_IN_SPRING] = 1.0;
        let mut changed = false;
        let card_shown = self.open && self.openness() > 0.6;
        if self.tab != self.shown_tab {
            self.shown_tab = self.tab;
            if card_shown {
                // The new tab's content eases in while the highlight slides over.
                self.springs[TAB_IN_SPRING].value = 0.15;
                self.springs[TAB_IN_SPRING].velocity = 0.0;
                changed = true;
            }
        }
        if !card_shown {
            // Opening shows the highlight already in place and the content as it is.
            for i in [CHIP_SPRING, TAB_IN_SPRING] {
                self.springs[i].target = targets[i];
                self.springs[i].snap();
            }
        }
        for (spring, target) in self.springs.iter_mut().zip(targets) {
            if spring.target != target {
                spring.target = target;
                changed = true;
            }
        }
        if changed {
            self.start_animating();
        }
    }

    fn start_animating(&mut self) {
        if self.animating {
            return;
        }
        self.animating = true;
        self.stepper.reset();
        let now = Instant::now();
        self.last_frame = now;
        self.next_frame = now;
        self.anim_started = now;
        self.anim_frames = 0;
    }

    fn set_open(&mut self, open: bool) {
        let open = open && self.mode == Mode::Normal;
        if self.open == open {
            return;
        }
        self.open = open;
        // A half-done quit does not survive closing the card.
        self.quit_armed = false;
        if open {
            // A tab something asked for wins over the tab the card usually opens on.
            self.summoned = self.summon_next.take();
            if self.summoned.is_none()
                && self.settings.behavior.reopen_tab == ReopenTab::Music
                && self.music_shown()
                && self.tab_list().contains(&Tab::Music)
            {
                self.tab = Tab::Music;
            }
            self.leave_missing_tab();
            self.tab_shown();
        } else {
            self.scrub = None;
            self.t.pressed = None;
            self.t.shelf_menu = None;
            if self.pillbox_typing() {
                self.unfocus(true);
            }
            self.t.slider = None;
            self.lyrics_view = None;
            self.close_link_panel();
            self.sync_music();
            self.sync_stats_timer();
        }
        self.retarget();
        if self.ambient_running() {
            self.start_animating();
        }
    }

    fn pointer_entered(&mut self) {
        if self.hovering || self.mode != Mode::Normal {
            return;
        }
        self.hovering = true;
        self.window.kill_timer(TIMER_CLOSE);
        // A flash is a notification: the pointer on it waits for a click instead of opening
        // the card over it (the card opens when the flash goes, if the pointer is still there).
        if !self.open && self.script.showing_flash.is_none() && !self.note_stack_showing() {
            self.window
                .set_timer(TIMER_OPEN, self.settings.behavior.open_delay_ms);
        }
        self.retarget();
    }

    fn pointer_left(&mut self) {
        // While the progress bar is being dragged the pointer may wander off the card; while
        // typing or while a reminder rings the card stays put.
        if !self.hovering || self.scrub.is_some() || self.holds_open() {
            return;
        }
        self.hovering = false;
        self.window.kill_timer(TIMER_OPEN);
        if self.open {
            self.window
                .set_timer(TIMER_CLOSE, self.settings.behavior.close_delay_ms);
        } else {
            self.retarget();
        }
    }

    // ---- music ----------------------------------------------------------------------------

    /// Is there a track the music views should show?
    fn music_shown(&self) -> bool {
        self.settings.music.enabled && self.track.is_some()
    }

    fn music_playing(&self) -> bool {
        self.settings.music.enabled && self.track.as_ref().is_some_and(|t| t.playing)
    }

    /// Start or stop the visualizer's audio capture: it runs only while music is playing, the
    /// Pillbox can be seen and the visualizer is not switched off.
    fn sync_music(&mut self) {
        // The bars on the pill, or the Music tab's glow while that tab is open, need the audio.
        let glow = self.settings.music.reactive_background && self.open && self.tab == Tab::Music;
        let want = self.music_playing()
            && self.effective_visible()
            && self.mode == Mode::Normal
            && ((self.settings.music.on_pill
                && self.settings.behavior.visualizer != VisualizerStyle::Off)
                || glow);
        if want && self.visualizer.is_none() {
            self.visualizer = Some(Visualizer::start());
        } else if !want {
            self.visualizer = None;
        }
        if self.ambient_running() {
            self.start_animating();
        }
    }

    fn on_media_update(&mut self, update: MediaUpdate) {
        match update.art {
            ArtUpdate::Unchanged => {}
            ArtUpdate::Cleared => {
                self.art = None;
                self.palette = Palette::default();
                self.renderer.clear_image(ImageSlot::Art);
                self.renderer.clear_image(ImageSlot::ArtSoft);
            }
            ArtUpdate::New(art) => {
                self.palette = music::palette(&art.bgra);
                tracing::debug!(colors = ?self.palette.colors, "cover palette");
                let soft =
                    music::soften(&art.bgra, art.width as usize, art.height as usize, 7, 0.3);
                self.art = Some(ArtPixels {
                    width: art.width,
                    height: art.height,
                    bgra: art.bgra,
                    soft,
                });
                self.upload_art();
            }
        }
        let changed_song = match (&self.track, &update.track) {
            (Some(a), Some(b)) => a.title != b.title || a.artist != b.artist,
            (None, None) => false,
            _ => true,
        };
        if changed_song {
            match &update.track {
                Some(t) => tracing::info!(
                    "now playing from {} (player allows: seek {}, next {}, previous {})",
                    t.app_id,
                    t.can_seek,
                    t.can_next,
                    t.can_previous
                ),
                None => tracing::info!("nothing playing"),
            }
            self.scrub = None;
            self.lyrics_view = None;
            self.last_line = None;
            if update.track.is_some() {
                // A hidden Pillbox shows a new song for a few seconds (eveal.music).
                let secs = self.settings.layout.reveal.song_seconds;
                self.reveal_song_until = Some(Instant::now() + Duration::from_secs(secs.into()));
                self.window.set_timer(TIMER_REVEAL, secs * 1000);
            }
        }
        self.track = update.track;
        self.lyrics.update(
            self.settings.music.enabled && self.settings.music.lyrics,
            self.track.as_ref(),
            self.window.waker(),
        );
        self.sync_music();
        // A song started or changed: Pillbox must be on top to show it.
        self.window.raise();
        self.dirty = true;
    }

    fn upload_art(&mut self) {
        if let Some(art) = &self.art {
            self.renderer
                .set_image(ImageSlot::Art, &art.bgra, art.width, art.height);
            self.renderer
                .set_image(ImageSlot::ArtSoft, &art.soft, art.width, art.height);
        }
    }

    fn seek(&mut self, seconds: f64) {
        self.media.send(MediaCommand::SeekTo(seconds));
        // Show the new position at once; the player confirms it a moment later.
        if let Some(track) = &mut self.track {
            track.position = seconds;
            track.at = Instant::now();
        }
        self.dirty = true;
    }

    fn act(&mut self, action: Action, x: f32, y: f32) {
        // With the shelf file menu open, a click either picks a line, switches tab, or only
        // closes the menu.
        if self.t.shelf_menu.take().is_some() {
            self.dirty = true;
            if !matches!(action, Action::ShelfMenu(..) | Action::Tab(_)) {
                return;
            }
        }
        // Any click but the second one on the quit button takes the "armed" state back.
        if self.quit_armed && !matches!(action, Action::Quit) {
            self.quit_armed = false;
            self.dirty = true;
        }
        // Any click but the second one on a Stop button takes the "Stop?" back.
        if self.script.confirm_stop.is_some() && !matches!(action, Action::AgentStop(_)) {
            self.script.confirm_stop = None;
            self.dirty = true;
        }
        match action {
            Action::NoteOpen(id) => self.open_note_app(id),
            Action::Quit => {
                if self.quit_armed {
                    self.quit_now = true;
                } else {
                    self.quit_armed = true;
                    self.dirty = true;
                }
            }
            Action::AgentAllow(_)
            | Action::AgentDeny(_)
            | Action::AgentStop(_)
            | Action::AgentNewChat(_)
            | Action::AgentOpen(_)
            | Action::AgentExpand(_)
            | Action::ActivityDismiss(_) => self.act_activity(action),
            Action::Tab(tab) => self.show_tab(tab),
            Action::PlayPause => self.media.send(MediaCommand::PlayPause),
            Action::Next => self.media.send(MediaCommand::Next),
            Action::Previous => self.media.send(MediaCommand::Previous),
            Action::SeekBar { left, right } => {
                let mut scrub = Scrub {
                    left,
                    right,
                    fraction: 0.0,
                };
                scrub.fraction = scrub_fraction(&scrub, x);
                self.scrub = Some(scrub);
                self.dirty = true;
            }
            Action::SeekTo(seconds) => self.seek(f64::from(seconds)),
            Action::LyricsLive => {
                self.lyrics_view = None;
                self.dirty = true;
            }
            // The lyrics column only exists to catch the wheel.
            Action::LyricsArea => {}
            other => {
                self.act_tool(other, x, y);
            }
        }
    }

    // ---- lock keys ------------------------------------------------------------------------

    fn on_lock_key(&mut self, vk: u16, down: bool) {
        let Some(i) = LOCK_KEYS.iter().position(|(code, _)| *code == vk) else {
            return;
        };
        // Holding the key repeats "down"; only the first press toggles the lock.
        let pressed = down && !self.lock_down[i];
        self.lock_down[i] = down;
        if !pressed {
            return;
        }
        self.lock_on[i] = !self.lock_on[i];
        let state = if self.lock_on[i] { "on" } else { "off" };
        self.show_toast(Toast::text(format!("{} {state}", LOCK_KEYS[i].1)), TOAST_MS);
    }

    // ---- device notices -------------------------------------------------------------------

    /// Read the battery and announce what changed: the charger, a full or a low battery.
    fn check_power(&mut self) {
        let now = system::battery().map(|b| Power {
            percent: b.percent,
            plugged_in: b.plugged_in,
        });
        let quiet = self.mode != Mode::Normal;
        if let Some(notice) = self.power.update(now, quiet) {
            self.on_power_notice(notice);
        }
    }

    fn on_power_notice(&mut self, notice: PowerNotice) {
        let wanted = match notice {
            PowerNotice::Charging(_) | PowerNotice::Unplugged(_) | PowerNotice::Full => {
                self.settings.popups.charging
            }
            PowerNotice::Low(_) => self.settings.popups.low_battery,
        };
        if !wanted {
            return;
        }
        let low = matches!(notice, PowerNotice::Low(_));
        tracing::info!("device notice: {}", notice.text());
        let (glyph, ms) = match notice {
            PowerNotice::Low(_) => (G_BATTERY_LOW, 5000),
            PowerNotice::Unplugged(_) | PowerNotice::Full => (G_BATTERY, 3000),
            PowerNotice::Charging(_) => (G_CHARGING, 3000),
        };
        let toast = Toast {
            alert: low,
            ..Toast::icon(glyph, notice.text())
        };
        self.show_toast(toast, ms);
    }

    /// The watchers run only while their notices are wanted.
    fn sync_device_watches(&mut self) {
        let p = &self.settings.popups;
        if !p.headphones {
            self.headphones = None;
        } else if self.headphones.is_none() {
            self.headphones = HeadphoneWatch::start(self.window.waker());
        }
        if !p.internet {
            self.internet = None;
        } else if self.internet.is_none() {
            self.internet = InternetWatch::start(self.window.waker());
        }
    }

    /// A short notice about a device. Nothing shows while a game is in front.
    fn device_notice(&mut self, toast: Toast, ms: u32) {
        if self.mode == Mode::Normal {
            self.show_toast(toast, ms);
        }
    }

    fn on_headphones(&mut self, name: &str, connected: bool) {
        let what = if connected {
            "connected"
        } else {
            "disconnected"
        };
        tracing::info!("headphones {what}");
        self.device_notice(Toast::icon(G_HEADPHONES, format!("{name} {what}")), 3000);
    }

    fn on_drives(&mut self, mask: u32, connected: bool) {
        if !self.settings.popups.drives {
            return;
        }
        if let Some(text) = notices::drive_text(&notices::drive_letters(mask), connected) {
            tracing::info!("{text}");
            self.device_notice(Toast::icon(G_DRIVE, text), 3000);
        }
    }

    fn on_internet(&mut self, online: bool) {
        tracing::info!("internet {}", if online { "back" } else { "lost" });
        let toast = if online {
            Toast::icon(G_ONLINE, "Back online")
        } else {
            Toast {
                alert: true,
                ..Toast::icon(G_OFFLINE, "No internet")
            }
        };
        self.device_notice(toast, 4000);
    }

    /// Developer aid: `PILLBOX_FAKE_NOTICE` shows a device notice or a game recap at once:
    /// `charging:63`, `unplugged:63`, `full`, `low:15`, `headphones:WH-1000XM4:on` (or `off`),
    /// `drive:on`, `internet:off`, `recap:THE FINALS:4800`.
    fn fake_notice(&mut self) {
        let Ok(spec) = std::env::var("PILLBOX_FAKE_NOTICE") else {
            return;
        };
        let (kind, rest) = spec.split_once(':').unwrap_or((spec.as_str(), ""));
        match (kind, rest.rsplit_once(':')) {
            ("charging", _) => {
                self.on_power_notice(PowerNotice::Charging(rest.parse().unwrap_or(50)));
            }
            ("low", _) => self.on_power_notice(PowerNotice::Low(rest.parse().unwrap_or(10))),
            ("unplugged", _) => {
                self.on_power_notice(PowerNotice::Unplugged(rest.parse().unwrap_or(50)));
            }
            ("full", _) => self.on_power_notice(PowerNotice::Full),
            ("headphones", Some((name, state))) => self.on_headphones(name, state == "on"),
            ("drive", _) => self.on_drives(1 << 4, rest == "on"),
            ("internet", _) => self.on_internet(rest == "on"),
            ("recap", Some((game, seconds))) => {
                self.show_recap(game, seconds.parse().unwrap_or(0));
            }
            _ => {}
        }
    }

    fn show_toast(&mut self, toast: Toast, ms: u32) {
        // Not a flash, unless `show_flash` says so right after.
        self.script.showing_flash = None;
        self.toast = Some(toast);
        // A notice must be seen: make sure no window sits on top of Pillbox.
        self.window.raise();
        self.window.set_timer(TIMER_TOAST, ms);
        self.retarget();
        self.dirty = true;
    }

    // ---- global shortcuts -----------------------------------------------------------------

    /// (Re)register every shortcut from the settings. Called at start and when they change.
    fn sync_hotkeys(&mut self) {
        for id in self.hotkeys.drain(..) {
            self.window.unregister_hotkey(id);
        }
        let h = &self.settings.hotkeys;
        let mut wanted: Vec<(i32, String)> = [
            (HK_TOGGLE, &h.toggle_pillbox),
            (HK_PLAY, &h.play_pause),
            (HK_MUTE, &h.mute_mic),
            (HK_HIDE, &h.hide),
            (HK_SHOT, &h.screenshot),
            (HK_COLOR, &h.color_picker),
        ]
        .into_iter()
        .map(|(id, text)| (id, text.clone()))
        .collect();
        for (slot, launch) in self.settings.launcher.slots.iter().enumerate() {
            wanted.push((LAUNCH_HOTKEY + slot as i32, launch.hotkey.clone()));
        }
        let mut taken: Vec<String> = Vec::new();
        let mut seen: Vec<Hotkey> = Vec::new();
        for (id, text) in wanted {
            if text.is_empty() {
                continue;
            }
            let Ok(key) = Hotkey::parse(&text) else {
                tracing::warn!("shortcut `{text}` is not valid; skipped");
                continue;
            };
            if seen.contains(&key) {
                tracing::warn!("shortcut {key} is set twice; only the first use is active");
                continue;
            }
            seen.push(key);
            match self.window.register_hotkey(id, key.modifiers(), key.vk) {
                Ok(()) => self.hotkeys.push(id),
                Err(e) => {
                    tracing::warn!("shortcut {key} could not be registered: {e}");
                    taken.push(key.to_string());
                }
            }
        }
        tracing::info!("{} global shortcuts active", self.hotkeys.len());
        if !taken.is_empty() {
            self.show_toast(
                Toast {
                    alert: true,
                    ..Toast::icon(
                        "\u{E765}",
                        format!("{} is used by another app", taken.join(", ")),
                    )
                },
                3500,
            );
        }
    }

    fn on_hotkey(&mut self, id: i32) {
        tracing::debug!("shortcut {id}");
        match id {
            HK_TOGGLE => {
                if !self.user_visible {
                    self.set_user_visible(true);
                }
                if self.open {
                    self.hovering = false;
                    self.set_open(false);
                } else {
                    // Stays open until the shortcut is pressed again or the pointer passes
                    // over the card and leaves it.
                    self.hovering = true;
                    self.window.kill_timer(TIMER_CLOSE);
                    self.set_open(true);
                }
                self.retarget();
            }
            HK_PLAY => self.media.send(MediaCommand::PlayPause),
            HK_MUTE => self.toggle_mic_mute(),
            HK_HIDE => self.set_user_visible(!self.user_visible),
            HK_SHOT => self.begin_screenshot(),
            HK_COLOR => self.pick_color(),
            id if id >= LAUNCH_HOTKEY => self.launch_slot((id - LAUNCH_HOTKEY) as usize),
            _ => {}
        }
    }

    // ---- microphone, camera, calls -----------------------------------------------------------

    fn toggle_mic_mute(&mut self) {
        let muted = !devices::mic_muted().unwrap_or(self.mic_muted);
        match devices::set_mic_muted(muted) {
            Ok(0) => self.show_toast(Toast::icon("\u{E720}", "No microphone found"), TOAST_MS),
            Ok(n) => {
                tracing::info!(
                    "microphone {} ({n} device(s))",
                    if muted { "muted" } else { "on" }
                );
                self.mic_muted = muted;
                let toast = if muted {
                    Toast {
                        alert: true,
                        ..Toast::icon("\u{EC54}", "Microphone muted")
                    }
                } else {
                    Toast::icon("\u{E720}", "Microphone on")
                };
                self.show_toast(toast, TOAST_MS);
            }
            Err(e) => {
                tracing::warn!("mic mute: {e:#}");
                self.show_toast(Toast::text("Could not change the microphone"), TOAST_MS);
            }
        }
    }

    /// The sensor thread runs only while the call timer or the dot is wanted.
    fn sync_sensors(&mut self) {
        let want = self.settings.calls.call_timer || self.settings.calls.sensor_dot;
        if want && self.sensors.is_none() {
            self.sensors = SensorWatch::start(self.window.waker());
        } else if !want {
            self.sensors = None;
            self.set_in_use(InUse::default());
        }
    }

    fn set_in_use(&mut self, mut raw: InUse) {
        // Developer aid: pretend an app holds the microphone (`PILLBOX_FAKE_MIC=discord`).
        if let Ok(app) = std::env::var("PILLBOX_FAKE_MIC")
            && !app.is_empty()
            && !raw.mic.contains(&app)
        {
            raw.mic.push(app);
        }
        let ignore = &self.settings.calls.ignore_apps;
        let keep = |apps: Vec<String>| -> Vec<String> {
            apps.into_iter()
                .filter(|a| !ignore.iter().any(|i| a.contains(i.as_str())))
                .collect()
        };
        let now = InUse {
            mic: keep(raw.mic),
            camera: keep(raw.camera),
        };
        if now == self.in_use {
            return;
        }
        tracing::info!("in use: mic {:?}, camera {:?}", now.mic, now.camera);
        let calling = !now.mic.is_empty();
        if calling && self.call_since.is_none() {
            self.call_since = Some(Instant::now());
            self.mic_muted = devices::mic_muted().unwrap_or(false);
        } else if !calling {
            self.call_since = None;
        }
        self.in_use = now;
        if self.call_since.is_some() && self.settings.calls.call_timer {
            self.window.set_timer(TIMER_CALL, 1000);
        } else {
            self.window.kill_timer(TIMER_CALL);
        }
        self.dirty = true;
    }

    fn launch_slot(&mut self, slot: usize) {
        let Some(s) = self.settings.launcher.slots.get(slot).cloned() else {
            return;
        };
        match system::open_path(&s.path, &s.args) {
            Ok(()) => tracing::info!("launched {}", s.path),
            Err(e) => {
                tracing::warn!("launcher: {e:#}");
                let name = if s.name.is_empty() { &s.path } else { &s.name };
                self.show_toast(Toast::text(format!("Could not open {name}")), 2500);
            }
        }
    }

    fn set_user_visible(&mut self, visible: bool) {
        self.user_visible = visible;
        tray::set_pillbox_visible(visible);
        self.sync_visibility();
        self.dirty = true;
        tracing::info!("Pillbox {}", if visible { "shown" } else { "hidden" });
    }

    /// Copy the hex code of the colour under the pointer.
    fn pick_color(&mut self) {
        let Some((r, g, b)) = screen::pixel_under_cursor() else {
            return self.show_toast(Toast::text("Could not read the screen"), TOAST_MS);
        };
        let hex = image::hex(r, g, b);
        match clipboard::write(self.window.hwnd(), &clipboard::Content::Text(hex.clone())) {
            Ok(()) => {
                let toast = Toast {
                    swatch: Some(Rgba::new(
                        f32::from(r) / 255.0,
                        f32::from(g) / 255.0,
                        f32::from(b) / 255.0,
                        1.0,
                    )),
                    ..Toast::text(format!("{hex} copied"))
                };
                self.show_toast(toast, 2200);
            }
            Err(e) => {
                tracing::warn!("colour picker: {e:#}");
                self.show_toast(Toast::text("Clipboard is busy"), TOAST_MS);
            }
        }
    }

    /// Screenshots leave Pillbox out: it is excluded from capture for a moment first,
    /// which Windows applies on its next composition pass, hence the short timer.
    fn begin_screenshot(&mut self) {
        if self.shot_restore.is_some() {
            return;
        }
        self.shot_restore = Some(self.window.is_excluded_from_capture());
        self.window.set_excluded_from_capture(true);
        self.window.set_timer(TIMER_SHOT, 60);
    }

    fn finish_screenshot(&mut self) {
        self.window.kill_timer(TIMER_SHOT);
        let shot = screen::capture_monitor_under_cursor();
        if let Some(previous) = self.shot_restore.take() {
            self.window.set_excluded_from_capture(previous);
            self.sync_surface();
        }
        let img = match shot {
            Ok(img) => img,
            Err(e) => {
                tracing::warn!("screenshot failed: {e:#}");
                return self.show_toast(Toast::text("Screenshot failed"), TOAST_MS);
            }
        };
        if let Err(e) = clipboard::write(
            self.window.hwnd(),
            &clipboard::Content::Picture(img.clone()),
        ) {
            tracing::warn!("screenshot not copied: {e:#}");
        }
        self.save_png_in_background(img);
    }

    fn save_png_in_background(&mut self, img: Bgra) {
        let tx = self.jobs_tx.clone();
        let waker = self.window.waker();
        let spawned = std::thread::Builder::new()
            .name("screenshot".into())
            .spawn(move || {
                let result = (|| -> std::result::Result<PathBuf, String> {
                    let dir = screen::screenshots_dir().map_err(|e| format!("{e:#}"))?;
                    let t = system::local_time();
                    let name = format!(
                        "Pillbox {:04}-{:02}-{:02} {:02}{:02}{:02}.png",
                        t.year, t.month, t.day, t.hour, t.minute, t.second
                    );
                    let path = dir.join(name);
                    let png = img.to_png()?;
                    std::fs::write(&path, png).map_err(|e| e.to_string())?;
                    Ok(path)
                })();
                let _ = tx.send(Job::ScreenshotSaved(result));
                waker.wake();
            });
        if let Err(e) = spawned {
            tracing::warn!("could not start saving the screenshot: {e}");
        }
    }

    // ---- shelf ----------------------------------------------------------------------------

    fn shelf_add(&mut self, paths: Vec<PathBuf>) {
        for p in paths.into_iter().rev() {
            self.shelf.retain(|q| q != &p);
            self.shelf.insert(0, p);
        }
        self.t.shelf_selected = vec![false; self.shelf.len()];
        self.save_shelf();
        self.dirty = true;
    }

    fn save_shelf(&self) {
        if let Err(e) = store::save_json("shelf.json", &self.shelf) {
            tracing::warn!("shelf not saved: {e:#}");
        }
    }

    /// A new screenshot file exists. The shelf picks it up when that option is on.
    fn on_capture_saved(&mut self, path: PathBuf) {
        if self.settings.shelf.captures {
            self.shelf_add(vec![path]);
        }
    }

    fn poll_jobs(&mut self) {
        while let Ok(job) = self.jobs_rx.try_recv() {
            match job {
                Job::ScreenshotSaved(Ok(path)) => {
                    tracing::info!("screenshot saved to {}", path.display());
                    self.on_capture_saved(path);
                    self.show_toast(Toast::icon("\u{E722}", "Screenshot copied and saved"), 2200);
                }
                Job::ScreenshotSaved(Err(e)) => {
                    tracing::warn!("screenshot not saved: {e}");
                    self.show_toast(Toast::text("Screenshot not saved"), TOAST_MS);
                }
                Job::ClipThumb(id, thumb) => self.upload_clip_thumb(id, thumb),
                Job::Ocr(result) => self.on_ocr(result),
                Job::Online(online) => self.on_online(online),
                Job::AccountUsage(result) => self.on_account_usage(result),
                Job::Saved(Ok(Some(path))) => {
                    tracing::info!("shelf file saved to {}", path.display());
                    self.show_toast(Toast::icon("\u{E896}", "Saved"), TOAST_MS);
                }
                Job::Saved(Ok(None)) => {}
                Job::Saved(Err(e)) => {
                    tracing::warn!("shelf download: {e}");
                    self.show_toast(Toast::text("Not saved"), TOAST_MS);
                }
            }
        }
    }

    // ---- settings -------------------------------------------------------------------------

    /// Glass capture and capture exclusion follow the surface and visibility.
    fn sync_surface(&mut self) {
        let glass = self.theme.glass.is_some();
        // Exclusion must be in place before capture starts, or the glass would sample itself.
        self.window
            .set_excluded_from_capture(glass || self.settings.appearance.hide_from_capture);
        let want = glass && self.effective_visible() && self.window.is_excluded_from_capture();
        if !want {
            self.renderer.disable_backdrop();
            return;
        }
        if self.renderer.has_backdrop() || self.backdrop_failed {
            return;
        }
        let origin = (
            (self.win_pos.0 - self.monitor.rect.left).max(0) as u32,
            (self.win_pos.1 - self.monitor.rect.top).max(0) as u32,
        );
        let source = (
            self.monitor.rect.width().max(0) as u32,
            self.monitor.rect.height().max(0) as u32,
        );
        if let Err(e) = self
            .renderer
            .enable_backdrop(self.monitor.handle, origin, source)
        {
            tracing::warn!("glass falls back to a solid fill: {e:#}");
            self.backdrop_failed = true;
        }
    }

    /// Start only the timers the current face needs.
    fn sync_timers(&mut self) {
        self.window.set_timer(
            TIMER_MINUTE,
            face::ms_until_next_minute(system::local_time()),
        );
        let face = self.settings.appearance.resting_face;
        if face == RestingFace::Time && self.settings.appearance.clock.seconds {
            // Twice a second, so a second is never shown late; redraws only when it changed.
            self.window.set_timer(TIMER_SECOND, 250);
        } else {
            self.window.kill_timer(TIMER_SECOND);
        }
        if face == RestingFace::Cpu {
            self.faces.sample_cpu();
            self.window.set_timer(TIMER_CPU, 2000);
        } else {
            self.window.kill_timer(TIMER_CPU);
        }
        if face == RestingFace::Pet {
            self.window.set_timer(TIMER_PET, 4200);
        } else {
            self.window.kill_timer(TIMER_PET);
            self.window.kill_timer(TIMER_PET_EYES);
            self.faces.pet_blink = false;
        }
        if faces::wants_weather(&self.settings) {
            self.faces
                .refresh_weather(&self.settings, self.window.waker());
            self.window.set_timer(TIMER_WEATHER, 30 * 60 * 1000);
        } else {
            self.window.kill_timer(TIMER_WEATHER);
        }
    }

    /// Tell the background services what the settings ask of them.
    fn sync_services(&mut self) {
        let m = &self.settings.music;
        self.media.send(MediaCommand::SetFilter {
            enabled: m.enabled,
            mode: m.source_mode,
            apps: m.source_apps.clone(),
        });
        self.window.watch_lock_keys(self.settings.popups.lock_keys);
        if !self.settings.popups.lock_keys {
            self.toast = None;
        }
        self.lyrics.update(
            m.enabled && m.lyrics,
            self.track.as_ref(),
            self.window.waker(),
        );
        self.sync_music();
        self.sync_sensors();
        self.sync_device_watches();
        self.sync_script();
        self.sync_notes();
        self.sync_system_window();
    }

    fn reload_settings(&mut self) {
        let Some(new) = store::load() else { return };
        if new == self.settings {
            return;
        }
        tracing::info!("settings changed; applying");
        let keys_changed = new.hotkeys != self.settings.hotkeys
            || new.launcher.slots.iter().map(|s| &s.hotkey).ne(self
                .settings
                .launcher
                .slots
                .iter()
                .map(|s| &s.hotkey));
        self.settings = new;
        if keys_changed {
            self.sync_hotkeys();
        }
        self.sync_clipboard_listener();
        self.refresh_widgets(false);
        self.theme = make_theme(&self.settings);
        self.params = SpringParams::DEFAULT.with_speed(self.settings.behavior.animation_speed);
        self.placement = Placement::from_layout(&self.settings.layout);
        self.backdrop_failed = false;
        let mode = self.mode_for_foreground();
        if mode != self.mode {
            tracing::info!("Pillbox mode: {mode:?}");
            self.mode = mode;
        }
        self.relayout(true);
        self.retarget();
        self.sync_visibility();
        self.sync_timers();
        self.sync_fullscreen_timer();
        self.sync_services();
        self.leave_missing_tab();
        // The System refresh period may have changed.
        self.sync_stats_timer();
        self.dirty = true;
    }

    /// Re-read the monitor and move/resize the window to match. `force` applies even when the
    /// monitor itself is unchanged (the layout settings changed).
    fn relayout(&mut self, force: bool) {
        let monitor = match read_monitor(&self.settings, self.scale_override) {
            Ok(m) => m,
            Err(e) => {
                tracing::warn!("could not read the monitor: {e:#}");
                return;
            }
        };
        if monitor == self.monitor && !force {
            return;
        }
        let (x, y, w, h) = self.placement.window_rect(monitor.rect, monitor.scale);
        let moved = (x, y) != self.win_pos || (w, h) != self.win_size || monitor != self.monitor;
        self.monitor = monitor;
        if !moved {
            return;
        }
        if let Err(e) = self.window.set_rect(x, y, w, h) {
            tracing::error!("{e:#}");
        }
        self.win_pos = (x, y);
        // The backdrop is tied to the window's position and size; rebuild it either way.
        self.renderer.disable_backdrop();
        if (w, h) != self.win_size {
            self.win_size = (w, h);
            if let Err(e) = self.renderer.resize(w, h) {
                tracing::warn!("resize failed ({e:#}); the next draw rebuilds the renderer");
            }
        }
        self.backdrop_failed = false;
        self.sync_surface();
        self.log_layout();
        self.dirty = true;
    }

    // ---- main loop ------------------------------------------------------------------------

    pub fn run(&mut self) -> Result<()> {
        loop {
            let timer = self.animating.then_some(&self.frame_timer);
            let pumped = window::pump(timer, self.watcher.as_ref());
            match pumped {
                Pumped::Quit => return Ok(()),
                Pumped::SettingsChanged => {
                    if let Some(w) = &self.watcher {
                        w.rearm();
                    }
                    // Editors and our own atomic save produce several writes; wait for quiet.
                    self.window.set_timer(TIMER_RELOAD, 120);
                }
                Pumped::Messages | Pumped::FrameDue => {}
            }
            for event in window::drain_events() {
                if self.handle(event) {
                    return Ok(());
                }
            }
            if self.animating {
                // Draw when the frame timer fired, or immediately on the first frame.
                if pumped == Pumped::FrameDue || Instant::now() >= self.next_frame {
                    self.frame()?;
                } else if self.dirty && self.effective_visible() {
                    // A slow ambient cadence (a drifting theme, a resting shimmer) must not
                    // hold back a hover or a click: those draw at once.
                    self.draw();
                }
            } else if self.dirty && self.effective_visible() {
                self.draw();
            }
        }
    }

    /// Returns true when the app should quit.
    fn handle(&mut self, event: Event) -> bool {
        match event {
            Event::MouseMove { x, y } => {
                if let Some(scrub) = &mut self.scrub {
                    scrub.fraction = scrub_fraction(scrub, x);
                    self.dirty = true;
                } else if self.hides_until_hover() || self.pill().contains(x, y) {
                    // With hide-until-hover the whole region counts, including the edge band.
                    self.pointer_entered();
                }
                if self.open {
                    self.tools_mouse_move(x, y);
                }
            }
            Event::MouseLeave => {
                self.tools_mouse_left();
                self.pointer_left();
            }
            Event::Click { x, y } => {
                tracing::debug!("click at {x},{y}");
                // Buttons drawn on top of a row come later in the list, so the last match wins.
                if self.open
                    && let Some(hit) = self.hits.iter().rev().find(|h| h.contains(x, y)).copied()
                {
                    self.t.pressed = Some((x, y));
                    self.dirty = true;
                    self.act(hit.action, x, y);
                } else if self.click_flash() || self.click_notes() {
                    // A flash on the closed pill: the click opened the Activity tab.
                } else if self.t.focus.is_some() {
                    self.unfocus(true);
                }
            }
            Event::RightClick { x, y } => {
                if self.open
                    && let Some(hit) = self.hits.iter().rev().find(|h| h.contains(x, y)).copied()
                {
                    self.act_secondary(hit.action);
                }
            }
            Event::Wheel { x, y, delta } => self.on_wheel(x, y, delta),
            Event::Char(c) => self.on_char(c),
            Event::Key { vk, ctrl, .. } => self.on_key(vk, ctrl),
            Event::FocusLost => self.on_focus_lost(),
            Event::ClipboardChanged => self.on_clipboard_changed(),
            Event::DragEnter => self.on_drag_enter(),
            Event::DragLeave => self.on_drag_leave(),
            Event::FilesDropped => self.on_files_dropped(),
            Event::WidgetMoved(id) => self.on_widget_moved(id),
            Event::WidgetResized(id) => self.on_widget_resized(id),
            Event::WidgetClose(id) => self.on_widget_close(id),
            Event::WidgetClick { id, x, y } => self.on_widget_click(id, x, y),
            Event::MouseUp { x, y } => {
                self.tools_mouse_up();
                if let Some(scrub) = self.scrub.take() {
                    if let Some(duration) =
                        self.track.as_ref().map(|t| t.duration).filter(|d| *d > 0.0)
                    {
                        self.seek(f64::from(scrub_fraction(&scrub, x)) * duration);
                    }
                    // The drag may have ended off the card; then the pointer has left.
                    if !self.pill().contains(x, y) {
                        self.pointer_left();
                    }
                    self.dirty = true;
                }
            }
            Event::LockKey { vk, down } => self.on_lock_key(vk, down),
            Event::Timer(id) => self.on_timer(id),
            Event::DisplayChanged => {
                self.relayout(false);
                // Exclusive fullscreen games change the display mode on the way in and out.
                self.refresh_mode();
            }
            Event::PowerChanged => {
                if self.settings.appearance.resting_face == RestingFace::Battery {
                    self.dirty = true;
                }
                self.check_power();
            }
            Event::Drives { mask, connected } => self.on_drives(mask, connected),
            Event::ForegroundChanged => {
                // Windows can stack the newly active window above Pillbox (see
                // `Window::raise`): put it back on top now, and once more a moment later in
                // case the activation was still settling.
                self.window.raise();
                self.window.set_timer(TIMER_RAISE, 400);
                self.refresh_mode();
            }
            Event::BackdropFrame => self.on_backdrop_frame(),
            Event::Wake => {
                if let Some(update) = self.media.poll() {
                    self.on_media_update(update);
                }
                if self.faces.poll() {
                    // A new weather reading: the widgets show it right away.
                    self.refresh_widgets(false);
                    self.dirty = true;
                }
                if self.lyrics.poll() {
                    self.dirty = true;
                }
                if let Some(state) = self.sensors.as_ref().and_then(SensorWatch::poll) {
                    self.set_in_use(state);
                }
                if let Some((name, on)) = self.headphones.as_ref().and_then(HeadphoneWatch::poll) {
                    self.on_headphones(&name, on);
                }
                if let Some(online) = self.internet.as_ref().and_then(InternetWatch::poll) {
                    self.on_internet(online);
                }
                self.poll_jobs();
                self.poll_thumbs();
                self.poll_controls();
                self.poll_mirror();
                self.poll_phone();
                self.poll_script();
                self.poll_notes();
            }
            Event::Hotkey(id) => self.on_hotkey(id),
            Event::Tray(TrayCommand::Settings) => open_settings_window(),
            Event::Tray(TrayCommand::ToggleSystemWindow) => self.toggle_system_window(),
            Event::Tray(TrayCommand::ToggleVisible) => self.set_user_visible(!self.user_visible),
            Event::Tray(TrayCommand::ToggleAutostart) => {
                let enable = !system::autostart_enabled();
                match system::set_autostart(enable) {
                    Ok(()) => tracing::info!("start with Windows: {enable}"),
                    Err(e) => tracing::error!("could not change autostart: {e:#}"),
                }
            }
            Event::Tray(TrayCommand::DumpFrame) => {
                if let Some(path) = std::env::var_os("PILLBOX_DUMP") {
                    self.renderer.request_dump(PathBuf::from(path));
                    self.dirty = true;
                }
            }
            Event::Tray(TrayCommand::ForceOpen(open)) => {
                self.hovering = open;
                self.set_open(open);
                self.retarget();
            }
            Event::Tray(TrayCommand::ShowTab(index)) => {
                let wanted = match index {
                    1 => Tab::Music,
                    2 => Tab::Music, // the lyrics are on the Music tab
                    3 => Tab::Clipboard,
                    4 => Tab::Shelf,
                    5 => Tab::Reminders,
                    6 => Tab::Controls,
                    7 => Tab::Activity,
                    8 => Tab::Notifications,
                    9 => Tab::System,
                    _ => Tab::Home,
                };
                if self.tab_list().contains(&wanted) {
                    self.tab = wanted;
                }
                self.dirty = true;
                self.tab_shown();
                self.retarget();
                if self.ambient_running() {
                    self.start_animating();
                }
            }
            Event::Tray(TrayCommand::Quit) => {
                self.flush_play();
                return true;
            }
        }
        if self.quit_now {
            // The card's quit button, clicked twice.
            self.flush_play();
            return true;
        }
        false
    }

    fn on_timer(&mut self, id: usize) {
        match id {
            TIMER_OPEN => {
                self.window.kill_timer(TIMER_OPEN);
                if self.hovering {
                    self.set_open(true);
                }
            }
            TIMER_CLOSE => {
                self.window.kill_timer(TIMER_CLOSE);
                if !self.hovering {
                    self.set_open(false);
                }
            }
            TIMER_MINUTE => {
                // Re-align to the next minute boundary each time, so the clock never drifts.
                self.window.set_timer(
                    TIMER_MINUTE,
                    face::ms_until_next_minute(system::local_time()),
                );
                // Windows does not report every percent.
                self.check_power();
                self.check_reminders();
                self.count_screen_time();
                // Old clipboard copies go even when nobody opens the tab.
                self.expire_clips();
                self.script_housekeeping();
                self.flush_play();
                self.refresh_widgets(false);
                self.dirty = true;
            }
            TIMER_SECOND => {
                let second = system::local_time().second;
                if second != self.clock_second {
                    self.clock_second = second;
                    self.dirty = true;
                }
            }
            TIMER_REVEAL => {
                self.window.kill_timer(TIMER_REVEAL);
                self.reveal_song_until = None;
                self.retarget();
                self.dirty = true;
            }
            TIMER_INTRO => {
                self.window.kill_timer(TIMER_INTRO);
                self.intro_until = None;
                self.retarget();
                self.dirty = true;
            }
            TIMER_STATS => self.refresh_widgets(true),
            phone::TIMER_PHONE => self.phone_timeout(),
            phone::TIMER_PHONE_CLIP => self.copy_phone_text(),
            TIMER_RAISE => {
                self.window.kill_timer(TIMER_RAISE);
                self.window.raise();
            }
            script::TIMER_REPLY => self.expire_replies(),
            script::TIMER_PRESENCE => self.refresh_agent_presence(),
            notes::TIMER_NOTES => self.notes_stack_expired(),
            TIMER_STRIP => self.sample_strip(),
            TIMER_FULLSCREEN => self.refresh_mode(),
            games::TIMER_RECAP => self.check_recap(),
            TIMER_FOCUS => self.on_focus_tick(),
            TIMER_CLIP_RETRY => {
                self.window.kill_timer(TIMER_CLIP_RETRY);
                self.on_clipboard_changed();
            }
            TIMER_ONLINE => self.fetch_online(true),
            TIMER_PHOTO => self.next_photo(),
            TIMER_LINKS => self.on_links_tick(),
            TIMER_CPU => {
                self.faces.sample_cpu();
                self.dirty = true;
            }
            TIMER_PET => {
                self.faces.pet_blink = true;
                self.window.set_timer(TIMER_PET_EYES, 160);
                self.dirty = true;
            }
            TIMER_PET_EYES => {
                self.window.kill_timer(TIMER_PET_EYES);
                self.faces.pet_blink = false;
                self.dirty = true;
            }
            TIMER_RELOAD => {
                self.window.kill_timer(TIMER_RELOAD);
                self.reload_settings();
            }
            TIMER_BACKDROP => {
                self.window.kill_timer(TIMER_BACKDROP);
                self.on_backdrop_frame();
            }
            TIMER_WEATHER => {
                self.faces
                    .refresh_weather(&self.settings, self.window.waker());
            }
            TIMER_TOAST => {
                self.window.kill_timer(TIMER_TOAST);
                self.toast = None;
                self.script.showing_flash = None;
                // The next waiting flash (if any) takes the pill.
                self.flash_pump();
                if self.hovering && !self.open && self.script.showing_flash.is_none() {
                    self.window
                        .set_timer(TIMER_OPEN, self.settings.behavior.open_delay_ms);
                }
                self.retarget();
                self.dirty = true;
            }
            TIMER_SHOT => self.finish_screenshot(),
            TIMER_CALL => {
                // Once a second during a call: the timer text, and the mute state in case it
                // was changed somewhere else (Windows settings, a headset button).
                self.mic_muted = devices::mic_muted().unwrap_or(self.mic_muted);
                // Redraw only where the call is on screen (not while music keeps the pill).
                if self.call_showing() || (self.open && self.tab == Tab::Controls) {
                    self.dirty = true;
                }
            }
            _ => {}
        }
    }

    /// A capture frame is waiting. Take it now, or shortly if we sampled very recently.
    fn on_backdrop_frame(&mut self) {
        let since = self.last_backdrop.elapsed();
        if since < BACKDROP_INTERVAL {
            let wait = (BACKDROP_INTERVAL - since).as_millis() as u32;
            self.window.set_timer(TIMER_BACKDROP, wait.max(1));
            return;
        }
        self.last_backdrop = Instant::now();
        match self.renderer.pump_backdrop() {
            Ok(true) => self.dirty = true,
            Ok(false) => {}
            Err(e) => {
                tracing::warn!("backdrop capture failed ({e:#}); using a solid fill");
                self.renderer.disable_backdrop();
                self.backdrop_failed = true;
                self.dirty = true;
            }
        }
    }

    fn springs_settled(&self) -> bool {
        self.springs.iter().all(Spring::is_settled)
    }

    /// Is anything moving besides the springs? Sparkles and petals on the open home card;
    /// the visualizer on the resting pill; the progress bar and lyrics on the music tabs.
    /// With none of these Pillbox draws nothing at all between events.
    fn ambient_running(&self) -> bool {
        if !self.effective_visible() || self.mode != Mode::Normal {
            return false;
        }
        self.theme_moving() || self.other_ambient()
    }

    /// Ambient motion other than the theme's: decorations on the home card, music.
    fn other_ambient(&self) -> bool {
        if self.open && self.tab == Tab::Home {
            return self.theme.touch.is_some();
        }
        // Closed, the song moves only while it is on the pill.
        self.music_playing() && (self.open || self.settings.music.on_pill)
    }

    /// A moving theme (drift or shimmer) is in sight. Hidden until hover and slid away, it
    /// stands still; so it does with Pillbox hidden or a fullscreen app in front
    /// (`ambient_running` checks those).
    fn theme_moving(&self) -> bool {
        let out_of_sight = self.springs[3].target > 0.5 && self.springs[3].value > 0.97;
        self.theme.motion.is_some() && !out_of_sight
    }

    /// How long to wait for the next ambient frame once the shape has settled. Only a moving
    /// theme: its own slow cadence (a shimmer sleeps between sweeps). Anything else: the
    /// ambient rate.
    fn ambient_interval(&self) -> Duration {
        match self.theme.motion {
            Some(m) if !self.other_ambient() => {
                let wait = m.next_frame_in(self.started.elapsed().as_secs_f32());
                Duration::from_secs_f32(wait.clamp(1.0 / MOTION_FPS as f32, 60.0))
            }
            _ => Duration::from_secs_f64(1.0 / AMBIENT_FPS),
        }
    }

    /// One animation frame: advance the springs by wall time, draw, schedule the next frame.
    fn frame(&mut self) -> Result<()> {
        let now = Instant::now();
        let steps = self.stepper.advance((now - self.last_frame).as_secs_f32());
        self.last_frame = now;
        // The same springs open and close the card; closing runs a little faster.
        let params = if !self.open && self.openness() > 0.02 {
            self.params.with_speed(CLOSE_SPEED)
        } else {
            self.params
        };
        for _ in 0..steps {
            for spring in &mut self.springs {
                spring.step(params, STEP);
            }
        }
        let settled = self.springs_settled();
        if settled {
            for spring in &mut self.springs {
                spring.snap();
            }
        }
        self.animating = !settled || self.ambient_running();
        if self.effective_visible() {
            self.draw();
        }
        self.anim_frames += 1;
        if self.animating {
            if settled && self.theme_moving() && !self.other_ambient() {
                // A theme's own slow cadence, counted from now (a shimmer's rest can be long).
                self.next_frame = now + self.ambient_interval();
            } else {
                let interval = if settled {
                    self.ambient_interval()
                } else {
                    Duration::from_secs_f64(1.0 / MOTION_FPS)
                };
                // Keep a steady cadence; if we fell behind, restart from now instead of
                // bursting.
                self.next_frame = (self.next_frame + interval).max(now);
            }
            self.frame_timer
                .arm(self.next_frame.saturating_duration_since(Instant::now()))?;
        } else {
            let secs = self.anim_started.elapsed().as_secs_f32();
            tracing::debug!(
                "animation done: {} frames in {secs:.2}s ({:.0} fps)",
                self.anim_frames,
                self.anim_frames as f32 / secs.max(0.001)
            );
        }
        Ok(())
    }

    fn paint(&mut self, pill: &Pill) -> Result<()> {
        let scale = self.monitor.scale;
        let now = system::local_time();
        let face = self.resting_face(now);
        // The song's colours as they fade and blend.
        let amount = self.springs[4].value.clamp(0.0, 1.0);
        let tint = (amount > 0.01).then(|| {
            let c = |i: usize| self.springs[i].value.clamp(0.0, 1.0);
            let colour = |n: usize| {
                let at = TINT_SPRINGS + 3 * n;
                Rgba::new(c(at), c(at + 1), c(at + 2), 1.0)
            };
            ([colour(0), colour(1), colour(2)], amount)
        });
        let tab_list = self.tab_list();
        let call = self
            .call_since
            .filter(|_| self.settings.calls.call_timer)
            .map(|since| CallView {
                seconds: since.elapsed().as_secs(),
                app: self.in_use.mic.first().map_or("", String::as_str),
                muted: self.mic_muted,
            });
        let ui = UiState {
            pointer: self.t.pointer,
            pressed: self.t.pressed,
            focus: self.t.focus.filter(|_| self.t.focus_widget.is_none()),
            edits: &self.t.edits,
            scroll: self.t.scroll(self.tab),
            now_secs: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs() as i64),
            now_min: pillbox_core::calendar::now_minutes(now),
            clock_24h: self.settings.behavior.clock_24h,
            clip: ClipUi {
                history: &self.t.clip,
                filter: self.t.clip_filter,
                snippets: &self.settings.clipboard.snippets,
                two_columns: self.settings.clipboard.two_columns,
                enabled: self.settings.clipboard.enabled,
            },
            shelf: ShelfUi {
                files: &self.shelf,
                selected: &self.t.shelf_selected,
                phone_enabled: self.settings.phone.enabled,
                phone: self.phone.ui(),
                menu: self.shelf_menu_ui(),
            },
            reminders: RemindersUi {
                list: &self.t.reminders,
                ringing: &self.t.ringing,
                error: self.t.reminder_error.as_deref(),
                snooze_minutes: self.settings.reminders.snooze_minutes,
            },
            controls: ControlsUi {
                volume: self.t.volume,
                brightness: self.t.ctl.brightness,
                wifi: self.t.ctl.wifi,
                bluetooth: self.t.ctl.bluetooth,
                mic_muted: Some(self.mic_muted),
                call,
                launcher: &self.settings.launcher.slots,
                mirror: self.mirror.on.then_some(crate::tabs::MirrorUi {
                    live: self.mirror.live,
                    message: &self.mirror.message,
                }),
                links: crate::tabs::LinksUi {
                    panel: self.t.link_panel,
                    wifi_adapter: self.t.wifi.adapter,
                    wifi: &self.t.wifi.nets,
                    bluetooth: &self.t.bluetooth,
                    wifi_loaded: self.t.wifi_loaded,
                    bluetooth_loaded: self.t.bluetooth_loaded,
                    busy: self.t.link_busy.as_deref(),
                    note: self.t.link_note.as_deref(),
                },
            },
            widgets: WidgetsUi {
                shown: &self.settings.widgets.shown,
                data: &self.t.wdata,
                play_open: self.t.play_open,
                play_day: self.t.play_day,
                weather_open: self.t.weather_open,
            },
            notes: crate::tabs::NotesUi {
                store: &self.notes.store,
                now_ms: script::now_ms(),
                message: self.notes_message(),
            },
            activity: crate::tabs::ActivityUi {
                board: &self.script.board,
                hidden: &self.settings.script.hidden_agents,
                now_ms: script::now_ms(),
                expanded: self.script.expanded,
                confirm_stop: self.script.confirm_stop,
                claude_running: self.script.claude_running,
                usage_note: if self.settings.script.claude_account_usage {
                    self.script
                        .usage_problem
                        .unwrap_or("Asking Anthropic for your usage…")
                } else {
                    "Usage limits show while Claude Code runs in a terminal"
                },
            },
        };
        let time = face::format_time(now, self.settings.behavior.clock_24h);
        let date = face::format_date(now);
        let blur = self.theme.glass.map_or(0.0, |g| {
            (2.0 + 26.0 * self.settings.appearance.glass_blur) * g.blur * scale
        });
        let bends = self.theme.glass.is_some_and(|g| g.bend) && self.renderer.has_backdrop();
        if bends {
            let (w, h) = (self.win_size.0 as usize, self.win_size.1 as usize);
            bend::displacement_map(&mut self.bend_buf, w, h, pill, 18.0 * scale);
        }
        let bands = if self.fake_levels {
            // Developer aid: the pretend track is silent, so it gets pretend levels that move,
            // to see the visualizer styles.
            let t = self.started.elapsed().as_secs_f32();
            std::array::from_fn(|i| {
                let i = i as f32;
                (0.5 + 0.45 * (t * (2.1 + i * 0.7) + i * 1.3).sin()).clamp(0.0, 1.0)
            })
        } else {
            self.visualizer
                .as_ref()
                .map_or([0.0; BANDS], Visualizer::bands)
        };
        // The Music tab's glow follows the audio, and each new lyric line gives it a flash.
        let tick = Instant::now();
        let dt = (tick - self.last_glow_at).as_secs_f32();
        self.last_glow_at = tick;
        let reactive = self.settings.music.reactive_background
            && self.open
            && self.tab == Tab::Music
            && self.track.is_some();
        if reactive {
            music::glow_step(&mut self.glow, &bands, dt);
            if let (LyricsState::Found(l), Some(t)) = (&self.lyrics.state, &self.track) {
                let line = l.index_at(t.position_now() as f32);
                if line != self.last_line {
                    if line.is_some() {
                        self.pulse = 1.0;
                    }
                    self.last_line = line;
                }
            }
            self.pulse = music::pulse_step(self.pulse, dt);
        }
        let (glow, pulse) = (self.glow, self.pulse);
        let text_look = self.text_look();
        let music_wide = self.music_with_lyrics() && self.placement.side == Side::Top;
        let music = self
            .track
            .as_ref()
            .filter(|_| self.settings.music.enabled)
            .map(|t| MusicView {
                title: &t.title,
                artist: &t.artist,
                playing: t.playing,
                position: t.position_now() as f32,
                duration: t.duration as f32,
                has_art: self.art.is_some(),
                palette: self.palette,
                bands,
                lyrics: &self.lyrics.state,
                scrub: self.scrub.map(|s| s.fraction),
                can_seek: t.can_seek,
                can_next: t.can_next,
                can_previous: t.can_previous,
                glow: reactive.then_some((glow, pulse)),
                lyrics_view: self.lyrics_view,
            });
        let note_stack = self.note_stack();
        let scene = Scene {
            pill: *pill,
            win: (self.win_size.0 as f32, self.win_size.1 as f32),
            scale,
            side: self.placement.side,
            theme: &self.theme,
            blur,
            bend: bends.then_some((self.bend_buf.as_slice(), 34.0 * scale)),
            openness: self.openness(),
            tint,
            strip: (self.mode == Mode::Strip).then_some(&self.strip.view),
            face: &face,
            text_look,
            time: &time,
            date: &date,
            day_progress: face::day_progress(now),
            clock: self.started.elapsed().as_secs_f32(),
            tab: self.tab,
            music,
            music_wide,
            music_settings: &self.settings.music,
            visualizer: self.settings.behavior.visualizer,
            toast: self.toast.as_ref(),
            notes: note_stack.as_ref(),
            // On the closed pill only when it has the pill (music may keep it); the Controls
            // tab above shows any call.
            call: call.filter(|_| self.call_showing()),
            sensor_dot: if self.settings.calls.sensor_dot {
                (!self.in_use.mic.is_empty(), !self.in_use.camera.is_empty())
            } else {
                (false, false)
            },
            quit_armed: self.quit_armed,
            tab_left: self
                .placement
                .pill(self.win_size, scale, self.placement.open, 0.0)
                .left,
            tabs: &tab_list,
            tab_chip: self.springs[CHIP_SPRING].value,
            tab_in: self.springs[TAB_IN_SPRING].value,
            ui: &ui,
        };
        let mut frame = self.renderer.begin();
        let painted = scene::paint(&mut frame, &scene);
        let result = frame.finish();
        self.hits = painted.hits;
        self.t.scroll_max = painted.scroll_max;
        result
    }

    /// Update the click region and draw. A failed draw rebuilds the renderer once (device lost).
    fn draw(&mut self) {
        // What the resting pill shows may have changed since the last frame (a new song, a
        // notice that ended, the clock): point the springs at its size and colour. Starts an
        // animation only when a target moved.
        self.retarget();
        let pill = self.pill();
        let region = self.region(&pill);
        self.window.set_region(region);
        if let Err(e) = self.paint(&pill) {
            tracing::warn!("draw failed ({e:#}); rebuilding the renderer");
            match self.renderer.rebuild() {
                Ok(()) => {
                    self.backdrop_failed = false;
                    self.sync_surface();
                    self.upload_art();
                    self.reupload_thumbs();
                    if let Err(e) = self.paint(&pill) {
                        tracing::error!("draw failed again after rebuild: {e:#}");
                    }
                }
                Err(e) => tracing::error!("could not rebuild the renderer: {e:#}"),
            }
        }
        self.draw_widget_windows();
        // Opening, closing and switching tabs all redraw, so this keeps the camera and the
        // phone code in step.
        self.sync_mirror();
        self.sync_phone();
        self.dirty = false;
    }
}

/// Where along the progress bar the pointer is, 0..1.
fn scrub_fraction(scrub: &Scrub, x: f32) -> f32 {
    ((x - scrub.left) / (scrub.right - scrub.left).max(1.0)).clamp(0.0, 1.0)
}

/// Start the Settings window: this same executable with `--settings`, as its own process, so
/// Pillbox itself stays small and can never be blocked by it.
fn open_settings_window() {
    // Already open (maybe minimised or behind other windows): bring that one forward. A
    // second copy would only find it running and quit.
    if crate::platform::settings_window::bring_forward(crate::settings_ui::TITLE) {
        tracing::info!("settings window brought forward");
        return;
    }
    let spawned = std::env::current_exe()
        .and_then(|exe| std::process::Command::new(exe).arg("--settings").spawn());
    match spawned {
        Ok(_) => tracing::info!("settings window started"),
        Err(e) => tracing::error!("could not open the settings window: {e}"),
    }
}
