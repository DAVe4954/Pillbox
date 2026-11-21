//! Synced lyrics from LRCLIB. Off unless enabled in settings. Each song is looked up once:
//! results (including "nothing found") are remembered in memory and in
//! `%APPDATA%\Pillbox\lyrics`, so replaying a song never touches the network again.

use std::{
    collections::HashMap,
    fs,
    path::PathBuf,
    sync::mpsc::{self, Receiver},
    time::Duration,
};

use pillbox_core::{
    lrc::{self, Lyrics},
    music,
};

use crate::{
    platform::{media::Track, window::Waker},
    store,
};

#[derive(Debug, Clone, PartialEq)]
pub enum LyricsState {
    /// Lyrics are switched off, or nothing is playing.
    Off,
    Loading,
    Found(Lyrics),
    NotFound,
    /// Music videos run on a different clock from the album track, so they are skipped.
    SkippedVideo,
}

/// What a lookup produced: lyrics text, a definite "none exist", or a failure to ask.
enum Outcome {
    Lyrics(String),
    Missing,
    Failed,
}

pub struct LyricsService {
    memory: HashMap<String, Option<Lyrics>>,
    /// The song the current state belongs to.
    key: String,
    pending: Option<(String, Receiver<Outcome>)>,
    pub state: LyricsState,
}

impl Default for LyricsService {
    fn default() -> Self {
        Self {
            memory: HashMap::new(),
            key: String::new(),
            pending: None,
            state: LyricsState::Off,
        }
    }
}

fn cache_file(key: &str) -> Option<PathBuf> {
    let dir = store::dir().ok()?.join("lyrics");
    fs::create_dir_all(&dir).ok()?;
    Some(dir.join(format!("{:016x}.lrc", music::stable_hash(key))))
}

fn http_get(url: &str) -> Result<Option<String>, ()> {
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(12))
        .user_agent("Pillbox/0.1 (personal desktop app)")
        .build();
    match agent.get(url).call() {
        Ok(response) => response.into_string().map(Some).map_err(|_| ()),
        // 404 is LRCLIB's "no such track", a real answer rather than a failure.
        Err(ureq::Error::Status(404, _)) => Ok(None),
        Err(_) => Err(()),
    }
}

fn look_up(title: &str, artist: &str, album: &str, duration: f32) -> Outcome {
    let exact = match http_get(&music::lrclib_get_url(title, artist, album, duration)) {
        Ok(body) => body.and_then(|b| music::parse_lrclib_get(&b)),
        Err(()) => return Outcome::Failed,
    };
    if let Some(lrc) = exact {
        return Outcome::Lyrics(lrc);
    }
    // The exact endpoint is strict about album and length; the search is more forgiving.
    match http_get(&music::lrclib_search_url(title, artist)) {
        Ok(body) => body
            .and_then(|b| music::parse_lrclib_search(&b, duration))
            .map_or(Outcome::Missing, Outcome::Lyrics),
        Err(()) => Outcome::Failed,
    }
}

impl LyricsService {
    /// Call whenever the track or the lyrics setting may have changed.
    pub fn update(&mut self, enabled: bool, track: Option<&Track>, waker: Waker) {
        let Some(track) = track.filter(|_| enabled) else {
            self.key.clear();
            self.state = LyricsState::Off;
            return;
        };
        let duration = track.duration as f32;
        let key = music::track_key(&track.title, &track.artist, duration);
        if key == self.key {
            return; // same song as before: already looked up or in progress
        }
        self.key = key.clone();
        // Developer aid: the pretend track (`PILLBOX_FAKE_TRACK`) takes its lyrics from this
        // .lrc file instead of the network.
        if track.app_id == "fake.exe"
            && let Some(path) = std::env::var_os("PILLBOX_FAKE_LYRICS")
        {
            let text = fs::read_to_string(path).unwrap_or_default();
            self.state = Some(lrc::parse(&text))
                .filter(|l| !l.is_empty())
                .map(Lyrics::with_intro_gap)
                .map_or(LyricsState::NotFound, LyricsState::Found);
            return;
        }
        if track.is_video || music::looks_like_music_video(&track.title) {
            self.state = LyricsState::SkippedVideo;
            return;
        }
        if let Some(known) = self.memory.get(&key) {
            self.state = known
                .clone()
                .map_or(LyricsState::NotFound, LyricsState::Found);
            return;
        }
        if let Some(text) = cache_file(&key).and_then(|p| fs::read_to_string(p).ok()) {
            let parsed = Some(lrc::parse(&text))
                .filter(|l| !l.is_empty())
                .map(Lyrics::with_intro_gap);
            self.state = parsed
                .clone()
                .map_or(LyricsState::NotFound, LyricsState::Found);
            self.memory.insert(key, parsed);
            return;
        }
        let (tx, rx) = mpsc::channel();
        let (title, artist, album) = (
            track.title.clone(),
            track.artist.clone(),
            track.album.clone(),
        );
        let spawned = std::thread::Builder::new()
            .name("lyrics".into())
            .spawn(move || {
                let _ = tx.send(look_up(&title, &artist, &album, duration));
                waker.wake();
            });
        match spawned {
            Ok(_) => {
                tracing::info!("looking up lyrics");
                self.pending = Some((key, rx));
                self.state = LyricsState::Loading;
            }
            Err(e) => {
                tracing::warn!("could not start the lyrics lookup: {e}");
                self.state = LyricsState::NotFound;
            }
        }
    }

    /// Collect a finished lookup. Returns true if the state changed.
    pub fn poll(&mut self) -> bool {
        let Some((key, rx)) = &self.pending else {
            return false;
        };
        let outcome = match rx.try_recv() {
            Ok(outcome) => outcome,
            Err(mpsc::TryRecvError::Empty) => return false,
            Err(mpsc::TryRecvError::Disconnected) => Outcome::Failed,
        };
        let key = key.clone();
        self.pending = None;
        let parsed = match outcome {
            Outcome::Lyrics(text) => {
                let parsed = Some(lrc::parse(&text))
                    .filter(|l| !l.is_empty())
                    .map(Lyrics::with_intro_gap);
                if let Some(path) = cache_file(&key) {
                    let _ = fs::write(path, if parsed.is_some() { text.as_str() } else { "" });
                }
                parsed
            }
            Outcome::Missing => {
                // An empty file records "asked, nothing there".
                if let Some(path) = cache_file(&key) {
                    let _ = fs::write(path, "");
                }
                None
            }
            Outcome::Failed => {
                tracing::warn!(
                    "lyrics lookup failed (network?); will retry when the song plays again"
                );
                if key == self.key {
                    self.key.clear();
                    self.state = LyricsState::NotFound;
                }
                return true;
            }
        };
        tracing::info!(
            "lyrics {}",
            if parsed.is_some() {
                "found"
            } else {
                "not found"
            }
        );
        self.memory.insert(key.clone(), parsed.clone());
        if key == self.key {
            self.state = parsed.map_or(LyricsState::NotFound, LyricsState::Found);
            return true;
        }
        false
    }
}
