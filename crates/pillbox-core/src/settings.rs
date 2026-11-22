//! The settings file: `%APPDATA%\Pillbox\settings.json`.
//!
//! Parsing is forgiving on purpose. The file is edited by hand as well as by the Settings
//! window, so one bad value must not throw the rest away: each value is accepted on its own,
//! anything invalid keeps its default and produces a warning, and numbers are clamped.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Style {
    /// Floating pill below the screen edge.
    #[default]
    #[serde(alias = "island")]
    Pill,
    /// Attached to the screen edge, rounded only on the far side.
    Notch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    #[default]
    Top,
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Surface {
    #[default]
    Black,
    Graphite,
    PureBlack,
    Acrylic,
    LiquidGlass,
    CandyGlass,
    Rose,
    Lavender,
    Peach,
    Sakura,
}

impl Surface {
    pub const ALL: [Surface; 10] = [
        Surface::Black,
        Surface::Graphite,
        Surface::PureBlack,
        Surface::Acrylic,
        Surface::LiquidGlass,
        Surface::CandyGlass,
        Surface::Rose,
        Surface::Lavender,
        Surface::Peach,
        Surface::Sakura,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Surface::Black => "Black",
            Surface::Graphite => "Graphite",
            Surface::PureBlack => "Pure black (OLED)",
            Surface::Acrylic => "Acrylic",
            Surface::LiquidGlass => "Liquid Glass",
            Surface::CandyGlass => "Candy glass",
            Surface::Rose => "Rosé",
            Surface::Lavender => "Lavender",
            Surface::Peach => "Peach",
            Surface::Sakura => "Sakura",
        }
    }

    /// Glass looks sample the screen behind Pillbox.
    pub fn is_glass(self) -> bool {
        matches!(
            self,
            Surface::Acrylic | Surface::LiquidGlass | Surface::CandyGlass
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RestingFace {
    None,
    #[default]
    Time,
    Date,
    Weather,
    Battery,
    Cpu,
    NextEvent,
    Pet,
}

impl RestingFace {
    pub const ALL: [RestingFace; 8] = [
        RestingFace::None,
        RestingFace::Time,
        RestingFace::Date,
        RestingFace::Weather,
        RestingFace::Battery,
        RestingFace::Cpu,
        RestingFace::NextEvent,
        RestingFace::Pet,
    ];

    pub fn label(self) -> &'static str {
        match self {
            RestingFace::None => "Nothing",
            RestingFace::Time => "Time",
            RestingFace::Date => "Date",
            RestingFace::Weather => "Weather",
            RestingFace::Battery => "Battery",
            RestingFace::Cpu => "CPU",
            RestingFace::NextEvent => "Next event",
            RestingFace::Pet => "Pixel pet",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReopenTab {
    #[default]
    Last,
    Music,
}

/// What the clock face on the resting pill shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PillClock {
    /// "14:05"
    #[default]
    Time,
    /// "Fri 14:05"
    DayTime,
    /// "20 Nov 14:05"
    DateTime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Visualizer {
    /// No bars at all.
    Off,
    /// Square cells stacked from the bottom, like an old level meter.
    #[default]
    PixelBars,
    /// Rounded bars growing up and down from the middle.
    RoundedBars,
    /// Rounded bars standing on the bottom, like an equalizer.
    Equalizer,
    /// Rounded bars swelling from the centre outwards (lowest band in the middle).
    Burst,
    /// A smooth sound wave mirrored around the middle.
    Wave,
    /// Six dots that bounce and grow with the music.
    Dots,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleBehavior {
    /// Remove Pillbox completely while this app is in front.
    #[default]
    Hide,
    /// Shrink to a slim strip that does not open.
    Strip,
    /// Always show normally (overrides automatic hiding).
    Show,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Layout {
    pub style: Style,
    pub side: Side,
    /// "primary" or a display device name such as `\\.\DISPLAY2`.
    pub monitor: String,
    /// Distance from the screen edge, in DIPs. Ignored for the notch style. Not changed by
    /// `size`.
    pub distance: f32,
    /// Scale of the whole Pillbox, closed and open, text included: 1 = normal, `SIZE_MIN` to
    /// `SIZE_MAX`. Replaces the old per-shape pixel sizes (see `RETIRED`).
    pub size: f32,
    pub hidden_until_hover: bool,
    /// What brings a hidden Pillbox into view (with `hidden_until_hover`); it hides again when
    /// the reason ends.
    pub reveal: Reveal,
}

/// When a song brings a hidden Pillbox into view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MusicReveal {
    Never,
    /// Shown for as long as music plays.
    #[default]
    WhilePlaying,
    /// Shown for `Reveal::song_seconds` when a song starts or changes.
    OnSongChange,
}

/// The reasons that bring a hidden Pillbox into view. Each can be switched off.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Reveal {
    pub music: MusicReveal,
    /// Seconds a changed song stays in view (`MusicReveal::OnSongChange`), 2..30.
    pub song_seconds: u32,
    /// Windows notifications on the pill (needs `notifications.enabled`).
    pub notifications: bool,
    /// Flashes from `pillboxctl` and the coding agents.
    pub flashes: bool,
    /// A call: an app is using the microphone.
    pub calls: bool,
    /// A reminder that rings.
    pub reminders: bool,
    /// Short notices: lock keys, microphone muted, screenshot saved, colour copied.
    pub notices: bool,
}

impl Default for Reveal {
    fn default() -> Self {
        Self {
            music: MusicReveal::WhilePlaying,
            song_seconds: 6,
            notifications: true,
            flashes: true,
            calls: true,
            reminders: true,
            notices: true,
        }
    }
}

/// The smallest and largest `layout.size`.
pub const SIZE_MIN: f32 = 0.8;
pub const SIZE_MAX: f32 = 1.4;

impl Default for Layout {
    fn default() -> Self {
        Self {
            style: Style::Pill,
            side: Side::Top,
            monitor: "primary".into(),
            distance: 8.0,
            size: 1.0,
            hidden_until_hover: false,
            reveal: Reveal::default(),
        }
    }
}

/// Global shortcuts, as text such as `"Ctrl+Alt+N"`. An empty string means no shortcut.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Hotkeys {
    /// Open or close the card.
    #[serde(alias = "toggle_pillbox")]
    pub toggle_pillbox: String,
    pub play_pause: String,
    /// Mute or unmute the microphone for every app.
    pub mute_mic: String,
    /// Hide or show Pillbox.
    pub hide: String,
    pub screenshot: String,
    /// Copy the colour under the pointer as a hex code.
    pub color_picker: String,
    /// Open or close the chat drawer.
    pub chat_drawer: String,
}

impl Default for Hotkeys {
    fn default() -> Self {
        Self {
            toggle_pillbox: "Ctrl+Alt+N".into(),
            play_pause: "Ctrl+Alt+P".into(),
            // Ctrl+Alt+M is often owned by other apps (it was on this PC), so Shift is added.
            mute_mic: "Ctrl+Alt+Shift+M".into(),
            hide: "Ctrl+Alt+H".into(),
            screenshot: "Ctrl+Alt+S".into(),
            color_picker: "Ctrl+Alt+C".into(),
            chat_drawer: "Ctrl+Alt+Shift+T".into(),
        }
    }
}

impl Hotkeys {
    /// Every shortcut with its settings key.
    pub fn named(&self) -> [(&'static str, &str); 7] {
        [
            ("toggle_pillbox", &self.toggle_pillbox),
            ("play_pause", &self.play_pause),
            ("mute_mic", &self.mute_mic),
            ("hide", &self.hide),
            ("screenshot", &self.screenshot),
            ("color_picker", &self.color_picker),
            ("chat_drawer", &self.chat_drawer),
        ]
    }

    fn named_mut(&mut self) -> [(&'static str, &mut String); 7] {
        [
            ("toggle_pillbox", &mut self.toggle_pillbox),
            ("play_pause", &mut self.play_pause),
            ("mute_mic", &mut self.mute_mic),
            ("hide", &mut self.hide),
            ("screenshot", &mut self.screenshot),
            ("color_picker", &mut self.color_picker),
            ("chat_drawer", &mut self.chat_drawer),
        ]
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Behavior {
    pub open_delay_ms: u32,
    pub close_delay_ms: u32,
    /// 0.5 to 2.
    pub animation_speed: f32,
    pub reopen_tab: ReopenTab,
    pub clock_24h: bool,
    /// What the clock face on the resting pill shows besides the time.
    pub pill_clock: PillClock,
    pub visualizer: Visualizer,
    /// Slide in and say "Pillbox is running" for a moment after starting, even when hidden.
    pub start_greeting: bool,
}

impl Default for Behavior {
    fn default() -> Self {
        Self {
            open_delay_ms: 120,
            close_delay_ms: 180,
            animation_speed: 1.0,
            reopen_tab: ReopenTab::Last,
            clock_24h: true,
            pill_clock: PillClock::Time,
            visualizer: Visualizer::PixelBars,
            start_greeting: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Appearance {
    pub surface: Surface,
    /// `#RRGGBB`
    pub accent: String,
    /// Colours for the card: tabs, tiles, text and accent. `Default` keeps the surface's own.
    pub app_theme: AppTheme,
    /// 0 = barely blurred, 1 = very blurred. Glass surfaces only.
    pub glass_blur: f32,
    pub hide_from_capture: bool,
    pub resting_face: RestingFace,
    /// How the text on the closed pill (the clock and the other text faces) looks.
    pub clock: ClockLook,
    /// The user's own background fade, used when `app_theme` is `custom`.
    pub fade: FadeSettings,
    /// Let the themes that move (Aurora, Midnight neon, Lava) drift and shimmer. They stand
    /// still anyway while Pillbox is hidden, a fullscreen app is in front, or Windows has
    /// animation effects switched off.
    pub animate: bool,
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            surface: Surface::Black,
            accent: "#7C5CFF".into(),
            app_theme: AppTheme::Default,
            glass_blur: 0.5,
            hide_from_capture: false,
            resting_face: RestingFace::Time,
            clock: ClockLook::default(),
            fade: FadeSettings::default(),
            animate: true,
        }
    }
}

/// One colour of a background fade.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FadeStop {
    /// `#RRGGBB`
    pub color: String,
    /// Where along the fade this colour is at full strength, 0 (start) to 1 (end).
    pub at: f32,
}

impl Default for FadeStop {
    fn default() -> Self {
        Self {
            color: "#7C5CFF".into(),
            at: 0.0,
        }
    }
}

/// A background fade of the user's own (`AppTheme::Custom`): two or three colours along a
/// line. The text colour is chosen for it (see `theme::readable_fade`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FadeSettings {
    /// `FADE_STOPS_MIN` to `FADE_STOPS_MAX` colours, in order along the fade.
    pub stops: Vec<FadeStop>,
    /// Direction in degrees, 0..360: 0 = left to right, 90 = top to bottom, 45 = top left to
    /// bottom right.
    pub angle: f32,
    /// 0 = hard edges between the colours, 1 = each colour melts all the way into the next.
    pub softness: f32,
    /// How strongly the fade covers the surface, `FADE_OPACITY_MIN` to 1. On glass it is the
    /// strength of the tint over the blurred screen.
    pub opacity: f32,
}

pub const FADE_STOPS_MIN: usize = 2;
pub const FADE_STOPS_MAX: usize = 3;
pub const FADE_OPACITY_MIN: f32 = 0.3;

impl Default for FadeSettings {
    fn default() -> Self {
        let stop = |color: &str, at| FadeStop {
            color: color.into(),
            at,
        };
        Self {
            stops: vec![
                stop("#2B1C6B", 0.0),
                stop("#7A2C7E", 0.55),
                stop("#B8455A", 1.0),
            ],
            angle: 20.0,
            softness: 1.0,
            opacity: 1.0,
        }
    }
}

impl FadeSettings {
    /// Bring a hand-edited fade into range: 2..3 colours with valid hex, positions in order
    /// within 0..1, an angle within 0..360.
    fn sanitize(&mut self, warnings: &mut Vec<String>) {
        let d = FadeSettings::default();
        if self.stops.len() < FADE_STOPS_MIN {
            warnings
                .push("`appearance.fade.stops` needs at least 2 colours; using the default".into());
            self.stops = d.stops.clone();
        }
        if self.stops.len() > FADE_STOPS_MAX {
            warnings.push(format!(
                "`appearance.fade.stops`: only the first {FADE_STOPS_MAX} colours are used"
            ));
            self.stops.truncate(FADE_STOPS_MAX);
        }
        for (i, stop) in self.stops.iter_mut().enumerate() {
            if crate::theme::parse_hex(&stop.color).is_none() {
                warnings.push(format!(
                    "`appearance.fade.stops[{i}].color` = {:?} is not #RRGGBB; using the default",
                    stop.color
                ));
                stop.color = d.stops[i.min(d.stops.len() - 1)].color.clone();
            }
            if !stop.at.is_finite() {
                stop.at = 0.0;
            }
            stop.at = stop.at.clamp(0.0, 1.0);
        }
        self.stops.sort_by(|a, b| a.at.total_cmp(&b.at));
        self.angle = if self.angle.is_finite() {
            self.angle.rem_euclid(360.0)
        } else {
            d.angle
        };
        self.softness = if self.softness.is_finite() {
            self.softness.clamp(0.0, 1.0)
        } else {
            d.softness
        };
        self.opacity = if self.opacity.is_finite() {
            self.opacity.clamp(FADE_OPACITY_MIN, 1.0)
        } else {
            d.opacity
        };
    }
}

/// Colour of the text on the closed pill.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClockColor {
    /// The surface's text colour.
    #[default]
    Text,
    Accent,
    Custom,
}

/// Look of the closed pill's text. The pill's own height stays; the text grows inside it and
/// the pill widens to fit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ClockLook {
    /// Text size, `CLOCK_SIZE_MIN` to `CLOCK_SIZE_MAX` times the normal one.
    pub size: f32,
    pub bold: bool,
    /// Show seconds ("14:05:09"); the pill then redraws once a second.
    pub seconds: bool,
    /// "09:05" instead of "9:05".
    pub leading_zero: bool,
    pub color: ClockColor,
    /// `#RRGGBB`, used when `color` is `Custom`.
    pub custom_color: String,
}

pub const CLOCK_SIZE_MIN: f32 = 0.8;
pub const CLOCK_SIZE_MAX: f32 = 1.3;

impl Default for ClockLook {
    fn default() -> Self {
        Self {
            size: 1.0,
            bold: true,
            seconds: false,
            leading_zero: false,
            color: ClockColor::Text,
            custom_color: "#FFFFFF".into(),
        }
    }
}

/// A colour scheme for the open card.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AppTheme {
    /// The surface's own colours.
    #[default]
    Default,
    Midnight,
    Ocean,
    Forest,
    Sunset,
    Violet,
    Ember,
    Mono,
    // Fade themes: several colours across the pill and the card.
    /// Teal, green and violet that drift slowly.
    Aurora,
    /// Plum into rose into burnt orange.
    GoldenHour,
    /// Navy into teal.
    DeepSea,
    /// Indigo and violet with a light that sweeps across now and then.
    MidnightNeon,
    /// Light sage greens with dark text.
    ForestMist,
    /// Black-red into glowing orange that drifts slowly.
    Lava,
    /// Graphite into black.
    MonoFade,
    /// Light pink, lilac and sky blue with dark text.
    CottonCandy,
    /// The user's own fade (`appearance.fade`).
    Custom,
}

impl AppTheme {
    pub const ALL: [AppTheme; 17] = [
        AppTheme::Default,
        AppTheme::Midnight,
        AppTheme::Ocean,
        AppTheme::Forest,
        AppTheme::Sunset,
        AppTheme::Violet,
        AppTheme::Ember,
        AppTheme::Mono,
        AppTheme::Aurora,
        AppTheme::GoldenHour,
        AppTheme::DeepSea,
        AppTheme::MidnightNeon,
        AppTheme::ForestMist,
        AppTheme::Lava,
        AppTheme::MonoFade,
        AppTheme::CottonCandy,
        AppTheme::Custom,
    ];

    pub fn label(self) -> &'static str {
        match self {
            AppTheme::Default => "Surface colours",
            AppTheme::Midnight => "Midnight",
            AppTheme::Ocean => "Ocean",
            AppTheme::Forest => "Forest",
            AppTheme::Sunset => "Sunset",
            AppTheme::Violet => "Violet",
            AppTheme::Ember => "Ember",
            AppTheme::Mono => "Mono",
            AppTheme::Aurora => "Aurora",
            AppTheme::GoldenHour => "Golden hour",
            AppTheme::DeepSea => "Deep sea",
            AppTheme::MidnightNeon => "Midnight neon",
            AppTheme::ForestMist => "Forest mist",
            AppTheme::Lava => "Lava",
            AppTheme::MonoFade => "Mono fade",
            AppTheme::CottonCandy => "Cotton candy",
            AppTheme::Custom => "Your fade",
        }
    }

    /// A theme of several colours across the pill and the card (not one flat colour).
    pub fn is_fade(self) -> bool {
        matches!(
            self,
            AppTheme::Aurora
                | AppTheme::GoldenHour
                | AppTheme::DeepSea
                | AppTheme::MidnightNeon
                | AppTheme::ForestMist
                | AppTheme::Lava
                | AppTheme::MonoFade
                | AppTheme::CottonCandy
                | AppTheme::Custom
        )
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Weather {
    /// Off by default: nothing touches the network until this is switched on.
    pub enabled: bool,
    /// The place the coordinates belong to, as the Settings window's city search named it
    /// ("Prague, Czechia"); empty when the coordinates were typed in. Shown only in Settings.
    pub place: String,
    pub latitude: f32,
    pub longitude: f32,
    pub fahrenheit: bool,
}

impl Default for Weather {
    fn default() -> Self {
        Self {
            enabled: false,
            place: String::new(),
            latitude: 0.0,
            longitude: 0.0,
            fahrenheit: false,
        }
    }
}

/// Which apps' playback the music card follows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceMode {
    /// Every app that reports playback to Windows.
    #[default]
    All,
    /// Only the apps in the list.
    Only,
    /// Every app except the ones in the list.
    Except,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProgressStyle {
    #[default]
    Line,
    Bar,
    Dots,
    Wave,
    Segments,
}

impl ProgressStyle {
    pub const ALL: [ProgressStyle; 5] = [
        ProgressStyle::Line,
        ProgressStyle::Bar,
        ProgressStyle::Dots,
        ProgressStyle::Wave,
        ProgressStyle::Segments,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ProgressStyle::Line => "Thin line",
            ProgressStyle::Bar => "Thick bar",
            ProgressStyle::Dots => "Dots",
            ProgressStyle::Wave => "Wave",
            ProgressStyle::Segments => "Segments",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Music {
    pub enabled: bool,
    pub source_mode: SourceMode,
    /// App names matched anywhere in the media session's app id, e.g. "spotify".
    pub source_apps: Vec<String>,
    /// Show the playing song (cover, title, bars) on the closed pill. Off = the closed pill
    /// keeps its own face while music plays; the Music tab is unchanged.
    pub on_pill: bool,
    /// Show the song title on the resting pill while music plays.
    pub title_in_pill: bool,
    /// Colour the resting pill with the album cover's colours while music plays.
    pub pill_colors: bool,
    /// The Music tab's background glows in the cover's colours and moves with the music (its
    /// bass, middle and treble) and with each new lyric line. Uses the audio capture.
    pub reactive_background: bool,
    /// Use the blurred album cover as the music card's background.
    pub art_background: bool,
    pub progress_style: ProgressStyle,
    /// Off by default: looking up lyrics sends the song's title, artist, album and length
    /// to lrclib.net.
    pub lyrics: bool,
}

impl Default for Music {
    fn default() -> Self {
        Self {
            enabled: true,
            source_mode: SourceMode::All,
            source_apps: Vec::new(),
            on_pill: true,
            title_in_pill: true,
            pill_colors: true,
            reactive_background: true,
            art_background: true,
            progress_style: ProgressStyle::Line,
            lyrics: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Popups {
    /// Show a short note on Pillbox when Caps, Num or Scroll Lock changes.
    pub lock_keys: bool,
}

impl Default for Popups {
    fn default() -> Self {
        Self { lock_keys: true }
    }
}

/// One launcher slot: an app, file or folder to open, with an optional global shortcut.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct LaunchSlot {
    pub name: String,
    /// Executable, document, folder or URL.
    pub path: String,
    pub args: String,
    /// Global shortcut text, or empty.
    pub hotkey: String,
}

pub const LAUNCH_SLOTS: usize = 6;

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Launcher {
    /// Up to six favourites, shown on the Controls tab.
    pub slots: Vec<LaunchSlot>,
}

/// Microphone and camera: call timer, sensor dot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Calls {
    /// Show a timer on the pill while any app uses the microphone.
    pub call_timer: bool,
    /// While music plays on the closed pill, the song stays and the call timer waits (an app
    /// that keeps the microphone open, such as a Discord voice chat, then never hides the
    /// song). The dot still shows the microphone is in use.
    pub music_first: bool,
    /// A small dot on the pill while the microphone or camera is in use.
    pub sensor_dot: bool,
    /// Executable or app names whose mic/camera use is not shown (matched anywhere in the name).
    pub ignore_apps: Vec<String>,
}

impl Default for Calls {
    fn default() -> Self {
        Self {
            call_timer: true,
            music_first: true,
            sensor_dot: true,
            ignore_apps: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Snippet {
    pub name: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ClipboardSettings {
    /// Record clipboard history. Off = the clipboard is never read.
    pub enabled: bool,
    /// Unpinned items beyond this are dropped, oldest first.
    pub max_items: u32,
    /// Keep pictures (they take the most space).
    pub pictures: bool,
    /// Show the list in two columns.
    pub two_columns: bool,
    /// Saved text to paste often.
    pub snippets: Vec<Snippet>,
    /// Executable names whose copies are never recorded (e.g. a password manager that does
    /// not mark its copies as private).
    pub ignore_apps: Vec<String>,
}

impl Default for ClipboardSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            max_items: 200,
            pictures: true,
            two_columns: false,
            snippets: Vec::new(),
            ignore_apps: Vec::new(),
        }
    }
}

/// Unpinned clipboard items older than this many days are deleted (pinned items and snippets
/// stay). Fixed: copies often hold private text, so they never pile up.
pub const CLIP_KEEP_DAYS: u32 = 2;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ShelfSettings {
    /// Put new screenshots on the shelf.
    pub captures: bool,
}

impl Default for ShelfSettings {
    fn default() -> Self {
        Self { captures: true }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RemindersSettings {
    /// Play a chime until the reminder is answered.
    pub chime: bool,
    pub snooze_minutes: u32,
}

impl Default for RemindersSettings {
    fn default() -> Self {
        Self {
            chime: true,
            snooze_minutes: 10,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WidgetKind {
    Clock,
    WorldClock,
    Weather,
    Todo,
    Photos,
    Battery,
    PcStats,
    FocusTimer,
    NextEvents,
    Ping,
    QuickNote,
    Countdown,
    Currency,
    Calculator,
    WebsiteCheck,
    AirQuality,
    SunMoon,
    ScreenTime,
    DayProgress,
    YearProgress,
    DayClock,
    PlayTime,
    /// Kept only so that settings files that list it still load: the System card is now its own
    /// window (`widgets.system_window`), no longer a tile on the Home tab. Dropped on load.
    System,
}

impl WidgetKind {
    pub const ALL: [WidgetKind; 22] = [
        WidgetKind::Clock,
        WidgetKind::DayClock,
        WidgetKind::WorldClock,
        WidgetKind::Weather,
        WidgetKind::AirQuality,
        WidgetKind::SunMoon,
        WidgetKind::NextEvents,
        WidgetKind::Todo,
        WidgetKind::QuickNote,
        WidgetKind::Calculator,
        WidgetKind::FocusTimer,
        WidgetKind::Countdown,
        WidgetKind::DayProgress,
        WidgetKind::YearProgress,
        WidgetKind::PcStats,
        WidgetKind::Battery,
        WidgetKind::ScreenTime,
        WidgetKind::PlayTime,
        WidgetKind::Ping,
        WidgetKind::WebsiteCheck,
        WidgetKind::Currency,
        WidgetKind::Photos,
    ];

    pub fn label(self) -> &'static str {
        match self {
            WidgetKind::Clock => "Clock",
            WidgetKind::WorldClock => "World clock",
            WidgetKind::Weather => "Weather",
            WidgetKind::Todo => "To-do",
            WidgetKind::Photos => "Photos",
            WidgetKind::Battery => "Battery",
            WidgetKind::PcStats => "PC stats",
            WidgetKind::FocusTimer => "Focus timer",
            WidgetKind::NextEvents => "Next reminders",
            WidgetKind::Ping => "Ping",
            WidgetKind::QuickNote => "Quick note",
            WidgetKind::Countdown => "Countdown",
            WidgetKind::Currency => "Currency rates",
            WidgetKind::Calculator => "Calculator",
            WidgetKind::WebsiteCheck => "Website check",
            WidgetKind::AirQuality => "Air quality",
            WidgetKind::SunMoon => "Sun and moon",
            WidgetKind::ScreenTime => "Screen time",
            WidgetKind::DayProgress => "Day progress",
            WidgetKind::YearProgress => "Year progress",
            WidgetKind::DayClock => "Day clock",
            WidgetKind::PlayTime => "Play time",
            WidgetKind::System => "System",
        }
    }

    /// Widgets that need the network, and so also `widgets.online`.
    pub fn online(self) -> bool {
        matches!(
            self,
            WidgetKind::Ping
                | WidgetKind::Currency
                | WidgetKind::WebsiteCheck
                | WidgetKind::AirQuality
                | WidgetKind::Weather
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WeatherLook {
    #[default]
    Minimal,
    Detailed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Widgets {
    /// Widgets on the Home tab, in order.
    pub shown: Vec<WidgetKind>,
    /// Allow the widgets that use the network (ping, currency, website check, air quality).
    /// Weather has its own switch in the weather section.
    pub online: bool,
    /// IANA or Windows time-zone names, e.g. "Tokyo Standard Time" or "Asia/Tokyo".
    pub world_clocks: Vec<String>,
    pub weather_look: WeatherLook,
    /// `#RRGGBB` for the clock widgets.
    pub clock_color: String,
    /// Network speed in megabits per second instead of megabytes.
    pub network_mbps: bool,
    pub ping_host: String,
    pub websites: Vec<String>,
    pub currency_base: String,
    pub currencies: Vec<String>,
    pub countdown_name: String,
    /// `YYYY-MM-DD`.
    pub countdown_date: String,
    /// Folder for the photo widget; empty = Pictures.
    pub photos_folder: String,
    pub focus_minutes: u32,
    /// The System window (off by default; the System tab shows the same cards inside the
    /// Pillbox): a desktop window with a card for each of CPU, GPU, RAM, disk,
    /// network and (on a laptop) battery. Its width can be dragged; it is closed with its ✕ or a
    /// right-click, and turned on again from the tray menu or here.
    pub system_window: bool,
    /// How often the System tab and the System window read the PC's numbers, in milliseconds
    /// (250 to 5000). Only while one of them is on screen; the Home stats strip stays at 2 s.
    pub system_refresh_ms: u32,
}

impl Default for Widgets {
    fn default() -> Self {
        Self {
            shown: vec![
                WidgetKind::Clock,
                WidgetKind::NextEvents,
                WidgetKind::PcStats,
                WidgetKind::FocusTimer,
                WidgetKind::DayProgress,
                WidgetKind::QuickNote,
            ],
            online: false,
            world_clocks: vec!["UTC".into()],
            weather_look: WeatherLook::Minimal,
            clock_color: "#FFFFFF".into(),
            network_mbps: true,
            ping_host: "1.1.1.1".into(),
            websites: Vec::new(),
            currency_base: "EUR".into(),
            currencies: vec!["USD".into(), "GBP".into()],
            countdown_name: String::new(),
            countdown_date: String::new(),
            photos_folder: String::new(),
            focus_minutes: 25,
            system_window: false,
            system_refresh_ms: 500,
        }
    }
}

/// Where the chat drawer's chat comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatSource {
    #[default]
    Twitch,
    /// YouTube live chat; the channel field holds the live video's id.
    #[serde(rename = "youtube")]
    YouTube,
}

/// The chat drawer: a stream's chat page in a panel below Pillbox (WebView2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Drawers {
    /// Off by default: no page is loaded and the network is not touched.
    pub chat_enabled: bool,
    pub chat_source: ChatSource,
    /// Twitch channel name, or the YouTube live video id.
    pub chat_channel: String,
    pub width: f32,
    pub height: f32,
    /// An open drawer stays open when a fullscreen game comes to the front.
    pub over_games: bool,
}

impl Default for Drawers {
    fn default() -> Self {
        Self {
            chat_enabled: false,
            chat_source: ChatSource::Twitch,
            chat_channel: String::new(),
            width: 380.0,
            height: 560.0,
            over_games: true,
        }
    }
}

impl Drawers {
    /// The page the chat drawer shows, if a channel is set.
    pub fn chat_url(&self) -> Option<String> {
        let channel = self.chat_channel.trim();
        if channel.is_empty()
            || !channel
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            return None;
        }
        Some(match self.chat_source {
            ChatSource::Twitch => format!(
                "https://www.twitch.tv/popout/{}/chat?popout=",
                channel.to_ascii_lowercase()
            ),
            ChatSource::YouTube => {
                format!("https://www.youtube.com/live_chat?v={channel}&is_popout=1")
            }
        })
    }
}

/// What Pillbox does while a fullscreen game is in front.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GameBehavior {
    /// Shrink to a slim strip of live stats.
    #[default]
    Strip,
    /// Remove Pillbox completely.
    Hide,
}

/// How the stats strip is drawn over a game.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StripBackground {
    /// Pillbox's own surface (glass, black, a soft theme...).
    #[default]
    #[serde(alias = "island")]
    Surface,
    /// No background: just the numbers, with a shadow so they stay readable.
    NumbersOnly,
}

/// Fullscreen games and the stats strip.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Games {
    /// React to fullscreen apps at all.
    pub enabled: bool,
    pub behavior: GameBehavior,
    /// Treat every fullscreen app as a game, including browsers and video players.
    pub every_fullscreen_app: bool,
    pub strip_background: StripBackground,
    /// Small usage bars under CPU and RAM.
    pub usage_bars: bool,
    /// Executables never treated as games (in addition to the built-in list).
    pub ignore_apps: Vec<String>,
}

impl Default for Games {
    fn default() -> Self {
        Self {
            enabled: true,
            behavior: GameBehavior::Strip,
            every_fullscreen_app: false,
            strip_background: StripBackground::Surface,
            usage_bars: true,
            ignore_apps: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AppRule {
    /// Executable file name, e.g. `game.exe`. Compared without regard to case.
    pub exe: String,
    pub behavior: RuleBehavior,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub layout: Layout,
    pub behavior: Behavior,
    pub appearance: Appearance,
    pub weather: Weather,
    pub music: Music,
    pub popups: Popups,
    pub hotkeys: Hotkeys,
    pub calls: Calls,
    pub clipboard: ClipboardSettings,
    pub shelf: ShelfSettings,
    pub reminders: RemindersSettings,
    pub launcher: Launcher,
    pub widgets: Widgets,
    pub games: Games,
    pub drawers: Drawers,
    pub phone: Phone,
    pub script: Script,
    pub notifications: Notifications,
    pub app_rules: Vec<AppRule>,
}

/// M7: the local script API (`pillboxctl`), flashes, statuses and coding agents.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Script {
    /// Off = no server is started, no Activity tab.
    pub enabled: bool,
    /// How long a flash stays on the closed pill, seconds, 1..60.
    pub flash_seconds: u32,
    /// Finished agents and statuses stay on the Activity tab this many minutes, 1..1440.
    pub keep_minutes: u32,
    /// A "working" agent that says nothing for this many minutes shows as idle, 2..240.
    pub stale_minutes: u32,
    /// Flash "<agent> finished" and "<agent> needs you" on the closed pill.
    pub agent_flashes: bool,
    /// Let Pillbox answer an agent's permission question (Allow / Deny buttons). Needs the
    /// reply hook from `pillboxctl hooks claude --reply`.
    pub answer_permissions: bool,
    /// How long a permission question waits for a click before the agent asks in its own
    /// window, seconds, 5..600.
    pub answer_seconds: u32,
    /// Agents to hide by name (lower case).
    pub hidden_agents: Vec<String>,
    /// Ask Anthropic for the Claude account's usage limits (off by default): reads the sign-in
    /// token Claude Code keeps in `~\.claude\.credentials.json` and sends it to
    /// api.anthropic.com, once a minute while the Activity tab is open and Claude runs.
    pub claude_account_usage: bool,
}

impl Default for Script {
    fn default() -> Self {
        Self {
            enabled: true,
            flash_seconds: 5,
            keep_minutes: 60,
            stale_minutes: 20,
            agent_flashes: true,
            answer_permissions: false,
            answer_seconds: 60,
            hidden_agents: Vec::new(),
            claude_account_usage: false,
        }
    }
}

/// M8: mirror Windows' toast notifications on Pillbox.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Notifications {
    /// Off by default: turning it on lets Pillbox read the text of every notification
    /// Windows shows (nothing leaves the PC). Off = nothing is read and there is no tab.
    pub enabled: bool,
    /// How long new notifications stay stacked on the closed pill, seconds, 1..60. Turning the
    /// wheel over the stack holds it for this long again.
    pub stack_seconds: u32,
    /// Rules for single apps, matched by the app's name or its app id (see `NoteRule`).
    pub rules: Vec<NoteRule>,
}

impl Default for Notifications {
    fn default() -> Self {
        Self {
            enabled: false,
            stack_seconds: 6,
            rules: Vec::new(),
        }
    }
}

/// What to do with the notifications of one app.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoteMode {
    /// Not listed and not stacked: as if the app posted none.
    #[default]
    Hide,
    /// Listed on the Notifications tab, but never stacked on the closed pill.
    ListOnly,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct NoteRule {
    /// The app's name as shown on its notifications ("Claude") or its app id
    /// ("Claude_pzs8sxrjxfjjc!Claude"). Compared without regard to case.
    pub app: String,
    pub mode: NoteMode,
}

/// M6: sending shelf files to a phone and receiving from it over the local network.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Phone {
    /// Off = no phone buttons and no server is ever started.
    pub enabled: bool,
    /// Where files from the phone are saved; "" = `Downloads\Pillbox`.
    pub receive_folder: String,
    /// Files from the phone go on the shelf.
    pub received_to_shelf: bool,
    /// The QR code (and the server) closes after this many minutes, 1..30.
    pub code_minutes: u32,
}

/// Largest upload the phone may send in one go, MB (a safety cap; was `phone.max_upload_mb`).
pub const PHONE_MAX_UPLOAD_MB: u64 = 4096;

impl Default for Phone {
    fn default() -> Self {
        Self {
            enabled: true,
            receive_folder: String::new(),
            received_to_shelf: true,
            code_minutes: 3,
        }
    }
}

/// Trim, lower-case and de-duplicate a list of app names; drop empty entries.
fn tidy_names(names: &mut Vec<String>) {
    for n in names.iter_mut() {
        *n = n.trim().to_lowercase();
    }
    names.retain(|n| !n.is_empty());
    let mut seen = Vec::new();
    names.retain(|n| {
        let new = !seen.contains(n);
        seen.push(n.clone());
        new
    });
}

/// Collect every non-object value in `value` with its path.
fn leaves(value: &Value, path: &mut Vec<String>, out: &mut Vec<(Vec<String>, Value)>) {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                path.push(key.clone());
                leaves(child, path, out);
                path.pop();
            }
        }
        other => out.push((path.clone(), other.clone())),
    }
}

/// Replace the value at `path` if that path exists in `root`. Returns the old value.
fn replace_at(root: &mut Value, path: &[String], new: Value) -> Option<Value> {
    let (last, parents) = path.split_last()?;
    let mut node = root;
    for key in parents {
        node = node.get_mut(key)?;
    }
    let slot = node.as_object_mut()?.get_mut(last)?;
    Some(std::mem::replace(slot, new))
}

/// Settings that older versions wrote and this one no longer has. They are dropped without a
/// warning (a file from an older version is not a mistake); `layout.height` first becomes
/// `layout.size` (see `migrate`).
pub const RETIRED: [&str; 9] = [
    "clipboard.expire_days",
    "layout.width",
    "layout.height",
    "layout.roundness",
    "layout.open_width",
    "layout.open_height",
    "layout.tall_height",
    "behavior.fps",
    "phone.max_upload_mb",
];

/// The pill height `layout.size` = 1 stands for, in DIPs (the old `layout.height` default).
const BASE_PILL_HEIGHT: f32 = 36.0;

/// Carry an older file's choices over to the settings that replaced them: a custom
/// `layout.height` becomes the `layout.size` that gives the same pill height (rounded to 5%),
/// unless the file already has a size.
fn migrate(user: &mut Value) {
    // The app was called Island: the shortcut that opens the card had another key name.
    if let Some(hotkeys) = user.get_mut("hotkeys").and_then(Value::as_object_mut)
        && let Some(old) = hotkeys.remove("toggle_island")
    {
        hotkeys.entry("toggle_pillbox").or_insert(old);
    }
    let Some(layout) = user.get_mut("layout").and_then(Value::as_object_mut) else {
        return;
    };
    if layout.contains_key("size") {
        return;
    }
    let Some(height) = layout.get("height").and_then(Value::as_f64) else {
        return;
    };
    let size = ((height as f32 / BASE_PILL_HEIGHT) * 20.0).round() / 20.0;
    if size.is_finite() && (size - 1.0).abs() > 1e-3 {
        layout.insert(
            "size".into(),
            Value::from(f64::from(size.clamp(SIZE_MIN, SIZE_MAX))),
        );
    }
}

impl Settings {
    /// Whether the closed pill shows the playing song (`music_playing`: a track is playing).
    pub fn song_on_pill(&self, music_playing: bool) -> bool {
        self.music.enabled && self.music.on_pill && music_playing
    }

    /// Whether the closed pill shows the call timer while an app uses the microphone
    /// (`in_call`). With `calls.music_first`, a song on the pill keeps its place.
    pub fn call_on_pill(&self, in_call: bool, music_playing: bool) -> bool {
        in_call
            && self.calls.call_timer
            && !(self.calls.music_first && self.song_on_pill(music_playing))
    }

    /// Parse a settings file. `Err` only when the text is not JSON at all; otherwise every
    /// usable value is applied over the defaults and the problems are listed as warnings.
    pub fn from_json(text: &str) -> Result<(Settings, Vec<String>), serde_json::Error> {
        let mut user: Value = serde_json::from_str(text)?;
        let mut warnings = Vec::new();
        let mut root = serde_json::to_value(Settings::default())?;
        if !user.is_object() {
            warnings.push("settings file is not a JSON object; using defaults".to_string());
            return Ok((Settings::default(), warnings));
        }
        migrate(&mut user);
        let mut found = Vec::new();
        leaves(&user, &mut Vec::new(), &mut found);
        for (path, value) in found {
            let name = path.join(".");
            if RETIRED.contains(&name.as_str()) {
                continue;
            }
            match replace_at(&mut root, &path, value) {
                None => warnings.push(format!("unknown setting `{name}` ignored")),
                Some(old) => {
                    // Keep the value only if the whole document still parses with it.
                    if serde_json::from_value::<Settings>(root.clone()).is_err() {
                        replace_at(&mut root, &path, old);
                        warnings.push(format!("invalid value for `{name}`; keeping the default"));
                    }
                }
            }
        }
        let mut settings: Settings = serde_json::from_value(root)?;
        settings.sanitize(&mut warnings);
        Ok((settings, warnings))
    }

    pub fn to_json(&self) -> String {
        // Serialising plain structs of numbers, strings and enums cannot fail.
        serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".to_string())
    }

    /// Clamp everything into its supported range.
    pub fn sanitize(&mut self, warnings: &mut Vec<String>) {
        fn clamp(
            name: &str,
            v: &mut f32,
            lo: f32,
            hi: f32,
            default: f32,
            warnings: &mut Vec<String>,
        ) {
            if !v.is_finite() {
                warnings.push(format!("`{name}` is not a number; using {default}"));
                *v = default;
            } else if *v < lo || *v > hi {
                warnings.push(format!("`{name}` = {v} is outside {lo}..{hi}; clamped"));
                *v = v.clamp(lo, hi);
            }
        }
        let d = Settings::default();
        let l = &mut self.layout;
        clamp(
            "layout.distance",
            &mut l.distance,
            0.0,
            300.0,
            d.layout.distance,
            warnings,
        );
        clamp(
            "layout.size",
            &mut l.size,
            SIZE_MIN,
            SIZE_MAX,
            d.layout.size,
            warnings,
        );
        if l.monitor.trim().is_empty() {
            l.monitor = "primary".into();
        }
        let default_keys = d.hotkeys.named();
        for ((name, value), (_, default)) in self.hotkeys.named_mut().into_iter().zip(default_keys)
        {
            match crate::hotkey::normalize(value) {
                Ok(canonical) => *value = canonical,
                Err(e) => {
                    warnings.push(format!("`hotkeys.{name}`: {e}; using {default}"));
                    *value = default.to_string();
                }
            }
        }
        let b = &mut self.behavior;
        clamp(
            "behavior.animation_speed",
            &mut b.animation_speed,
            0.5,
            2.0,
            d.behavior.animation_speed,
            warnings,
        );
        b.open_delay_ms = b.open_delay_ms.min(3000);
        b.close_delay_ms = b.close_delay_ms.min(3000);
        let a = &mut self.appearance;
        clamp(
            "appearance.glass_blur",
            &mut a.glass_blur,
            0.0,
            1.0,
            d.appearance.glass_blur,
            warnings,
        );
        if crate::theme::parse_hex(&a.accent).is_none() {
            warnings.push(format!(
                "`appearance.accent` = {:?} is not #RRGGBB; using the default",
                a.accent
            ));
            a.accent = d.appearance.accent;
        }
        a.fade.sanitize(warnings);
        let w = &mut self.weather;
        clamp(
            "weather.latitude",
            &mut w.latitude,
            -90.0,
            90.0,
            0.0,
            warnings,
        );
        clamp(
            "weather.longitude",
            &mut w.longitude,
            -180.0,
            180.0,
            0.0,
            warnings,
        );
        for app in &mut self.music.source_apps {
            *app = app.trim().to_lowercase();
        }
        self.music.source_apps.retain(|a| !a.is_empty());
        self.music.source_apps.dedup();
        for rule in &mut self.app_rules {
            rule.exe = rule.exe.trim().to_lowercase();
        }
        self.app_rules.retain(|r| !r.exe.is_empty());

        let slots = &mut self.launcher.slots;
        slots.retain(|s| !s.path.trim().is_empty());
        if slots.len() > LAUNCH_SLOTS {
            warnings.push(format!("only {LAUNCH_SLOTS} launcher slots are used"));
            slots.truncate(LAUNCH_SLOTS);
        }
        for (i, slot) in slots.iter_mut().enumerate() {
            slot.path = slot.path.trim().to_string();
            match crate::hotkey::normalize(&slot.hotkey) {
                Ok(h) => slot.hotkey = h,
                Err(e) => {
                    warnings.push(format!("`launcher.slots[{i}].hotkey`: {e}; removed"));
                    slot.hotkey.clear();
                }
            }
        }
        tidy_names(&mut self.calls.ignore_apps);
        tidy_names(&mut self.clipboard.ignore_apps);
        tidy_names(&mut self.games.ignore_apps);
        let dr = &mut self.drawers;
        clamp(
            "drawers.width",
            &mut dr.width,
            260.0,
            900.0,
            d.drawers.width,
            warnings,
        );
        clamp(
            "drawers.height",
            &mut dr.height,
            200.0,
            1200.0,
            d.drawers.height,
            warnings,
        );
        dr.chat_channel = dr.chat_channel.trim().to_string();
        let ph = &mut self.phone;
        ph.code_minutes = ph.code_minutes.clamp(1, 30);
        ph.receive_folder = ph.receive_folder.trim().to_string();
        let sc = &mut self.script;
        sc.flash_seconds = sc.flash_seconds.clamp(1, 60);
        sc.keep_minutes = sc.keep_minutes.clamp(1, 1440);
        sc.stale_minutes = sc.stale_minutes.clamp(2, 240);
        sc.answer_seconds = sc.answer_seconds.clamp(5, 600);
        tidy_names(&mut sc.hidden_agents);
        let nt = &mut self.notifications;
        nt.stack_seconds = nt.stack_seconds.clamp(1, 60);
        for r in &mut nt.rules {
            r.app = r.app.trim().to_string();
        }
        nt.rules.retain(|r| !r.app.is_empty());
        let mut seen_apps: Vec<String> = Vec::new();
        nt.rules.retain(|r| {
            let key = r.app.to_lowercase();
            let new = !seen_apps.contains(&key);
            seen_apps.push(key);
            new
        });
        let c = &mut self.clipboard;
        c.max_items = c.max_items.clamp(10, 5000);
        self.weather.place = self.weather.place.trim().to_string();
        self.layout.reveal.song_seconds = self.layout.reveal.song_seconds.clamp(2, 30);
        let ck = &mut self.appearance.clock;
        clamp(
            "appearance.clock.size",
            &mut ck.size,
            CLOCK_SIZE_MIN,
            CLOCK_SIZE_MAX,
            1.0,
            warnings,
        );
        if crate::theme::parse_hex(&ck.custom_color).is_none() {
            warnings.push(format!(
                "`appearance.clock.custom_color` = {:?} is not #RRGGBB; using white",
                ck.custom_color
            ));
            ck.custom_color = "#FFFFFF".into();
        }
        c.snippets.retain(|s| !s.text.is_empty());
        let r = &mut self.reminders;
        r.snooze_minutes = r.snooze_minutes.clamp(1, 240);
        let w = &mut self.widgets;
        let mut seen = Vec::new();
        w.shown.retain(|k| {
            let new = !seen.contains(k);
            seen.push(*k);
            new
        });
        w.focus_minutes = w.focus_minutes.clamp(1, 180);
        w.system_refresh_ms = w.system_refresh_ms.clamp(250, 5000);
        // The old Home tile moved into its own window.
        w.shown.retain(|k| *k != WidgetKind::System);
        if crate::theme::parse_hex(&w.clock_color).is_none() {
            warnings.push(format!(
                "`widgets.clock_color` = {:?} is not #RRGGBB; using white",
                w.clock_color
            ));
            w.clock_color = d.widgets.clock_color.clone();
        }
        w.currency_base = w.currency_base.trim().to_uppercase();
        for code in &mut w.currencies {
            *code = code.trim().to_uppercase();
        }
        w.currencies
            .retain(|c| c.len() == 3 && c.chars().all(|ch| ch.is_ascii_uppercase()));
        w.world_clocks.retain(|z| !z.trim().is_empty());
        w.websites.retain(|u| !u.trim().is_empty());
        if !w.countdown_date.is_empty() && crate::calendar::parse_date(&w.countdown_date).is_none()
        {
            warnings.push(format!(
                "`widgets.countdown_date` = {:?} is not YYYY-MM-DD; ignored",
                w.countdown_date
            ));
            w.countdown_date.clear();
        }
    }

    /// The rule for an executable name, if any.
    pub fn rule_for(&self, exe: &str) -> Option<RuleBehavior> {
        self.app_rules
            .iter()
            .find(|r| r.exe.eq_ignore_ascii_case(exe))
            .map(|r| r.behavior)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn music_keeps_the_pill_during_a_call_unless_told_otherwise() {
        let mut s = Settings::default();
        // Defaults: the song stays, the call timer waits.
        assert!(s.song_on_pill(true));
        assert!(!s.call_on_pill(true, true));
        // No music: the call shows.
        assert!(s.call_on_pill(true, false));
        assert!(!s.call_on_pill(false, false));
        // Calls first: the call takes the pill even with music.
        s.calls.music_first = false;
        assert!(s.call_on_pill(true, true));
        // Song off the pill: nothing to make way for, so the call shows.
        s.calls.music_first = true;
        s.music.on_pill = false;
        assert!(!s.song_on_pill(true));
        assert!(s.call_on_pill(true, true));
        // Call timer off: never.
        s.calls.call_timer = false;
        assert!(!s.call_on_pill(true, false));
    }

    #[test]
    fn old_files_get_the_new_pill_switches_on() {
        let (s, w) =
            parse(r#"{ "calls": { "call_timer": true }, "music": { "title_in_pill": false } }"#);
        assert!(w.is_empty(), "{w:?}");
        assert!(s.calls.music_first);
        assert!(s.music.on_pill);
        assert!(!s.music.title_in_pill);
        assert!(s.behavior.start_greeting);
    }

    fn parse(text: &str) -> (Settings, Vec<String>) {
        match Settings::from_json(text) {
            Ok(r) => r,
            Err(e) => panic!("should parse: {e}"),
        }
    }

    #[test]
    fn empty_object_gives_defaults_without_warnings() {
        let (s, w) = parse("{}");
        assert_eq!(s, Settings::default());
        assert!(w.is_empty(), "{w:?}");
    }

    #[test]
    fn defaults_round_trip() {
        let (s, w) = parse(&Settings::default().to_json());
        assert_eq!(s, Settings::default());
        assert!(w.is_empty(), "{w:?}");
    }

    #[test]
    fn partial_file_overrides_only_what_it_names() {
        let (s, w) = parse(
            r##"{ "layout": { "side": "left", "distance": 20 },
                  "appearance": { "surface": "liquid_glass", "accent": "#ff8800" } }"##,
        );
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(s.layout.side, Side::Left);
        assert_eq!(s.layout.distance, 20.0);
        assert_eq!(s.layout.size, 1.0);
        assert_eq!(s.appearance.surface, Surface::LiquidGlass);
        assert_eq!(s.appearance.accent, "#ff8800");
        assert_eq!(s.behavior, Behavior::default());
    }

    #[test]
    fn one_bad_value_does_not_spoil_the_rest() {
        let (s, w) = parse(
            r#"{ "layout": { "size": "big", "distance": 12, "side": "bottom" },
                 "behavior": { "clock_24h": false } }"#,
        );
        assert_eq!(s.layout.size, 1.0, "bad type keeps default");
        assert_eq!(s.layout.side, Side::Top, "unknown enum value keeps default");
        assert_eq!(s.layout.distance, 12.0);
        assert!(!s.behavior.clock_24h);
        assert_eq!(w.len(), 2, "{w:?}");
    }

    #[test]
    fn unknown_keys_are_ignored_with_a_warning() {
        let (s, w) = parse(r#"{ "layout": { "colour": 3 }, "future_section": { "x": 1 } }"#);
        assert_eq!(s, Settings::default());
        assert_eq!(w.len(), 2, "{w:?}");
    }

    #[test]
    fn numbers_are_clamped() {
        let (s, w) = parse(
            r#"{ "layout": { "size": 5, "distance": -4 },
                 "behavior": { "animation_speed": 100, "open_delay_ms": 999999 },
                 "appearance": { "glass_blur": 2 } }"#,
        );
        assert_eq!(s.layout.size, SIZE_MAX);
        assert_eq!(s.layout.distance, 0.0);
        assert_eq!(s.behavior.animation_speed, 2.0);
        assert_eq!(s.behavior.open_delay_ms, 3000);
        assert_eq!(s.appearance.glass_blur, 1.0);
        assert!(w.len() >= 4, "{w:?}");
        let (s, _) =
            parse(r#"{ "layout": { "size": 0.1 }, "behavior": { "animation_speed": 0.25 } }"#);
        assert_eq!(s.layout.size, SIZE_MIN);
        assert_eq!(s.behavior.animation_speed, 0.5);
    }

    #[test]
    fn an_older_file_loads_quietly_and_keeps_its_choices() {
        // Everything an older version wrote, including the settings that are gone now.
        let (s, w) = parse(
            r#"{ "layout": { "style": "notch", "width": 260, "height": 36, "roundness": 0.5,
                             "open_width": 500, "open_height": 200, "tall_height": 400 },
                 "behavior": { "fps": 90, "clock_24h": false },
                 "phone": { "max_upload_mb": 100, "code_minutes": 5 } }"#,
        );
        assert!(w.is_empty(), "retired settings are not warned about: {w:?}");
        assert_eq!(s.layout.style, Style::Notch);
        assert_eq!(s.layout.size, 1.0, "the default height stays at 100%");
        assert!(!s.behavior.clock_24h);
        assert_eq!(s.phone.code_minutes, 5);
        // A custom height becomes the size that gives the same pill, rounded to 5%.
        for (height, size) in [
            (45.0, 1.25),
            (30.0, 0.85),
            (40.0, 1.1),
            (90.0, SIZE_MAX),
            (20.0, SIZE_MIN),
        ] {
            let (s, w) = parse(&format!(r#"{{ "layout": {{ "height": {height} }} }}"#));
            assert!(w.is_empty(), "{w:?}");
            assert!(
                (s.layout.size - size).abs() < 1e-4,
                "height {height}: {}",
                s.layout.size
            );
        }
        // A size already in the file wins over an old height.
        let (s, _) = parse(r#"{ "layout": { "height": 45, "size": 0.9 } }"#);
        assert!((s.layout.size - 0.9).abs() < 1e-6);
        // Saving drops the retired keys for good.
        let saved: Value = match serde_json::from_str(&s.to_json()) {
            Ok(v) => v,
            Err(e) => panic!("saved settings are JSON: {e}"),
        };
        for key in RETIRED {
            let (section, name) = key.split_once('.').unwrap_or((key, ""));
            assert!(saved[section].get(name).is_none(), "{key} still written");
        }
    }

    #[test]
    fn reveal_clock_and_theme_settings() {
        let s = Settings::default();
        // Every reason shows a hidden Pillbox by default, and music plays it in for good.
        let r = &s.layout.reveal;
        assert_eq!(r.music, MusicReveal::WhilePlaying);
        assert!(r.notifications && r.flashes && r.calls && r.reminders && r.notices);
        assert!(s.music.reactive_background);
        assert_eq!(s.appearance.app_theme, AppTheme::Default);
        assert_eq!(s.appearance.clock, ClockLook::default());
        let (s, w) = parse(
            r##"{ "layout": { "hidden_until_hover": true,
                              "reveal": { "music": "on_song_change", "song_seconds": 99,
                                          "notifications": false } },
                  "appearance": { "app_theme": "sunset",
                                  "clock": { "size": 5, "seconds": true, "color": "custom",
                                             "custom_color": "orange" } },
                  "music": { "reactive_background": false } }"##,
        );
        assert_eq!(s.layout.reveal.music, MusicReveal::OnSongChange);
        assert_eq!(s.layout.reveal.song_seconds, 30, "clamped to 2..30");
        assert!(!s.layout.reveal.notifications && s.layout.reveal.calls);
        assert_eq!(s.appearance.app_theme, AppTheme::Sunset);
        let ck = &s.appearance.clock;
        assert_eq!(ck.size, CLOCK_SIZE_MAX, "clamped");
        assert!(ck.seconds && ck.bold && !ck.leading_zero);
        assert_eq!(ck.color, ClockColor::Custom);
        assert_eq!(ck.custom_color, "#FFFFFF", "a bad colour falls back");
        assert!(!s.music.reactive_background);
        assert_eq!(w.len(), 2, "the size and the colour are reported: {w:?}");
        // Unknown names keep the defaults.
        let (s, w) = parse(
            r#"{ "appearance": { "app_theme": "neon" }, "layout": { "reveal": { "music": "always" } } }"#,
        );
        assert_eq!(s.appearance.app_theme, AppTheme::Default);
        assert_eq!(s.layout.reveal.music, MusicReveal::WhilePlaying);
        assert_eq!(w.len(), 2);
    }
    #[test]
    fn fade_settings_load_from_old_and_hand_edited_files() {
        // A file from before fades: the new fields take their defaults, without warnings.
        let (s, w) =
            parse(r##"{ "appearance": { "surface": "graphite", "app_theme": "ocean" } }"##);
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(s.appearance.fade, FadeSettings::default());
        assert!(s.appearance.animate);
        assert_eq!(s.appearance.app_theme, AppTheme::Ocean);
        // The new themes by name.
        for (name, app) in [
            ("aurora", AppTheme::Aurora),
            ("golden_hour", AppTheme::GoldenHour),
            ("midnight_neon", AppTheme::MidnightNeon),
            ("cotton_candy", AppTheme::CottonCandy),
            ("custom", AppTheme::Custom),
        ] {
            let (s, w) = parse(&format!(
                r#"{{ "appearance": {{ "app_theme": "{name}" }} }}"#
            ));
            assert!(w.is_empty(), "{w:?}");
            assert_eq!(s.appearance.app_theme, app);
        }
        // A good custom fade is kept as written, stops sorted by position.
        let (s, w) = parse(
            r##"{ "appearance": { "app_theme": "custom", "animate": false,
                   "fade": { "stops": [ { "color": "#00FF00", "at": 1 },
                                        { "color": "#FF0000", "at": 0 } ],
                             "angle": 450, "softness": 0.25, "opacity": 0.6 } } }"##,
        );
        assert!(w.is_empty(), "{w:?}");
        let f = &s.appearance.fade;
        assert!(!s.appearance.animate);
        assert_eq!(f.stops.len(), 2);
        assert_eq!((f.stops[0].color.as_str(), f.stops[0].at), ("#FF0000", 0.0));
        assert_eq!(f.angle, 90.0, "wrapped into 0..360");
        assert_eq!((f.softness, f.opacity), (0.25, 0.6));
        // Bad values never crash: each is reported and repaired on its own.
        let (s, w) = parse(
            r##"{ "appearance": { "fade": {
                   "stops": [ { "color": "pink", "at": 7 }, { "color": "#123456", "at": -1 },
                              { "color": "#ABCDEF" }, { "color": "#000000" } ],
                   "softness": 9, "opacity": 0.01 } } }"##,
        );
        let f = &s.appearance.fade;
        assert_eq!(
            f.stops.len(),
            FADE_STOPS_MAX,
            "the fourth colour is dropped"
        );
        assert!(
            f.stops
                .iter()
                .all(|s| crate::theme::parse_hex(&s.color).is_some())
        );
        assert!(f.stops.windows(2).all(|p| p[0].at <= p[1].at));
        assert!(f.stops.iter().all(|s| (0.0..=1.0).contains(&s.at)));
        assert_eq!((f.softness, f.opacity), (1.0, FADE_OPACITY_MIN));
        assert!(w.len() >= 2, "{w:?}");
        // Too few colours, or a list that is not a list, fall back to the default fade.
        let (s, w) =
            parse(r##"{ "appearance": { "fade": { "stops": [ { "color": "#FF0000" } ] } } }"##);
        assert_eq!(s.appearance.fade.stops, FadeSettings::default().stops);
        assert_eq!(w.len(), 1, "{w:?}");
        let (s, w) =
            parse(r#"{ "appearance": { "fade": { "stops": "red to blue", "angle": "up" } } }"#);
        assert_eq!(s.appearance.fade, FadeSettings::default());
        assert_eq!(w.len(), 2, "{w:?}");
        // And everything saves and loads back unchanged.
        let mut custom = Settings::default();
        custom.appearance.app_theme = AppTheme::Custom;
        custom.appearance.fade.angle = 135.0;
        let (back, w) = parse(&custom.to_json());
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(back, custom);
    }

    #[test]
    fn new_settings_have_sane_defaults_and_ranges() {
        let s = Settings::default();
        assert_eq!(s.behavior.pill_clock, PillClock::Time);
        assert!(s.music.pill_colors);
        assert!(s.weather.place.is_empty());
        let (s, w) = parse(
            r#"{ "behavior": { "pill_clock": "day_time", "visualizer": "off" },
                 "music": { "pill_colors": false },
                 "weather": { "place": "  Prague, Czechia " } }"#,
        );
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(s.behavior.pill_clock, PillClock::DayTime);
        assert_eq!(s.behavior.visualizer, Visualizer::Off);
        assert!(!s.music.pill_colors);
        assert_eq!(s.weather.place, "Prague, Czechia");
        // The old expiry choice is retired: an old file still loads, quietly.
        let (_, w) = parse(r#"{ "clipboard": { "expire_days": 7 } }"#);
        assert!(w.is_empty(), "{w:?}");
    }

    #[test]
    fn bad_accent_falls_back() {
        let (s, w) = parse(r#"{ "appearance": { "accent": "purple" } }"#);
        assert_eq!(s.appearance.accent, "#7C5CFF");
        assert_eq!(w.len(), 1);
    }

    #[test]
    fn app_rules_parse_and_match_case_insensitively() {
        let (s, w) = parse(
            r#"{ "app_rules": [ { "exe": " Game.EXE ", "behavior": "strip" },
                                { "exe": "", "behavior": "hide" },
                                { "exe": "vlc.exe" } ] }"#,
        );
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(s.app_rules.len(), 2, "empty exe is dropped");
        assert_eq!(s.rule_for("GAME.exe"), Some(RuleBehavior::Strip));
        assert_eq!(s.rule_for("vlc.exe"), Some(RuleBehavior::Hide));
        assert_eq!(s.rule_for("notepad.exe"), None);
    }

    #[test]
    fn invalid_rule_list_is_rejected_as_a_whole() {
        let (s, w) = parse(r#"{ "app_rules": [ { "exe": "a.exe", "behavior": "explode" } ] }"#);
        assert!(s.app_rules.is_empty());
        assert_eq!(w.len(), 1);
    }

    #[test]
    fn not_json_is_an_error_and_non_object_is_defaults() {
        assert!(Settings::from_json("{ nope").is_err());
        let (s, w) = parse("[1, 2]");
        assert_eq!(s, Settings::default());
        assert_eq!(w.len(), 1);
    }

    #[test]
    fn music_and_popup_settings() {
        let (s, w) = parse("{}");
        assert!(w.is_empty());
        assert!(
            s.music.enabled && !s.music.lyrics,
            "lyrics must be off by default"
        );
        assert!(s.popups.lock_keys);
        let (s, w) = parse(
            r#"{ "music": { "source_mode": "only", "source_apps": [" Spotify ", "", "spotify"],
                            "progress_style": "wave", "lyrics": true },
                 "popups": { "lock_keys": false } }"#,
        );
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(s.music.source_mode, SourceMode::Only);
        assert_eq!(s.music.source_apps, vec!["spotify".to_string()]);
        assert_eq!(s.music.progress_style, ProgressStyle::Wave);
        assert!(s.music.lyrics && !s.popups.lock_keys);
        let (s, w) = parse(r#"{ "music": { "progress_style": "zigzag" } }"#);
        assert_eq!(s.music.progress_style, ProgressStyle::Line);
        assert_eq!(w.len(), 1);
    }

    #[test]
    fn drawer_pages() {
        let mut d = Drawers::default();
        assert_eq!(d.chat_url(), None);
        d.chat_channel = " SomeStreamer ".into();
        assert_eq!(
            d.chat_url().as_deref(),
            Some("https://www.twitch.tv/popout/somestreamer/chat?popout=")
        );
        d.chat_source = ChatSource::YouTube;
        d.chat_channel = "dQw4w9WgXcQ".into();
        assert_eq!(
            d.chat_url().as_deref(),
            Some("https://www.youtube.com/live_chat?v=dQw4w9WgXcQ&is_popout=1")
        );
        d.chat_channel = "a/b?c".into();
        assert_eq!(d.chat_url(), None, "only plain names and ids");
        let (s, w) = parse(r#"{ "drawers": { "chat_source": "youtube" } }"#);
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(s.drawers.chat_source, ChatSource::YouTube);
    }

    #[test]
    fn phone_settings_are_clamped() {
        let (s, w) = parse("{}");
        assert!(w.is_empty());
        assert!(s.phone.enabled && s.phone.received_to_shelf);
        assert_eq!(s.phone.code_minutes, 3);
        let (s, _) = parse(r#"{ "phone": { "code_minutes": 0, "receive_folder": " D:\\In " } }"#);
        assert_eq!(s.phone.code_minutes, 1);
        assert_eq!(s.phone.receive_folder, "D:\\In");
    }

    #[test]
    fn script_settings_are_clamped() {
        let (s, w) = parse("{}");
        assert!(w.is_empty());
        assert!(s.script.enabled && !s.script.answer_permissions);
        assert_eq!((s.script.flash_seconds, s.script.keep_minutes), (5, 60));
        let (s, _) = parse(
            r#"{ "script": { "flash_seconds": 0, "keep_minutes": 99999, "stale_minutes": 0,
                              "answer_seconds": 1, "hidden_agents": [" Codex ", "codex", ""] } }"#,
        );
        assert_eq!(s.script.flash_seconds, 1);
        assert_eq!(s.script.keep_minutes, 1440);
        assert_eq!(s.script.stale_minutes, 2);
        assert_eq!(s.script.answer_seconds, 5);
        assert_eq!(s.script.hidden_agents, ["codex"]);
    }

    #[test]
    fn the_system_tile_moved_to_its_own_window() {
        let (s, w) = parse("{}");
        assert!(w.is_empty());
        assert!(!s.widgets.system_window);
        assert!(!s.widgets.shown.contains(&WidgetKind::System));
        // An older settings file that lists the tile still loads; the tile is dropped.
        let (s, w) = parse(r#"{ "widgets": { "shown": ["clock", "system", "battery"] } }"#);
        assert!(w.is_empty());
        assert_eq!(s.widgets.shown, [WidgetKind::Clock, WidgetKind::Battery]);
        assert!(!WidgetKind::ALL.contains(&WidgetKind::System));
        // The System readings refresh twice a second unless told otherwise, within limits.
        assert_eq!(s.widgets.system_refresh_ms, 500);
        let (s, _) = parse(r#"{ "widgets": { "system_refresh_ms": 10 } }"#);
        assert_eq!(s.widgets.system_refresh_ms, 250);
        let (s, _) = parse(r#"{ "widgets": { "system_refresh_ms": 99999 } }"#);
        assert_eq!(s.widgets.system_refresh_ms, 5000);
    }

    #[test]
    fn notification_settings_default_off_and_are_tidied() {
        let (s, w) = parse("{}");
        assert!(w.is_empty());
        assert!(!s.notifications.enabled);
        assert_eq!(s.notifications.stack_seconds, 6);
        assert!(s.notifications.rules.is_empty());
        let (s, _) = parse(
            r#"{ "notifications": { "enabled": true, "stack_seconds": 0, "rules": [
                 { "app": " Claude ", "mode": "list_only" },
                 { "app": "claude", "mode": "hide" },
                 { "app": "  ", "mode": "hide" },
                 { "app": "Mail" } ] } }"#,
        );
        assert!(s.notifications.enabled);
        assert_eq!(s.notifications.stack_seconds, 1);
        // Trimmed, blank and repeated apps dropped (the first rule for an app stays); a rule
        // with no mode hides.
        assert_eq!(
            s.notifications.rules,
            [
                NoteRule {
                    app: "Claude".into(),
                    mode: NoteMode::ListOnly
                },
                NoteRule {
                    app: "Mail".into(),
                    mode: NoteMode::Hide
                },
            ]
        );
        let (s, _) = parse(r#"{ "notifications": { "stack_seconds": 9999 } }"#);
        assert_eq!(s.notifications.stack_seconds, 60);
    }

    #[test]
    fn hotkeys_are_normalised_and_bad_ones_keep_the_default() {
        let (s, w) = parse("{}");
        assert!(w.is_empty());
        assert_eq!(s.hotkeys.toggle_pillbox, "Ctrl+Alt+N");
        assert_eq!(s.hotkeys.mute_mic, "Ctrl+Alt+Shift+M");
        let (s, w) = parse(
            r#"{ "hotkeys": { "toggle_pillbox": "alt + shift + i", "play_pause": "",
                              "screenshot": "Ctrl+Banana" } }"#,
        );
        assert_eq!(s.hotkeys.toggle_pillbox, "Alt+Shift+I");
        assert_eq!(s.hotkeys.play_pause, "", "empty = no shortcut");
        assert_eq!(
            s.hotkeys.screenshot, "Ctrl+Alt+S",
            "bad text keeps the default"
        );
        assert_eq!(w.len(), 1, "{w:?}");
    }

    #[test]
    fn settings_written_when_the_app_was_called_island_still_load() {
        let (s, w) = parse(
            r#"{ "layout": { "style": "island" },
                 "hotkeys": { "toggle_island": "Ctrl+Alt+K" },
                 "games": { "strip_background": "island" } }"#,
        );
        assert_eq!(s.layout.style, Style::Pill);
        assert_eq!(s.hotkeys.toggle_pillbox, "Ctrl+Alt+K");
        assert_eq!(s.games.strip_background, StripBackground::Surface);
        assert!(w.is_empty(), "{w:?}");
        // They are written back under the new names.
        let json = s.to_json();
        assert!(
            json.contains("toggle_pillbox") && !json.contains("toggle_island"),
            "{json}"
        );
        assert!(json.contains("\"pill\""), "{json}");
    }

    #[test]
    fn glass_surfaces_are_flagged() {
        let glass: Vec<_> = Surface::ALL.iter().filter(|s| s.is_glass()).collect();
        assert_eq!(glass.len(), 3);
    }
}
