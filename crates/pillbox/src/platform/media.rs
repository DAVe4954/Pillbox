//! "Now playing" through Windows' system media transport controls (the same source the
//! lock screen and the volume flyout use). Works with any app that reports playback to
//! Windows: Spotify, browsers, Apple Music, VLC, foobar2000 and so on.
//!
//! Everything runs on its own thread, because the WinRT calls block. Windows raises events
//! when the track, play state or position changes; nothing polls.

use std::{
    sync::mpsc::{self, Receiver, Sender},
    time::Instant,
};

use anyhow::{Context, Result, anyhow};
use pillbox_core::{
    music::{source_allowed, stable_hash},
    settings::SourceMode,
};
use windows::{
    Foundation::TypedEventHandler,
    Media::{
        Control::{
            GlobalSystemMediaTransportControlsSession as Session,
            GlobalSystemMediaTransportControlsSessionManager as Manager,
            GlobalSystemMediaTransportControlsSessionPlaybackStatus as PlaybackStatus,
        },
        MediaPlaybackType,
    },
    Storage::Streams::{DataReader, IRandomAccessStreamReference},
    Win32::{
        Graphics::Imaging::{
            CLSID_WICImagingFactory, GUID_WICPixelFormat32bppPBGRA, IWICImagingFactory,
            WICBitmapDitherTypeNone, WICBitmapInterpolationModeFant, WICBitmapPaletteTypeCustom,
            WICDecodeMetadataCacheOnDemand,
        },
        System::{
            Com::{CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx},
            SystemInformation::GetSystemTimeAsFileTime,
        },
    },
};

use super::window::Waker;

/// Album art, decoded and scaled down.
pub struct Art {
    pub width: u32,
    pub height: u32,
    /// Premultiplied BGRA, `width * height * 4` bytes.
    pub bgra: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Track {
    pub app_id: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub playing: bool,
    /// The app says this is video, not music.
    pub is_video: bool,
    /// Seconds into the track at the moment `at`.
    pub position: f64,
    /// Seconds; 0 when the app does not report a length.
    pub duration: f64,
    pub at: Instant,
    /// What the player says it accepts right now. Not every app supports every control.
    pub can_seek: bool,
    pub can_next: bool,
    pub can_previous: bool,
}

impl Track {
    /// Where playback is right now.
    pub fn position_now(&self) -> f64 {
        let mut p = self.position;
        if self.playing {
            p += self.at.elapsed().as_secs_f64();
        }
        if self.duration > 0.0 {
            p = p.min(self.duration);
        }
        p.max(0.0)
    }
}

pub enum ArtUpdate {
    Unchanged,
    Cleared,
    New(Art),
}

pub struct MediaUpdate {
    pub track: Option<Track>,
    pub art: ArtUpdate,
}

#[derive(Debug, Clone)]
pub enum MediaCommand {
    PlayPause,
    Next,
    Previous,
    /// Seconds from the start.
    SeekTo(f64),
    SetFilter {
        enabled: bool,
        mode: SourceMode,
        apps: Vec<String>,
    },
}

enum Msg {
    Command(MediaCommand),
    /// The set of sessions, or which one is current, changed.
    Sessions,
    /// Title, artist or artwork changed.
    Properties,
    /// Play state or position changed.
    Playback,
}

/// Handle to the media thread.
pub struct MediaService {
    tx: Sender<Msg>,
    rx: Receiver<MediaUpdate>,
}

impl MediaService {
    /// `fake` is a developer aid: `"Title|Artist|Album|seconds"` plays a pretend track
    /// instead of listening to Windows, so the UI can be tested silently.
    pub fn start(waker: Waker, fake: Option<String>) -> Self {
        let (tx, thread_rx) = mpsc::channel();
        let (out, rx) = mpsc::channel();
        let thread_tx = tx.clone();
        let spawned = std::thread::Builder::new()
            .name("media".into())
            .spawn(move || {
                let result = match fake {
                    Some(spec) => run_fake(&spec, thread_rx, &out, waker),
                    None => run(thread_rx, thread_tx, &out, waker),
                };
                if let Err(e) = result {
                    tracing::warn!("now-playing is unavailable: {e:#}");
                }
            });
        if let Err(e) = spawned {
            tracing::warn!("could not start the media thread: {e}");
        }
        Self { tx, rx }
    }

    pub fn send(&self, command: MediaCommand) {
        let _ = self.tx.send(Msg::Command(command));
    }

    /// The newest state, if anything arrived since the last call. Artwork changes are never
    /// lost when several updates have queued up.
    pub fn poll(&self) -> Option<MediaUpdate> {
        let mut latest: Option<MediaUpdate> = None;
        while let Ok(update) = self.rx.try_recv() {
            let art = match (update.art, latest.take().map(|l| l.art)) {
                (ArtUpdate::Unchanged, Some(earlier)) => earlier,
                (art, _) => art,
            };
            latest = Some(MediaUpdate {
                track: update.track,
                art,
            });
        }
        latest
    }
}

struct Filter {
    enabled: bool,
    mode: SourceMode,
    apps: Vec<String>,
}

/// A session we are listening to, with the registrations needed to stop listening.
struct Bound {
    session: Session,
    app_id: String,
    tokens: (i64, i64, i64),
}

impl Drop for Bound {
    fn drop(&mut self) {
        let _ = self.session.RemoveMediaPropertiesChanged(self.tokens.0);
        let _ = self.session.RemovePlaybackInfoChanged(self.tokens.1);
        let _ = self.session.RemoveTimelinePropertiesChanged(self.tokens.2);
    }
}

#[derive(Default)]
struct Cached {
    title: String,
    artist: String,
    album: String,
    art_key: u64,
    has_art: bool,
}

fn now_filetime() -> i64 {
    // SAFETY: no arguments; returns a plain struct.
    let ft = unsafe { GetSystemTimeAsFileTime() };
    ((u64::from(ft.dwHighDateTime) << 32) | u64::from(ft.dwLowDateTime)) as i64
}

fn is_playing(session: &Session) -> bool {
    session
        .GetPlaybackInfo()
        .and_then(|i| i.PlaybackStatus())
        .is_ok_and(|s| s == PlaybackStatus::Playing)
}

/// Choose the session to show: Windows' current one if the filter allows it, otherwise the
/// first allowed session, preferring one that is playing.
fn pick(manager: &Manager, filter: &Filter) -> Option<(Session, String)> {
    if !filter.enabled {
        return None;
    }
    let allowed = |s: &Session| -> Option<String> {
        let id = s.SourceAppUserModelId().ok()?.to_string();
        source_allowed(filter.mode, &filter.apps, &id).then_some(id)
    };
    if let Ok(current) = manager.GetCurrentSession()
        && let Some(id) = allowed(&current)
    {
        return Some((current, id));
    }
    let sessions: Vec<Session> = manager.GetSessions().ok()?.into_iter().collect();
    let mut candidates: Vec<(Session, String)> = sessions
        .into_iter()
        .filter_map(|s| allowed(&s).map(|id| (s, id)))
        .collect();
    if candidates.is_empty() {
        return None;
    }
    let index = candidates
        .iter()
        .position(|(s, _)| is_playing(s))
        .unwrap_or(0);
    Some(candidates.swap_remove(index))
}

fn bind(session: Session, app_id: String, tx: &Sender<Msg>) -> Result<Bound> {
    let (t1, t2, t3) = (tx.clone(), tx.clone(), tx.clone());
    let tokens = (
        session.MediaPropertiesChanged(&TypedEventHandler::new(move |_, _| {
            let _ = t1.send(Msg::Properties);
            Ok(())
        }))?,
        session.PlaybackInfoChanged(&TypedEventHandler::new(move |_, _| {
            let _ = t2.send(Msg::Playback);
            Ok(())
        }))?,
        session.TimelinePropertiesChanged(&TypedEventHandler::new(move |_, _| {
            let _ = t3.send(Msg::Playback);
            Ok(())
        }))?,
    );
    Ok(Bound {
        session,
        app_id,
        tokens,
    })
}

/// Read a WinRT stream reference fully into memory.
fn read_stream(reference: &IRandomAccessStreamReference) -> Result<Vec<u8>> {
    let stream = reference.OpenReadAsync()?.get()?;
    let size = u32::try_from(stream.Size()?).map_err(|_| anyhow!("thumbnail too large"))?;
    if size == 0 || size > 16 * 1024 * 1024 {
        return Err(anyhow!("unusable thumbnail size {size}"));
    }
    let reader = DataReader::CreateDataReader(&stream.GetInputStreamAt(0)?)?;
    let loaded = reader.LoadAsync(size)?.get()?;
    let mut bytes = vec![0u8; loaded as usize];
    reader.ReadBytes(&mut bytes)?;
    Ok(bytes)
}

/// Decode any image format Windows knows (PNG, JPEG, BMP, ...) and scale it so its longer
/// side is `max_side` pixels. Premultiplied BGRA. Needs COM on the calling thread.
pub fn decode_image(bytes: &[u8], max_side: u32) -> Result<Art> {
    // SAFETY: standard WIC usage. `bytes` outlives the stream (everything is finished before
    // this function returns), and the pixel buffer is exactly stride * height bytes.
    unsafe {
        let factory: IWICImagingFactory =
            CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)
                .context("creating the imaging factory")?;
        let stream = factory.CreateStream()?;
        stream.InitializeFromMemory(bytes)?;
        let decoder = factory.CreateDecoderFromStream(
            &stream,
            std::ptr::null(),
            WICDecodeMetadataCacheOnDemand,
        )?;
        let frame = decoder.GetFrame(0)?;
        let (mut w, mut h) = (0u32, 0u32);
        frame.GetSize(&mut w, &mut h)?;
        if w == 0 || h == 0 {
            return Err(anyhow!("empty image"));
        }
        let scale = (max_side as f32 / w.max(h) as f32).min(1.0);
        let (nw, nh) = (
            ((w as f32 * scale).round() as u32).max(1),
            ((h as f32 * scale).round() as u32).max(1),
        );
        let scaler = factory.CreateBitmapScaler()?;
        scaler.Initialize(&frame, nw, nh, WICBitmapInterpolationModeFant)?;
        let converter = factory.CreateFormatConverter()?;
        converter.Initialize(
            &scaler,
            &GUID_WICPixelFormat32bppPBGRA,
            WICBitmapDitherTypeNone,
            None,
            0.0,
            WICBitmapPaletteTypeCustom,
        )?;
        let mut bgra = vec![0u8; (nw * nh * 4) as usize];
        converter.CopyPixels(std::ptr::null(), nw * 4, &mut bgra)?;
        Ok(Art {
            width: nw,
            height: nh,
            bgra,
        })
    }
}

/// Side length artwork is scaled to. Plenty for a 64-pixel thumbnail at 200% scaling.
const ART_SIDE: u32 = 160;

/// A seek the player accepted but has not yet reflected in its position reports. Some players
/// (Media Player) only report their position every few seconds.
struct PendingSeek {
    target: f64,
    at: Instant,
    /// File time when the seek was sent; a report measured after this is fresh.
    sent: i64,
}

fn read_state(
    bound: &Bound,
    cached: &mut Cached,
    properties: bool,
    pending: &mut Option<PendingSeek>,
) -> Result<MediaUpdate> {
    let session = &bound.session;
    let mut art = ArtUpdate::Unchanged;
    if properties {
        let props = session.TryGetMediaPropertiesAsync()?.get()?;
        cached.title = props.Title().map(|s| s.to_string()).unwrap_or_default();
        cached.artist = props.Artist().map(|s| s.to_string()).unwrap_or_default();
        cached.album = props
            .AlbumTitle()
            .map(|s| s.to_string())
            .unwrap_or_default();
        match props.Thumbnail().ok().and_then(|t| read_stream(&t).ok()) {
            Some(bytes) => {
                // Decode only when the picture itself changed.
                let key = stable_hash(&format!("{}:{}", bytes.len(), stable_hash_bytes(&bytes)));
                if key != cached.art_key || !cached.has_art {
                    match decode_image(&bytes, ART_SIDE) {
                        Ok(image) => {
                            cached.art_key = key;
                            cached.has_art = true;
                            art = ArtUpdate::New(image);
                        }
                        Err(e) => tracing::debug!("could not decode artwork: {e:#}"),
                    }
                }
            }
            None if cached.has_art => {
                cached.has_art = false;
                cached.art_key = 0;
                art = ArtUpdate::Cleared;
            }
            None => {}
        }
    }
    if cached.title.trim().is_empty() {
        return Ok(MediaUpdate { track: None, art });
    }
    let info = session.GetPlaybackInfo()?;
    let playing = info.PlaybackStatus()? == PlaybackStatus::Playing;
    let is_video = info
        .PlaybackType()
        .and_then(|t| t.Value())
        .is_ok_and(|t| t == MediaPlaybackType::Video);
    let rate = info
        .PlaybackRate()
        .and_then(|r| r.Value())
        .unwrap_or(1.0)
        .clamp(0.1, 4.0);
    let controls = info.Controls()?;
    let can_seek = controls.IsPlaybackPositionEnabled().unwrap_or(false);
    let can_next = controls.IsNextEnabled().unwrap_or(false);
    let can_previous = controls.IsPreviousEnabled().unwrap_or(false);
    let timeline = session.GetTimelineProperties()?;
    let start = timeline.StartTime()?.Duration;
    let end = timeline.EndTime()?.Duration;
    let reported = timeline.Position()?.Duration;
    let updated = timeline.LastUpdatedTime()?.UniversalTime;
    const TICKS: f64 = 10_000_000.0;
    let duration = ((end - start) as f64 / TICKS).max(0.0);
    let mut position = (reported - start) as f64 / TICKS;
    // Apps report the position only now and then, with the time they measured it.
    if playing && updated > 0 {
        let age = (now_filetime() - updated) as f64 / TICKS;
        if (0.0..86_400.0).contains(&age) {
            position += age * rate;
        }
    }
    match pending {
        // Still the report from before the seek: show where the seek went instead.
        Some(seek) if updated <= seek.sent && seek.at.elapsed().as_secs() < 15 => {
            position = seek.target
                + if playing {
                    seek.at.elapsed().as_secs_f64() * rate
                } else {
                    0.0
                };
        }
        _ => *pending = None,
    }
    if duration > 0.0 {
        position = position.min(duration);
    }
    tracing::debug!(
        "timeline: reported {:.1}s, measured {:.1}s ago, now {:.1}s of {:.1}s, playing {playing}",
        (reported - start) as f64 / TICKS,
        if updated > 0 {
            (now_filetime() - updated) as f64 / TICKS
        } else {
            -1.0
        },
        position,
        duration
    );
    Ok(MediaUpdate {
        track: Some(Track {
            app_id: bound.app_id.clone(),
            title: cached.title.clone(),
            artist: cached.artist.clone(),
            album: cached.album.clone(),
            playing,
            is_video,
            position: position.max(0.0),
            duration,
            at: Instant::now(),
            can_seek,
            can_next,
            can_previous,
        }),
        art,
    })
}

fn stable_hash_bytes(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

fn run(rx: Receiver<Msg>, tx: Sender<Msg>, out: &Sender<MediaUpdate>, waker: Waker) -> Result<()> {
    // SAFETY: initialises COM for this thread only; paired with nothing because the thread
    // lives as long as the process.
    unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }
        .ok()
        .context("initialising COM")?;
    let manager = Manager::RequestAsync()?
        .get()
        .context("asking Windows for the media session manager")?;
    let (t1, t2) = (tx.clone(), tx.clone());
    manager.CurrentSessionChanged(&TypedEventHandler::new(move |_, _| {
        let _ = t1.send(Msg::Sessions);
        Ok(())
    }))?;
    manager.SessionsChanged(&TypedEventHandler::new(move |_, _| {
        let _ = t2.send(Msg::Sessions);
        Ok(())
    }))?;

    let mut filter = Filter {
        enabled: true,
        mode: SourceMode::All,
        apps: Vec::new(),
    };
    let mut bound: Option<Bound> = None;
    let mut cached = Cached::default();
    let mut had_track = false;
    let mut clear_art = false;
    let mut pending: Option<PendingSeek> = None;
    let _ = tx.send(Msg::Sessions);

    while let Ok(first) = rx.recv() {
        // Events arrive in bursts; handle everything queued, then read the state once.
        let (mut rebind, mut properties) = (false, false);
        let mut next = Some(first);
        while let Some(msg) = next {
            match msg {
                Msg::Sessions => rebind = true,
                Msg::Properties => properties = true,
                Msg::Playback => {}
                Msg::Command(MediaCommand::SetFilter {
                    enabled,
                    mode,
                    apps,
                }) => {
                    filter = Filter {
                        enabled,
                        mode,
                        apps,
                    };
                    rebind = true;
                }
                Msg::Command(command) => {
                    if let Some(b) = &bound {
                        // The session raises its own events when the command takes effect.
                        let name = format!("{command:?}");
                        let seek_target = match &command {
                            MediaCommand::SeekTo(seconds) => Some(*seconds),
                            _ => None,
                        };
                        let sent = now_filetime();
                        let accepted = match command {
                            MediaCommand::PlayPause => {
                                b.session.TryTogglePlayPauseAsync().and_then(|op| op.get())
                            }
                            MediaCommand::Next => {
                                b.session.TrySkipNextAsync().and_then(|op| op.get())
                            }
                            MediaCommand::Previous => {
                                b.session.TrySkipPreviousAsync().and_then(|op| op.get())
                            }
                            MediaCommand::SeekTo(seconds) => b
                                .session
                                .TryChangePlaybackPositionAsync(
                                    (seconds.max(0.0) * 10_000_000.0) as i64,
                                )
                                .and_then(|op| op.get()),
                            MediaCommand::SetFilter { .. } => Ok(true),
                        };
                        pending = match (&accepted, seek_target) {
                            (Ok(true), Some(target)) => Some(PendingSeek {
                                target,
                                at: Instant::now(),
                                sent,
                            }),
                            // Any other command (skip, pause) makes an earlier seek moot.
                            _ => None,
                        };
                        match accepted {
                            Ok(true) => tracing::debug!("{name}: accepted by the player"),
                            Ok(false) => tracing::info!("{name}: the player refused"),
                            Err(e) => tracing::info!("{name}: failed ({e})"),
                        }
                    }
                }
            }
            next = rx.try_recv().ok();
        }
        if rebind {
            let wanted = pick(&manager, &filter);
            let same = match (&bound, &wanted) {
                (Some(b), Some((s, _))) => b.session == *s,
                (None, None) => true,
                _ => false,
            };
            if !same {
                bound = None; // stop listening to the old session first
                // The previous session's cover must not linger on the new one.
                clear_art = cached.has_art;
                cached = Cached::default();
                if let Some((session, app_id)) = wanted {
                    tracing::info!("following media session {app_id}");
                    match bind(session, app_id, &tx) {
                        Ok(b) => bound = Some(b),
                        Err(e) => tracing::warn!("could not follow the session: {e:#}"),
                    }
                }
                properties = true;
            }
        }
        let mut update = match &bound {
            Some(b) => read_state(b, &mut cached, properties, &mut pending).unwrap_or_else(|e| {
                tracing::debug!("media read failed: {e:#}");
                MediaUpdate {
                    track: None,
                    art: ArtUpdate::Unchanged,
                }
            }),
            None => MediaUpdate {
                track: None,
                art: if had_track {
                    ArtUpdate::Cleared
                } else {
                    ArtUpdate::Unchanged
                },
            },
        };
        if clear_art && matches!(update.art, ArtUpdate::Unchanged) {
            update.art = ArtUpdate::Cleared;
        }
        clear_art = false;
        had_track = update.track.is_some();
        if out.send(update).is_err() {
            break;
        }
        waker.wake();
    }
    Ok(())
}

/// The app ids of every session Windows currently knows, for the Settings window's picker.
pub fn session_app_ids() -> Vec<String> {
    let list = || -> Result<Vec<String>> {
        let manager = Manager::RequestAsync()?.get()?;
        Ok(manager
            .GetSessions()?
            .into_iter()
            .filter_map(|s| s.SourceAppUserModelId().ok())
            .map(|id| id.to_string())
            .collect())
    };
    list().unwrap_or_default()
}

/// A drawn stand-in cover for the pretend track.
fn fake_art() -> Art {
    const N: u32 = 96;
    let mut bgra = Vec::with_capacity((N * N * 4) as usize);
    for y in 0..N {
        for x in 0..N {
            let (fx, fy) = (x as f32 / N as f32, y as f32 / N as f32);
            let d = ((fx - 0.5).powi(2) + (fy - 0.5).powi(2)).sqrt();
            let disc = if d < 0.28 { 1.0 } else { 0.0 };
            let r = 240.0 * (1.0 - fy) + 90.0 * fy;
            let g = 120.0 * (1.0 - fx) + 40.0 * fx + 80.0 * disc;
            let b = 60.0 + 150.0 * fy + 40.0 * disc;
            bgra.extend_from_slice(&[
                b.min(255.0) as u8,
                g.min(255.0) as u8,
                r.min(255.0) as u8,
                255,
            ]);
        }
    }
    Art {
        width: N,
        height: N,
        bgra,
    }
}

fn run_fake(spec: &str, rx: Receiver<Msg>, out: &Sender<MediaUpdate>, waker: Waker) -> Result<()> {
    let mut parts = spec.split('|');
    let mut field = || parts.next().unwrap_or("").trim().to_string();
    let (title, artist, album) = (field(), field(), field());
    let duration: f64 = field().parse().unwrap_or(180.0);
    let mut track = Track {
        app_id: "fake.exe".into(),
        title,
        artist,
        album,
        playing: true,
        is_video: false,
        position: 0.0,
        duration,
        at: Instant::now(),
        can_seek: true,
        can_next: true,
        can_previous: true,
    };
    let mut enabled = true;
    let mut first = true;
    loop {
        let update = MediaUpdate {
            track: enabled.then(|| track.clone()),
            art: if first {
                ArtUpdate::New(fake_art())
            } else {
                ArtUpdate::Unchanged
            },
        };
        first = false;
        if out.send(update).is_err() {
            return Ok(());
        }
        waker.wake();
        let Ok(msg) = rx.recv() else { return Ok(()) };
        if let Msg::Command(command) = msg {
            let now = track.position_now();
            track.position = now;
            track.at = Instant::now();
            match command {
                MediaCommand::PlayPause => track.playing = !track.playing,
                MediaCommand::Next | MediaCommand::Previous => track.position = 0.0,
                MediaCommand::SeekTo(seconds) => track.position = seconds.clamp(0.0, duration),
                MediaCommand::SetFilter { enabled: on, .. } => enabled = on,
            }
        }
    }
}
