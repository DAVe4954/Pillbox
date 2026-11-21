//! Live data behind the resting face: battery, CPU load, weather, the pet's blink.

use std::{
    sync::mpsc::{self, Receiver},
    time::{Duration, Instant},
};

use pillbox_core::{
    face::{self, LocalTime, WeatherNow},
    settings::{RestingFace, Settings, WidgetKind},
};

use crate::{
    platform::{
        system::{self, CpuMeter},
        window::Waker,
    },
    scene::Face,
};

/// Weather is fetched only when it is switched on, a location is set, and either the resting
/// face or a Home widget shows it.
pub fn wants_weather(s: &Settings) -> bool {
    s.weather.enabled
        && (s.appearance.resting_face == RestingFace::Weather
            || s.widgets.shown.contains(&WidgetKind::Weather))
}

/// How long a weather reading stays fresh.
const WEATHER_MAX_AGE: Duration = Duration::from_secs(25 * 60);

#[derive(Default)]
pub struct Faces {
    cpu: CpuMeter,
    cpu_percent: Option<f32>,
    weather: Option<WeatherNow>,
    /// Coordinates and time of the last successful or attempted fetch.
    weather_for: Option<(f32, f32, Instant)>,
    weather_rx: Option<Receiver<Option<WeatherNow>>>,
    pub pet_blink: bool,
}

impl Faces {
    pub fn sample_cpu(&mut self) {
        if let Some(p) = self.cpu.sample() {
            self.cpu_percent = Some(p);
        }
    }

    /// Start a weather request if weather is switched on, shown, and the reading is stale.
    /// The network is never touched otherwise.
    pub fn refresh_weather(&mut self, s: &Settings, waker: Waker) {
        if !wants_weather(s) {
            return;
        }
        let (lat, lon) = (s.weather.latitude, s.weather.longitude);
        if lat == 0.0 && lon == 0.0 {
            return;
        }
        let fresh = self
            .weather_for
            .is_some_and(|(la, lo, at)| la == lat && lo == lon && at.elapsed() < WEATHER_MAX_AGE);
        if fresh || self.weather_rx.is_some() {
            return;
        }
        self.weather_for = Some((lat, lon, Instant::now()));
        let (tx, rx) = mpsc::channel();
        self.weather_rx = Some(rx);
        let url = face::open_meteo_url(lat, lon);
        let spawned = std::thread::Builder::new()
            .name("weather".into())
            .spawn(move || {
                let agent = ureq::AgentBuilder::new()
                    .timeout(Duration::from_secs(10))
                    .build();
                let result = agent
                    .get(&url)
                    .call()
                    .ok()
                    .and_then(|r| r.into_string().ok())
                    .and_then(|body| face::parse_open_meteo(&body));
                let _ = tx.send(result);
                waker.wake();
            });
        if let Err(e) = spawned {
            tracing::warn!("could not start the weather request: {e}");
            self.weather_rx = None;
        }
    }

    /// Collect a finished weather request. Returns true if something new arrived.
    pub fn poll(&mut self) -> bool {
        let Some(rx) = &self.weather_rx else {
            return false;
        };
        match rx.try_recv() {
            Ok(result) => {
                self.weather_rx = None;
                match result {
                    Some(w) => {
                        tracing::info!("weather updated: {:.1} °C, code {}", w.temp_c, w.code);
                        self.weather = Some(w);
                    }
                    None => tracing::warn!("weather request failed"),
                }
                true
            }
            Err(mpsc::TryRecvError::Empty) => false,
            Err(mpsc::TryRecvError::Disconnected) => {
                self.weather_rx = None;
                false
            }
        }
    }

    /// Current weather as text ("☀ 21°"), for the weather widget.
    pub fn weather_text(&self, s: &Settings) -> Option<String> {
        self.weather
            .map(|w| face::format_weather(w, s.weather.fahrenheit))
    }

    /// Lines for the detailed weather look; empty until a reading has arrived.
    pub fn weather_details(&self, s: &Settings) -> Vec<String> {
        self.weather
            .map(|w| face::weather_details(w, s.weather.fahrenheit))
            .unwrap_or_default()
    }

    pub fn cpu_percent(&self) -> Option<f32> {
        self.cpu_percent
    }

    pub fn face(&self, s: &Settings, now: LocalTime) -> Face {
        let text = |t: &str| Face::Text(t.to_string());
        match s.appearance.resting_face {
            RestingFace::None => Face::Empty,
            RestingFace::Time => Face::Text(face::format_pill_clock(
                now,
                s.behavior.clock_24h,
                s.behavior.pill_clock,
                s.appearance.clock.seconds,
                s.appearance.clock.leading_zero,
            )),
            RestingFace::Date => Face::Text(face::format_date(now)),
            RestingFace::Battery => match system::battery() {
                Some(b) if b.plugged_in => Face::Text(format!("⚡ {}%", b.percent)),
                Some(b) => Face::Text(format!("{}%", b.percent)),
                None => text("No battery"),
            },
            RestingFace::Cpu => match self.cpu_percent {
                Some(p) => Face::Text(format!("CPU {p:.0}%")),
                None => text("CPU …"),
            },
            RestingFace::Weather => {
                if !s.weather.enabled {
                    text("Weather off")
                } else if s.weather.latitude == 0.0 && s.weather.longitude == 0.0 {
                    text("Set location")
                } else {
                    match self.weather {
                        Some(w) => Face::Text(face::format_weather(w, s.weather.fahrenheit)),
                        None => text("…"),
                    }
                }
            }
            // Reminders and calendar arrive in M4; until then there is nothing to show.
            RestingFace::NextEvent => text("No events"),
            RestingFace::Pet => Face::Pet {
                blink: self.pet_blink,
            },
        }
    }
}
