//! M8: the notification mirror. A worker (`platform::notifications`) reads Windows' toast
//! notifications every few seconds and wakes the UI thread when they change; this applies them:
//! the list for the Notifications tab, and a stack on the closed pill for the ones that just
//! arrived. Per-app rules (hide, or list only) are applied here, so they take effect at once
//! when the settings change.

use std::collections::{HashMap, HashSet};

use pillbox_core::{
    image::Bgra,
    notes as logic,
    notes::{Note, NoteStore, Synced},
    settings::NoteMode,
};

use super::{App, Mode, TOAST_MS, Toast, script::now_ms};
use crate::{
    platform::{
        gfx::ImageSlot,
        notifications::{Access, NoteWatch},
        system,
        thumbs::Job,
    },
    scene::{NoteStackView, Tab},
};

/// The stack on the closed pill going away.
pub(super) const TIMER_NOTES: usize = 60;

/// The pill fits about this many characters beside the badges.
const PILL_CHARS: usize = 18;

#[derive(Default)]
pub(super) struct Notes {
    watch: Option<NoteWatch>,
    pub store: NoteStore,
    access: Access,
    /// The last reading as Windows gave it, before the rules: they can change without a new one.
    raw: Vec<Note>,
    /// While the wheel is flipping through the stack: the notifications that were in it, so
    /// they stay even after their own time is up.
    pinned: Vec<u32>,
    /// Which of the stacked notifications is in front.
    flip: usize,
    /// Apps whose icon was asked for, and the decoded pictures (for re-uploading).
    icons_requested: HashSet<u64>,
    icons: HashMap<u64, Bgra>,
}

impl App {
    /// Start or stop the reader to match the settings (called at start and on every reload).
    pub(super) fn sync_notes(&mut self) {
        if self.settings.notifications.enabled {
            if self.notes.watch.is_none() {
                self.notes.watch = Some(NoteWatch::start(self.window.waker()));
                self.sync_notes_pause();
                tracing::info!("notification mirror on");
            } else if !self.notes.raw.is_empty() {
                // The rules may have changed.
                self.apply_rules();
            }
            return;
        }
        if self.notes.watch.take().is_some() {
            tracing::info!("notification mirror off");
        }
        self.notes.store.clear();
        self.notes.raw.clear();
        self.notes.pinned.clear();
        self.notes.flip = 0;
        self.notes.access = Access::Unknown;
        self.window.kill_timer(TIMER_NOTES);
        if self.tab == Tab::Notifications {
            self.tab = Tab::Home;
        }
        self.retarget();
        self.dirty = true;
    }

    /// The reader is idle while a game is in front.
    pub(super) fn sync_notes_pause(&mut self) {
        let quiet = self.notes_quiet();
        if let Some(w) = &self.notes.watch {
            w.set_paused(quiet);
        }
    }

    fn notes_quiet(&self) -> bool {
        self.mode != Mode::Normal || self.fullscreen.is_some()
    }

    fn stack_ms(&self) -> i64 {
        i64::from(self.settings.notifications.stack_seconds) * 1000
    }

    /// What the rules say about a notification's app.
    fn note_mode(&self, n: &Note) -> Option<NoteMode> {
        logic::rule_mode(n, &self.settings.notifications.rules)
    }

    /// Feed the store the last reading without the notifications of hidden apps.
    fn apply_rules(&mut self) -> Synced {
        let rules = &self.settings.notifications.rules;
        let shown: Vec<Note> = self
            .notes
            .raw
            .iter()
            .filter(|n| logic::rule_mode(n, rules) != Some(NoteMode::Hide))
            .cloned()
            .collect();
        let synced = self.notes.store.sync(shown, now_ms());
        if synced.changed {
            self.dirty = true;
        }
        synced
    }

    /// What the reader found (called when its worker woke the UI thread).
    pub(super) fn poll_notes(&mut self) {
        let Some(update) = self.notes.watch.as_ref().and_then(NoteWatch::poll) else {
            return;
        };
        if let Some(access) = update.access {
            self.notes.access = access;
            self.dirty = true;
        }
        for (app_id, logo) in update.icons {
            self.request_app_icon(&app_id, logo);
        }
        let Some(list) = update.notes else { return };
        self.notes.raw = list;
        let synced = self.apply_rules();
        if synced.arrived > 0 && !self.notes_quiet() {
            // The newest arrival goes to the front, and the stack gets its full time.
            self.notes.pinned.clear();
            self.notes.flip = 0;
            self.hold_stack();
            self.retarget();
        }
    }

    /// Keep the stack on the pill for the full duration from now.
    fn hold_stack(&mut self) {
        // A little over, so the stack has certainly expired when the timer fires.
        let ms = u32::try_from(self.stack_ms() + 50).unwrap_or(u32::MAX);
        self.window.set_timer(TIMER_NOTES, ms);
        // Stacked notifications must be seen (see `Window::raise`).
        self.window.raise();
    }

    /// The stack's time is up: the pill goes back to what it showed before.
    pub(super) fn notes_stack_expired(&mut self) {
        self.window.kill_timer(TIMER_NOTES);
        self.notes.pinned.clear();
        self.notes.flip = 0;
        // If the pointer waited on the stack, the card opens now.
        if self.hovering
            && !self.open
            && self.toast.is_none()
            && self.script.showing_flash.is_none()
            && self.mode == Mode::Normal
        {
            self.window
                .set_timer(super::TIMER_OPEN, self.settings.behavior.open_delay_ms);
        }
        self.retarget();
        self.dirty = true;
    }

    /// The notifications stacked on the pill, newest first, apart from the apps set to "list
    /// only". While the wheel flips through them they are the ones that were stacked then.
    fn stacked(&self) -> Vec<&Note> {
        if !self.notes.pinned.is_empty() {
            return self
                .notes
                .pinned
                .iter()
                .filter_map(|id| self.notes.store.notes().find(|n| n.id == *id))
                .collect();
        }
        let mut fresh = self.notes.store.stack(now_ms(), self.stack_ms());
        fresh.retain(|n| self.note_mode(n) != Some(NoteMode::ListOnly));
        fresh
    }

    /// The stack for the closed pill, if notifications arrived in the last few seconds.
    pub(super) fn note_stack(&self) -> Option<NoteStackView> {
        if self.notes_quiet() {
            return None;
        }
        let mut list = self.stacked();
        let count = list.len();
        if count == 0 {
            return None;
        }
        let index = self.notes.flip % count;
        list.rotate_left(index);
        Some(NoteStackView {
            badges: list
                .iter()
                .take(3)
                .map(|n| (n.app_id.clone(), n.app.clone()))
                .collect(),
            text: logic::pill_text(list[0], PILL_CHARS),
            count,
            index,
        })
    }

    pub(super) fn note_stack_showing(&self) -> bool {
        self.note_stack().is_some()
    }

    /// The wheel turned over the closed pill: flip to the next (down) or previous (up)
    /// notification in the stack, and keep the stack up for its full time again. Returns false
    /// when there is nothing to flip.
    pub(super) fn flip_stack(&mut self, delta: f32) -> bool {
        if self.open || self.notes_quiet() || delta == 0.0 {
            return false;
        }
        let stacked: Vec<u32> = self.stacked().iter().map(|n| n.id).collect();
        if stacked.is_empty() {
            return false;
        }
        let count = stacked.len();
        if self.notes.pinned.is_empty() {
            self.notes.pinned = stacked;
        }
        let at = self.notes.flip % count;
        self.notes.flip = if delta < 0.0 {
            (at + 1) % count
        } else {
            (at + count - 1) % count
        };
        self.hold_stack();
        self.dirty = true;
        true
    }

    /// A click on the closed pill while notifications are stacked on it: open the list.
    pub(super) fn click_notes(&mut self) -> bool {
        if self.open || !self.note_stack_showing() || !self.summon_tab(Tab::Notifications) {
            return false;
        }
        self.hovering = true;
        self.set_open(true);
        self.tab_shown();
        true
    }

    /// A click on a notification in the list: open the app that posted it. Windows has no way to
    /// jump to the notification's own page, so this brings the app up as if started from the
    /// Start menu.
    pub(super) fn open_note_app(&mut self, id: u32) {
        let Some(n) = self.notes.store.notes().find(|n| n.id == id) else {
            return;
        };
        let (app_id, app) = (n.app_id.clone(), n.app.clone());
        if system::open_app(&app_id) {
            tracing::info!("opened {app} from its notification");
        } else {
            tracing::warn!("could not open {app} ({app_id})");
            let name = logic::short(&app, 16);
            self.show_toast(Toast::text(format!("Could not open {name}")), TOAST_MS);
        }
    }

    /// Why the list is empty, when it is not simply that there are no notifications.
    pub(super) fn notes_message(&self) -> Option<&'static str> {
        match self.notes.access {
            Access::Denied => Some(
                "Windows blocked notification access. Allow it in Settings > Privacy & security > Notifications.",
            ),
            Access::Unspecified => Some("Waiting for Windows to allow notification access…"),
            Access::Failed => Some("Windows would not let Pillbox read notifications."),
            Access::Unknown if self.notes.watch.is_some() => Some("Reading notifications…"),
            _ => None,
        }
    }
}

/// Largest side of an app icon as decoded, pixels: enough for a 28 px disc at 250% scaling.
const ICON_PX: u32 = 72;

impl App {
    /// Ask the thumbnail worker for an app's icon: its logo when Windows gave one, else the
    /// icon the Start menu shows for it (`shell:AppsFolder\<app id>`). Without either the badge
    /// keeps its letter.
    pub(super) fn request_app_icon(&mut self, app_id: &str, logo: Option<Vec<u8>>) {
        let key = logic::icon_key(app_id);
        if !self.notes.icons_requested.insert(key) {
            return;
        }
        let job = match logo {
            Some(data) => Job::Bytes {
                key,
                data,
                max: ICON_PX,
            },
            None => Job::File {
                key,
                path: format!("shell:AppsFolder\\{app_id}").into(),
                size: ICON_PX,
            },
        };
        self.t.thumbs.request(job);
    }

    /// An icon was decoded and uploaded: remember it, because a rebuilt GPU device loses it.
    pub(super) fn icon_ready(&mut self, key: u64, image: &Bgra) {
        if self.notes.icons_requested.contains(&key) {
            self.notes.icons.insert(key, image.clone());
        }
    }

    /// After the GPU device was rebuilt: upload the icons again.
    pub(super) fn reupload_icons(&mut self) {
        for (key, img) in &self.notes.icons {
            self.renderer
                .set_image(ImageSlot::Thumb(*key), &img.pixels, img.width, img.height);
        }
    }
}
