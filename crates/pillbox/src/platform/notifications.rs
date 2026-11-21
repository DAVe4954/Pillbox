//! M8: reads Windows toast notifications with `UserNotificationListener`.
//!
//! * Access: on the dev PC (Windows 11 Insider 26300) an exe without package identity already has
//!   access, with no prompt (the spike in `spike-identity/`). If the status is "unspecified" a
//!   request is made once (it needs the UI thread, so `start` must run there); "denied" is
//!   reported and retried, since it can be switched on in Windows Settings > Privacy & security >
//!   Notifications. The Microsoft docs describe a manifest capability for this, so a stable
//!   Windows 11 might refuse an unpackaged exe; undocumented either way.
//! * No live events: subscribing to `NotificationChanged` fails with 0x80070490 without package
//!   identity, so a worker thread polls `GetNotificationsAsync`: every 3 s, and every second for
//!   12 s after a change. A reading costs about 12 ms of CPU even with nothing new (about 23 ms
//!   of wall time for a dozen notifications), so polling faster costs real idle CPU. A
//!   notification's text is read
//!   only the first time its id is seen.
//! * Only toast notifications; only text and the app's name and id are read (no images yet).

use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

use pillbox_core::notes::{self, Note};
use windows::{
    ApplicationModel::AppInfo,
    Foundation::Size,
    Storage::Streams::DataReader,
    UI::Notifications::{
        KnownNotificationBindings,
        Management::{UserNotificationListener, UserNotificationListenerAccessStatus},
        NotificationKinds, UserNotification,
    },
    Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize},
};

use super::window::Waker;

/// Whether Windows lets Pillbox read notifications.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Access {
    #[default]
    Unknown,
    Allowed,
    /// The user (or a policy) said no; Windows Settings > Privacy & security > Notifications.
    Denied,
    /// Windows has not been asked yet, or the question was dismissed.
    Unspecified,
    /// The API call itself failed.
    Failed,
}

/// What a poll brings.
#[derive(Debug, Default)]
pub struct Update {
    /// The notifications now present, when they changed (or on the first reading).
    pub notes: Option<Vec<Note>>,
    /// The access status, when it changed.
    pub access: Option<Access>,
    /// Icons of apps seen for the first time: the app id and its logo file bytes (`None` for an
    /// app Windows gives no logo for, such as a classic desktop program).
    pub icons: Vec<(String, Option<Vec<u8>>)>,
}

#[derive(Default)]
struct Shared {
    notes: Option<Vec<Note>>,
    access: Option<Access>,
    icons: Vec<(String, Option<Vec<u8>>)>,
}

/// Polls on a worker thread until dropped.
pub struct NoteWatch {
    /// Dropping the sender wakes the worker and ends it.
    _stop: mpsc::Sender<()>,
    shared: Arc<Mutex<Shared>>,
    paused: Arc<AtomicBool>,
}

/// Between readings when nothing has changed lately. Each reading costs about 12 ms of CPU
/// (Windows marshals every notification to us), so this is the idle cost: about 0.4% of one core.
const SLOW: Duration = Duration::from_secs(3);
/// Between readings for `HOT_FOR` after a change: notifications come in bursts.
const FAST: Duration = Duration::from_secs(1);
const HOT_FOR: Duration = Duration::from_secs(12);
/// Between tries while access is not granted.
const RETRY: Duration = Duration::from_secs(5);

impl NoteWatch {
    /// Call on the UI thread: asking for access needs it.
    pub fn start(waker: Waker) -> Self {
        if let Ok(listener) = UserNotificationListener::Current()
            && listener.GetAccessStatus().ok()
                == Some(UserNotificationListenerAccessStatus::Unspecified)
        {
            // The answer is read by the worker; this call must not block the UI thread.
            let _ = listener.RequestAccessAsync();
        }
        let (stop, rx) = mpsc::channel::<()>();
        let shared = Arc::new(Mutex::new(Shared::default()));
        let paused = Arc::new(AtomicBool::new(false));
        let (shared2, paused2) = (shared.clone(), paused.clone());
        let spawned = std::thread::Builder::new()
            .name("notifications".into())
            .spawn(move || run(&rx, &shared2, &paused2, waker));
        if let Err(e) = spawned {
            tracing::warn!("could not start the notification reader: {e}");
        }
        Self {
            _stop: stop,
            shared,
            paused,
        }
    }

    /// Take what the worker found since the last call.
    pub fn poll(&self) -> Option<Update> {
        let mut s = self.shared.lock().ok()?;
        let update = Update {
            notes: s.notes.take(),
            access: s.access.take(),
            icons: std::mem::take(&mut s.icons),
        };
        (update.notes.is_some() || update.access.is_some() || !update.icons.is_empty())
            .then_some(update)
    }

    /// Stop reading while a game is in front (readings resume, and catch up, afterwards).
    pub fn set_paused(&self, paused: bool) {
        self.paused.store(paused, Ordering::Relaxed);
    }
}

fn status_of(s: UserNotificationListenerAccessStatus) -> Access {
    match s {
        UserNotificationListenerAccessStatus::Allowed => Access::Allowed,
        UserNotificationListenerAccessStatus::Denied => Access::Denied,
        _ => Access::Unspecified,
    }
}

fn run(rx: &mpsc::Receiver<()>, shared: &Mutex<Shared>, paused: &AtomicBool, waker: Waker) {
    // SAFETY: called once at the start of this thread, which owns no apartment yet.
    if let Err(e) = unsafe { RoInitialize(RO_INIT_MULTITHREADED) } {
        tracing::warn!("notification reader: RoInitialize: {e}");
    }
    let publish = |f: &dyn Fn(&mut Shared)| {
        if let Ok(mut s) = shared.lock() {
            f(&mut s);
        }
        waker.wake();
    };
    let mut access = Access::Unknown;
    let mut last: Option<Vec<Note>> = None;
    let mut cache: HashMap<u32, Note> = HashMap::new();
    // Apps whose icon was already sent (an icon is fetched once per app per run).
    let mut icons_sent: HashSet<String> = HashSet::new();
    let mut wait = Duration::ZERO;
    let mut hot_until = Instant::now();
    // `recv_timeout` is the sleep: an `Err(Timeout)` means "time for the next reading", and a
    // closed channel means the watch was dropped.
    while matches!(rx.recv_timeout(wait), Err(mpsc::RecvTimeoutError::Timeout)) {
        wait = if Instant::now() < hot_until {
            FAST
        } else {
            SLOW
        };
        if paused.load(Ordering::Relaxed) {
            continue;
        }
        let Ok(listener) = UserNotificationListener::Current() else {
            wait = RETRY;
            continue;
        };
        let now_access = listener.GetAccessStatus().map_or(Access::Failed, status_of);
        if now_access != access {
            access = now_access;
            tracing::info!("notification access: {access:?}");
            publish(&|s| s.access = Some(access));
        }
        if access != Access::Allowed {
            wait = RETRY;
            continue;
        }
        let mut new_icons = Vec::new();
        let result = read(&listener, &mut cache, &mut icons_sent, &mut new_icons);
        if !new_icons.is_empty() {
            publish(&|s| s.icons.extend(new_icons.iter().cloned()));
        }
        match result {
            Ok(notes) => {
                if last.as_ref() != Some(&notes) {
                    last = Some(notes.clone());
                    hot_until = Instant::now() + HOT_FOR;
                    publish(&|s| s.notes = Some(notes.clone()));
                }
            }
            Err(e) => {
                tracing::warn!("notification read failed: {e}");
                wait = RETRY;
            }
        }
    }
}

/// The notifications present now. Text is read only for ids not in `cache`.
fn read(
    listener: &UserNotificationListener,
    cache: &mut HashMap<u32, Note>,
    icons_sent: &mut HashSet<String>,
    new_icons: &mut Vec<(String, Option<Vec<u8>>)>,
) -> windows::core::Result<Vec<Note>> {
    let list = listener
        .GetNotificationsAsync(NotificationKinds::Toast)?
        .get()?;
    let mut out = Vec::new();
    let mut seen = Vec::new();
    for n in list {
        let id = n.Id()?;
        seen.push(id);
        if let Some(known) = cache.get(&id) {
            out.push(known.clone());
            continue;
        }
        match convert(&n) {
            Ok(note) => {
                if icons_sent.insert(note.app_id.clone()) {
                    let logo = n.AppInfo().ok().and_then(|info| logo_bytes(&info));
                    new_icons.push((note.app_id.clone(), logo));
                }
                cache.insert(id, note.clone());
                out.push(note);
            }
            Err(e) => tracing::debug!("notification {id} skipped: {e}"),
        }
    }
    cache.retain(|id, _| seen.contains(id));
    Ok(out)
}

/// The bytes of an app's logo file (PNG, usually), `None` when Windows has none for it: classic
/// desktop programs return an empty reference (seen as success with a null result).
fn logo_bytes(info: &AppInfo) -> Option<Vec<u8>> {
    /// A logo is a small picture; anything bigger is not one.
    const MAX_BYTES: u32 = 1 << 20;
    let reference = info
        .DisplayInfo()
        .ok()?
        .GetLogo(Size {
            Width: 64.0,
            Height: 64.0,
        })
        .ok()?;
    let stream = reference.OpenReadAsync().ok()?.get().ok()?;
    let size = u32::try_from(stream.Size().ok()?)
        .ok()
        .filter(|s| (1..=MAX_BYTES).contains(s))?;
    let reader = DataReader::CreateDataReader(&stream.GetInputStreamAt(0).ok()?).ok()?;
    reader.LoadAsync(size).ok()?.get().ok()?;
    let mut bytes = vec![0u8; size as usize];
    reader.ReadBytes(&mut bytes).ok()?;
    Some(bytes)
}

fn convert(n: &UserNotification) -> windows::core::Result<Note> {
    let info = n.AppInfo()?;
    let binding = n
        .Notification()?
        .Visual()?
        .GetBinding(&KnownNotificationBindings::ToastGeneric()?)?;
    let mut texts = binding
        .GetTextElements()?
        .into_iter()
        .filter_map(|t| t.Text().ok())
        .map(|t| notes::clean(&t.to_string_lossy()))
        .filter(|t| !t.is_empty());
    let title = texts.next().unwrap_or_default();
    let body = texts.collect::<Vec<_>>().join(" ");
    Ok(Note {
        id: n.Id()?,
        app_id: info.AppUserModelId()?.to_string_lossy(),
        app: notes::clean(&info.DisplayInfo()?.DisplayName()?.to_string_lossy()),
        title,
        body,
        created_ms: notes::unix_ms_from_ticks(n.CreationTime()?.UniversalTime),
    })
}
