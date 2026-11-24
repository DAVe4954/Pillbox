//! M6: the QR code on the Shelf tab and the phone server behind it. The server runs only while
//! the code is on screen (card open on the Shelf tab, Pillbox visible, not in a game mode, phone
//! features on). The card stays open while the code shows, until Done, the time limit, or the
//! Pillbox shortcut.

use std::{
    path::PathBuf,
    sync::mpsc::{self, Receiver},
    time::{Duration, Instant},
};

use pillbox_core::phone::{self as logic, Qr};

use super::{App, Mode, TOAST_MS, Toast};
use crate::{
    phone::{Offer, PhoneEvent, PhoneServer},
    platform::{
        clipboard,
        firewall::{self, Firewall},
        net, screen,
    },
    scene::Tab,
    tabs::PhoneUi,
};

pub(super) const TIMER_PHONE: usize = 40;
pub(super) const TIMER_PHONE_CLIP: usize = 41;

/// The last two parts of a folder ("Downloads\Pillbox"), enough to recognise it on the card.
fn short_path(p: &std::path::Path) -> String {
    let parts: Vec<String> = p
        .components()
        .rev()
        .take(2)
        .map(|c| {
            c.as_os_str()
                .to_string_lossy()
                .trim_end_matches('\\')
                .to_string()
        })
        .collect();
    parts.into_iter().rev().collect::<Vec<_>>().join("\\")
}

const G_PHONE: &str = "\u{E8EA}";

#[derive(Default)]
pub(super) struct Phone {
    session: Option<Session>,
    /// Text from the phone still to be copied, and how many tries failed so far.
    pending_text: Option<(String, u32)>,
}

struct Session {
    server: PhoneServer,
    qr: Qr,
    title: String,
    detail: String,
    status: String,
    address: String,
    ends: Instant,
    /// The firewall check running on a worker thread.
    firewall: Option<Receiver<Firewall>>,
    /// Shown in red when the firewall will drop the phone's connection.
    warning: Option<String>,
    /// Name of the network the server listens on, and whether Windows calls it Private.
    network: (String, bool),
}

impl Phone {
    /// A QR code is on screen.
    pub fn showing(&self) -> bool {
        self.session.is_some()
    }

    pub fn ui(&self) -> Option<PhoneUi<'_>> {
        self.session.as_ref().map(|s| PhoneUi {
            qr: &s.qr,
            title: &s.title,
            detail: &s.detail,
            status: &s.status,
            address: &s.address,
            seconds_left: s.ends.saturating_duration_since(Instant::now()).as_secs(),
            warning: s.warning.as_deref(),
        })
    }
}

impl App {
    /// Phone ← PC: a code for downloading one shelf file.
    pub(super) fn phone_send(&mut self, index: usize) {
        let Some(path) = self.shelf.get(index).cloned() else {
            return;
        };
        let meta = match std::fs::metadata(&path) {
            Ok(m) if m.is_file() => m,
            Ok(_) => return self.show_toast(Toast::text("Only files can go to the phone"), 2500),
            Err(_) => return self.show_toast(Toast::text("That file is gone"), 2500),
        };
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "file".into());
        let detail = format!("{name} · {} · same Wi-Fi", logic::human_size(meta.len()));
        let offer = Offer::File {
            path,
            name,
            size: meta.len(),
        };
        self.phone_start(offer, "Scan to get this file on your phone", detail);
    }

    /// Phone → PC: a code for sending photos, files and text here.
    pub(super) fn phone_receive(&mut self) {
        let dir = match self.receive_dir() {
            Ok(d) => d,
            Err(e) => {
                tracing::warn!("phone receive folder: {e:#}");
                return self.show_toast(Toast::text("No folder to save into"), TOAST_MS);
            }
        };
        let p = &self.settings.phone;
        let max_bytes = pillbox_core::settings::PHONE_MAX_UPLOAD_MB * 1024 * 1024;
        let detail = format!(
            "Same Wi-Fi. Photos, files or text, saved in {}{}",
            short_path(&dir),
            if p.received_to_shelf {
                " and put on the shelf"
            } else {
                ""
            }
        );
        self.phone_start(
            Offer::Receive { dir, max_bytes },
            "Scan to send from your phone",
            detail,
        );
    }

    fn receive_dir(&self) -> anyhow::Result<PathBuf> {
        let custom = self.settings.phone.receive_folder.trim();
        if custom.is_empty() {
            Ok(net::downloads_dir()?.join("Pillbox"))
        } else {
            Ok(PathBuf::from(custom))
        }
    }

    fn phone_start(&mut self, offer: Offer, title: &str, detail: String) {
        if !self.settings.phone.enabled {
            return;
        }
        self.phone_close("replaced");
        let adapters = net::adapters();
        let Some(adapter) = logic::pick_adapter(&adapters).cloned() else {
            tracing::warn!("phone: no private network address among {adapters:?}");
            return self.show_toast(Toast::icon(G_PHONE, "Connect the PC to Wi-Fi first"), 3000);
        };
        let server = match PhoneServer::start(&adapter, offer, self.window.waker()) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("phone server: {e:#}");
                return self.show_toast(Toast::text("Could not start the phone link"), 3000);
            }
        };
        let Some(qr) = Qr::new(&server.url) else {
            tracing::warn!("phone: the link does not fit in a QR code");
            return;
        };
        let minutes = self.settings.phone.code_minutes;
        self.window.set_timer(TIMER_PHONE, minutes * 60 * 1000);
        self.phone.session = Some(Session {
            address: server.addr.to_string(),
            server,
            qr,
            title: title.to_string(),
            detail,
            status: "Waiting for your phone…".into(),
            ends: Instant::now() + Duration::from_secs(u64::from(minutes) * 60),
            firewall: self.start_firewall_check(adapter.trusted),
            warning: None,
            network: (adapter.name.clone(), adapter.trusted),
        });
        self.summon_tab(Tab::Shelf);
        self.dirty = true;
    }

    /// Take the code off screen; the server stops accepting connections.
    pub(super) fn phone_close(&mut self, why: &str) {
        if self.phone.session.take().is_some() {
            self.window.kill_timer(TIMER_PHONE);
            tracing::info!("phone code closed ({why})");
            self.dirty = true;
        }
    }

    /// The code's time is up.
    pub(super) fn phone_timeout(&mut self) {
        self.window.kill_timer(TIMER_PHONE);
        if !self.phone.showing() {
            return;
        }
        self.phone_close("time limit");
        self.release_card_if_pointer_away();
    }

    /// The card was held open for the code; close it if the pointer is not on it any more.
    fn release_card_if_pointer_away(&mut self) {
        let over = screen::cursor_pos().is_some_and(|(x, y)| {
            self.pill()
                .contains((x - self.win_pos.0) as f32, (y - self.win_pos.1) as f32)
        });
        if !over && !self.holds_open() {
            self.hovering = false;
            self.set_open(false);
        }
    }

    /// The code is shown only while it can be seen; otherwise the server goes too.
    pub(super) fn sync_phone(&mut self) {
        if self.phone.showing()
            && !(self.open
                && self.tab == Tab::Shelf
                && self.mode == Mode::Normal
                && self.effective_visible()
                && self.settings.phone.enabled)
        {
            self.phone_close("no longer on screen");
        }
    }

    /// Ask, on a worker thread, whether Windows Firewall will let the phone in. Read-only.
    fn start_firewall_check(&self, trusted: bool) -> Option<Receiver<Firewall>> {
        let (tx, rx) = mpsc::channel();
        let waker = self.window.waker();
        let spawned = std::thread::Builder::new()
            .name("firewall-check".into())
            .spawn(move || {
                match firewall::check(trusted) {
                    Ok(verdict) => {
                        let _ = tx.send(verdict);
                    }
                    // Unknown: say nothing rather than warn wrongly.
                    Err(e) => tracing::warn!("firewall check: {e:#}"),
                }
                waker.wake();
            });
        match spawned {
            Ok(_) => Some(rx),
            Err(e) => {
                tracing::warn!("firewall check not started: {e}");
                None
            }
        }
    }

    fn poll_firewall(&mut self) {
        let Some(s) = &mut self.phone.session else {
            return;
        };
        let Some(verdict) = s.firewall.as_ref().and_then(|rx| rx.try_recv().ok()) else {
            return;
        };
        s.firewall = None;
        tracing::info!("firewall for the phone server: {verdict:?}");
        let (name, private) = &s.network;
        s.warning = match verdict {
            Firewall::Open => None,
            Firewall::NoRule if *private => Some(
                "Windows Firewall will block your phone: pillbox.exe is not allowed in yet. \
                 Allow it once (README, One-time permissions)."
                    .into(),
            ),
            Firewall::NoRule => Some(format!(
                "Windows Firewall will block your phone: {name} is a Public network. Make it \
                 Private and allow pillbox.exe (README, One-time permissions)."
            )),
            Firewall::Blocked => Some(
                "Windows Firewall blocks pillbox.exe (a block rule, or \"block all incoming\"). \
                 See README, One-time permissions."
                    .into(),
            ),
        };
        self.dirty = true;
    }

    /// What the server reported (called when a worker woke the UI thread).
    pub(super) fn poll_phone(&mut self) {
        self.poll_firewall();
        let Some(events) = self.phone.session.as_ref().map(|s| s.server.poll()) else {
            return;
        };
        for e in events {
            match e {
                PhoneEvent::Opened { remote } => {
                    // A phone got through, whatever the rule check thought.
                    if remote && let Some(s) = &mut self.phone.session {
                        s.warning = None;
                    }
                    self.phone_status("Phone connected");
                }
                PhoneEvent::Downloaded => {
                    tracing::info!("phone downloaded the file");
                    self.phone_status("Sent to your phone ✓");
                }
                PhoneEvent::Receiving => self.phone_status("Receiving…"),
                PhoneEvent::Received(paths) => self.on_phone_files(paths),
                PhoneEvent::Text(text) => self.on_phone_text(&text),
                PhoneEvent::Failed(msg) => self.phone_status(&msg),
                PhoneEvent::Guessing => {
                    tracing::warn!("phone: too many wrong tokens; closing the code");
                    self.phone_close("too many wrong links");
                    self.show_toast(
                        Toast {
                            alert: true,
                            ..Toast::icon(G_PHONE, "Phone link closed: someone was guessing")
                        },
                        3500,
                    );
                }
            }
        }
    }

    fn phone_status(&mut self, text: &str) {
        if let Some(s) = &mut self.phone.session {
            s.status = text.to_string();
            self.dirty = true;
        }
    }

    fn on_phone_files(&mut self, paths: Vec<PathBuf>) {
        let n = paths.len();
        tracing::info!("phone sent {n} file(s)");
        let what = format!("{n} file{} from your phone", if n == 1 { "" } else { "s" });
        self.phone_status(&format!("Got {what} ✓"));
        if self.settings.phone.received_to_shelf {
            for p in &paths {
                self.request_file_thumb(p, 64);
            }
            self.shelf_add(paths);
        }
    }

    fn on_phone_text(&mut self, text: &str) {
        tracing::info!("phone sent text ({} chars)", text.chars().count());
        self.phone.pending_text = Some((text.to_string(), 0));
        self.copy_phone_text();
    }

    /// Copy the phone's text. Another app may hold the clipboard for a moment ("Access is
    /// denied"), so a failure is retried every 250 ms for up to 5 s, without blocking.
    pub(super) fn copy_phone_text(&mut self) {
        self.window.kill_timer(TIMER_PHONE_CLIP);
        let Some((text, tries)) = self.phone.pending_text.take() else {
            return;
        };
        match clipboard::write(self.window.hwnd(), &clipboard::Content::Text(text.clone())) {
            Ok(()) => {
                if tries > 0 {
                    tracing::info!("phone text copied after {tries} retries");
                }
                self.phone_status("Text from your phone copied ✓");
                self.show_toast(
                    Toast::icon(G_PHONE, "Text from your phone copied"),
                    TOAST_MS,
                );
            }
            Err(e) if tries < 20 => {
                tracing::debug!("phone text: clipboard busy ({e:#}); retrying");
                self.phone.pending_text = Some((text, tries + 1));
                self.window.set_timer(TIMER_PHONE_CLIP, 250);
            }
            Err(e) => {
                tracing::warn!("phone text not copied: {e:#}");
                self.phone_status("Text arrived but the clipboard was busy");
            }
        }
    }
}
