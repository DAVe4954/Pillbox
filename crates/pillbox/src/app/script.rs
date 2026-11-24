//! M7: the local script API, flashes on the closed pill, and the Activity tab's state. The
//! server (`crate::script`) runs on its own thread and only sends [`Inbound`] messages; they
//! are applied here, on the UI thread, when the thread wakes it.

use std::{
    os::windows::process::CommandExt,
    path::Path,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use pillbox_core::script::{
    AgentState, Board, Command, Flash, FlashQueue, TokenProblem, display_name,
};
use tokio::sync::oneshot;

use super::{App, Mode, TIMER_TOAST, TOAST_MS, Toast};
use crate::{
    platform::foreground,
    scene::{Action, Tab},
    script::{Inbound, ScriptServer},
    store, tabs,
};

/// Waiting permission questions that ran out of time.
pub(super) const TIMER_REPLY: usize = 50;
/// While the Activity tab is open: is Claude still running?
pub(super) const TIMER_PRESENCE: usize = 51;
const PRESENCE_MS: u32 = 3000;
/// Whether an agent or a script ever reported (see `Script::seen`).
const SEEN_FILE: &str = "agents_seen.json";

/// Keeps a console window from flashing up when Pillbox starts a helper program.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

struct Reply {
    ask: u64,
    tx: oneshot::Sender<bool>,
    deadline: Instant,
}

#[derive(Default)]
pub(super) struct Script {
    server: Option<ScriptServer>,
    pub board: Board,
    /// An agent or a script has reported to Pillbox at some time: the Activity tab shows.
    /// Kept in `agents_seen.json`.
    pub seen: bool,
    flashes: FlashQueue,
    /// Source of the flash on the pill right now ("" = it has none); `None` = no flash showing.
    pub showing_flash: Option<String>,
    replies: Vec<Reply>,
    /// The session whose Stop button waits for a second click.
    pub confirm_stop: Option<u64>,
    /// The session whose timeline is open.
    pub expanded: Option<u64>,
    /// A `claude.exe` runs: the Claude desktop app (also when only in the tray) or Claude Code
    /// in a terminal. Checked when the Activity tab opens and every 3 s while it is open.
    pub claude_running: bool,
    /// When the account usage was last asked for, and whether an answer is still coming.
    usage_asked: Option<Instant>,
    usage_busy: bool,
    /// The last account usage request worked (logged once per good stretch).
    usage_ok: bool,
    /// Why the last account usage request gave no numbers ("Sign in to Claude again").
    pub usage_problem: Option<&'static str>,
}

impl Script {
    pub fn load() -> Self {
        Self {
            seen: store::load_json(SEEN_FILE),
            ..Self::default()
        }
    }
}

/// Account usage is asked for at most this often.
const USAGE_EVERY: Duration = Duration::from_secs(60);
/// After a failure, wait this long before asking again (sign-in problems do not fix
/// themselves in a minute, and the endpoint answers 429 to clients that hurry).
const USAGE_RETRY: Duration = Duration::from_secs(5 * 60);

/// Ask Anthropic for the account's usage with Claude Code's own sign-in. Blocking; runs on a
/// worker thread. The token is read fresh each time (Claude Code renews it) and never stored,
/// logged or written back.
fn ask_account_usage() -> Result<pillbox_core::script::Usage, &'static str> {
    let path = std::env::var_os("USERPROFILE")
        .map(|h| Path::new(&h).join(".claude").join(".credentials.json"))
        .ok_or("No Claude sign-in found")?;
    let text = std::fs::read_to_string(&path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            "No Claude sign-in found"
        } else {
            tracing::info!("account usage: reading the sign-in failed: {}", e.kind());
            "Could not read the Claude sign-in"
        }
    })?;
    let token = pillbox_core::script::access_token(&text).map_err(|p| {
        // Where it went wrong, not what the file says: it holds secrets.
        tracing::info!("account usage: sign-in file not understood: {p:?}");
        match p {
            // The desktop app keeps its own sign-in elsewhere and leaves this file without a
            // token; only Claude Code's own `/login` fills it.
            TokenProblem::NoToken | TokenProblem::NoOauth | TokenProblem::Empty => {
                "No Claude Code sign-in here: run claude in a terminal, then /login"
            }
            _ => "Claude sign-in not understood",
        }
    })?;
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(10))
        .user_agent("Pillbox/0.1 (personal desktop app)")
        .build();
    let answer = agent
        .get(pillbox_core::script::ACCOUNT_USAGE_URL)
        .set("Authorization", &format!("Bearer {token}"))
        .set("anthropic-beta", pillbox_core::script::ACCOUNT_USAGE_BETA)
        .set("Accept", "application/json")
        .call();
    let body = match answer {
        Ok(r) => r.into_string().map_err(|_| "Usage answer was cut off")?,
        Err(ureq::Error::Status(401 | 403, _)) => {
            return Err("Sign-in expired: open Claude Code to renew it");
        }
        Err(ureq::Error::Status(429, _)) => return Err("Anthropic asked to wait; retrying soon"),
        Err(ureq::Error::Status(code, _)) => {
            tracing::warn!("account usage: HTTP {code}");
            return Err("Usage not available right now");
        }
        Err(ureq::Error::Transport(e)) => {
            tracing::info!("account usage: {}", e.kind());
            return Err("Could not reach Anthropic");
        }
    };
    let v: serde_json::Value =
        serde_json::from_str(&body).map_err(|_| "Usage answer not understood")?;
    pillbox_core::script::account_usage(&v, now_ms()).ok_or("Usage answer not understood")
}

pub(super) fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(0))
}

impl App {
    /// Start or stop the server to match the settings (called at start and on every reload).
    pub(super) fn sync_script(&mut self) {
        if self.settings.script.enabled {
            if self.script.server.is_none() {
                let started = store::data_dir()
                    .and_then(|dir| ScriptServer::start(&dir, self.window.waker()));
                match started {
                    Ok(server) => self.script.server = Some(server),
                    Err(e) => tracing::warn!("the script server did not start: {e:#}"),
                }
            }
            return;
        }
        if self.script.server.take().is_some() {
            tracing::info!("script API turned off");
        }
        self.script.board = Board::default();
        self.script.flashes = FlashQueue::default();
        self.script.replies.clear();
        self.script.showing_flash = None;
        if self.tab == Tab::Activity {
            self.tab = Tab::Home;
        }
        self.dirty = true;
    }

    /// What the server received (called when a worker woke the UI thread).
    pub(super) fn poll_script(&mut self) {
        let Some(items) = self.script.server.as_ref().map(ScriptServer::poll) else {
            return;
        };
        for item in items {
            match item {
                Inbound::Cmd(cmd) => self.on_command(cmd, None),
                Inbound::Permission(cmd, tx) => self.on_command(cmd, Some(tx)),
            }
        }
    }

    /// The first report ever of an agent or a script: from now on the Activity tab shows.
    fn note_agent_seen(&mut self) {
        if self.script.seen {
            return;
        }
        self.script.seen = true;
        tracing::info!("first agent or script report: the Activity tab shows from now on");
        if let Err(e) = store::save_json(SEEN_FILE, &true) {
            tracing::warn!("agents_seen.json not saved: {e:#}");
        }
        // The tab row gets one more icon.
        self.retarget();
        self.dirty = true;
    }

    fn on_command(&mut self, cmd: Command, reply: Option<oneshot::Sender<bool>>) {
        let now = now_ms();
        let hidden = &self.settings.script.hidden_agents;
        let reports = match &cmd {
            Command::Flash(_) => false,
            Command::Status(_) => true,
            Command::Statusline(agent, _) => !hidden.contains(agent),
            Command::Agent(u) => !hidden.contains(&u.agent),
        };
        if reports {
            self.note_agent_seen();
        }
        match cmd {
            Command::Flash(f) => self.on_flash(f),
            Command::Status(r) => {
                if self.script.board.set_status(&r, now) {
                    self.dirty = true;
                }
            }
            Command::Statusline(agent, info) => {
                if self.script.board.apply_statusline(&agent, &info) {
                    self.dirty = true;
                }
            }
            Command::Agent(u) => {
                if self.settings.script.hidden_agents.contains(&u.agent) {
                    return;
                }
                let before = self.script.board.state_of(&u.agent, &u.session);
                if !self.script.board.apply_agent(&u, now) {
                    return;
                }
                self.dirty = true;
                let after = self.script.board.state_of(&u.agent, &u.session);
                if u.ask.is_some() {
                    self.settle_ask(&u.agent, &u.session, reply);
                }
                if before != after && self.settings.script.agent_flashes {
                    // The closed pill is narrow (about 20 characters): the short name only.
                    let name = match u.agent.trim().to_lowercase().as_str() {
                        "claude" => "Claude".to_string(),
                        _ => display_name(&u.agent),
                    };
                    let text = match after {
                        Some(AgentState::Done) if before == Some(AgentState::Working) => {
                            format!("{name} is done")
                        }
                        Some(AgentState::NeedsYou) => format!("{name} needs you"),
                        _ => return,
                    };
                    self.on_flash(Flash {
                        text,
                        source: format!("agent:{}:{}", u.agent, u.session),
                        ..Flash::default()
                    });
                }
            }
        }
    }

    /// A permission question arrived (or not): keep its answer channel if Pillbox answers
    /// for agents, otherwise let the agent ask in its own window and remove the buttons.
    fn settle_ask(&mut self, agent: &str, session: &str, reply: Option<oneshot::Sender<bool>>) {
        let key = pillbox_core::script::agent_key(agent).unwrap_or_default();
        let session = pillbox_core::script::session_key(session);
        let ask = self
            .script
            .board
            .agents
            .iter()
            .find(|a| a.agent == key && a.session == session)
            .and_then(|a| a.ask.as_ref().map(|q| q.id));
        let Some(ask) = ask else { return };
        match reply {
            Some(tx) if self.settings.script.answer_permissions => {
                let wait = Duration::from_secs(u64::from(self.settings.script.answer_seconds));
                self.script.replies.push(Reply {
                    ask,
                    tx,
                    deadline: Instant::now() + wait,
                });
                self.window.set_timer(
                    TIMER_REPLY,
                    u32::try_from(wait.as_millis()).unwrap_or(u32::MAX).max(1),
                );
            }
            // No one to answer for: the agent asks in its own window.
            _ => {
                self.script.board.clear_ask(ask, false);
            }
        }
    }

    /// Permission questions whose time ran out: dropping the channel makes the server answer
    /// "no decision", so the agent asks in its own window.
    pub(super) fn expire_replies(&mut self) {
        self.window.kill_timer(TIMER_REPLY);
        let now = Instant::now();
        let mut gone = Vec::new();
        self.script.replies.retain(|r| {
            let live = r.deadline > now && !r.tx.is_closed();
            if !live {
                gone.push(r.ask);
            }
            live
        });
        for ask in gone {
            self.script.board.clear_ask(ask, false);
            self.dirty = true;
        }
        if let Some(next) = self.script.replies.iter().map(|r| r.deadline).min() {
            let ms = next.saturating_duration_since(now).as_millis();
            self.window
                .set_timer(TIMER_REPLY, u32::try_from(ms).unwrap_or(u32::MAX).max(50));
        }
    }

    /// Finished work leaves the Activity tab after a while; runs once a minute.
    pub(super) fn script_housekeeping(&mut self) {
        let s = &self.settings.script;
        let (done, stale) = (
            i64::from(s.keep_minutes) * 60_000,
            i64::from(s.stale_minutes) * 60_000,
        );
        if self.script.board.expire(now_ms(), done, stale) {
            self.dirty = true;
        }
        if self.tab == Tab::Activity {
            // "5 min ago" labels move on.
            self.dirty = true;
        }
    }

    // ---- flashes --------------------------------------------------------------------------------

    fn on_flash(&mut self, f: Flash) {
        let quiet = self.mode != Mode::Normal || self.fullscreen.is_some();
        if quiet && !f.urgent {
            return;
        }
        let replaces_showing =
            !f.source.is_empty() && self.script.showing_flash.as_deref() == Some(f.source.as_str());
        if replaces_showing {
            self.show_flash(f);
            return;
        }
        self.script.flashes.push(f);
        self.flash_pump();
    }

    /// Show the next waiting flash if the pill is free.
    pub(super) fn flash_pump(&mut self) {
        if self.toast.is_some() {
            return;
        }
        if self.mode != Mode::Normal || self.fullscreen.is_some() {
            // Game mode came on while flashes were waiting: only urgent ones still show.
            self.script.flashes.drop_quiet();
        }
        if let Some(f) = self.script.flashes.pop() {
            self.show_flash(f);
        }
    }

    fn show_flash(&mut self, f: Flash) {
        let ms =
            f.ms.unwrap_or(self.settings.script.flash_seconds.saturating_mul(1000));
        self.show_toast(Toast::text(f.text), ms);
        self.script.showing_flash = Some(f.source);
    }

    /// A click on the closed pill while a flash shows. A flash about Claude Code opens the
    /// Claude desktop app; any other opens the Activity tab.
    pub(super) fn click_flash(&mut self) -> bool {
        let Some(source) = self.script.showing_flash.clone() else {
            return false;
        };
        if self.open {
            return false;
        }
        if source.starts_with("agent:claude:") {
            // The flash did its job: take it off the pill.
            self.window.kill_timer(TIMER_TOAST);
            self.toast = None;
            self.script.showing_flash = None;
            self.flash_pump();
            self.retarget();
            self.dirty = true;
            self.open_claude_app();
            return true;
        }
        if !self.summon_tab(Tab::Activity) {
            return false;
        }
        self.hovering = true;
        self.set_open(true);
        self.tab_shown();
        true
    }

    // ---- whether Claude runs ----------------------------------------------------------------------

    /// Look whether Claude runs (one process-list snapshot). When it was quit, its sessions are
    /// over: Claude sends no "session end" when its window is closed.
    pub(super) fn refresh_agent_presence(&mut self) {
        // The card's icon: the Claude app's own, loaded once through the shell.
        self.request_app_icon(tabs::CLAUDE_AUMID, None);
        // Developer aid: PILLBOX_FAKE_CLAUDE=0 / 1 pretends Claude is closed / running, so the
        // Offline card can be checked without quitting the real app.
        let running = match std::env::var("PILLBOX_FAKE_CLAUDE").as_deref() {
            Ok("0") => false,
            Ok("1") => true,
            _ => foreground::process_running("claude.exe"),
        };
        if running == self.script.claude_running {
            self.maybe_fetch_account_usage();
            return;
        }
        self.script.claude_running = running;
        tracing::info!(
            "Claude is {}",
            if running { "running" } else { "not running" }
        );
        if !running && self.script.board.end_kind("claude") {
            self.script.expanded = None;
            self.script.confirm_stop = None;
        }
        self.dirty = true;
        self.maybe_fetch_account_usage();
    }

    /// Ask for the account's usage if the setting is on, Claude runs and the last answer is
    /// old enough. Called by the presence check (every 3 s while the Activity tab is open), so
    /// nothing is sent while the tab is closed.
    pub(super) fn maybe_fetch_account_usage(&mut self) {
        if !self.settings.script.claude_account_usage
            || !self.script.claude_running
            || self.script.usage_busy
        {
            return;
        }
        let wait = if self.script.usage_problem.is_some() {
            USAGE_RETRY
        } else {
            USAGE_EVERY
        };
        if self.script.usage_asked.is_some_and(|t| t.elapsed() < wait) {
            return;
        }
        self.script.usage_asked = Some(Instant::now());
        self.script.usage_busy = true;
        let tx = self.jobs_tx.clone();
        let waker = self.window.waker();
        let started = std::thread::Builder::new()
            .name("account-usage".into())
            .spawn(move || {
                let _ = tx.send(super::Job::AccountUsage(ask_account_usage()));
                waker.wake();
            });
        if started.is_err() {
            self.script.usage_busy = false;
        }
    }

    pub(super) fn on_account_usage(
        &mut self,
        result: Result<pillbox_core::script::Usage, &'static str>,
    ) {
        self.script.usage_busy = false;
        match result {
            Ok(usage) => {
                if !self.script.usage_ok {
                    tracing::info!("account usage received");
                }
                self.script.usage_ok = true;
                self.script.board.set_usage("claude", usage);
                self.script.usage_problem = None;
            }
            Err(why) => {
                if self.script.usage_problem != Some(why) {
                    tracing::info!("account usage: {why}");
                }
                self.script.usage_ok = false;
                self.script.usage_problem = Some(why);
            }
        }
        self.dirty = true;
    }

    /// The presence check runs only while the Activity tab is on screen.
    pub(super) fn sync_presence_timer(&mut self) {
        if self.open && self.tab == Tab::Activity {
            self.window.set_timer(TIMER_PRESENCE, PRESENCE_MS);
        } else {
            self.window.kill_timer(TIMER_PRESENCE);
        }
    }

    // ---- buttons on the Activity tab ------------------------------------------------------------

    pub(super) fn act_activity(&mut self, action: Action) {
        match action {
            Action::AgentAllow(id) => self.answer_ask(id, true),
            Action::AgentDeny(id) => self.answer_ask(id, false),
            Action::AgentStop(uid) => self.agent_stop(uid),
            Action::AgentNewChat(uid) => self.agent_new_chat(uid),
            Action::AgentOpen(_) => self.open_claude_app(),
            Action::AgentExpand(uid) => {
                self.script.expanded = (self.script.expanded != Some(uid)).then_some(uid);
                self.dirty = true;
            }
            Action::ActivityDismiss(uid) => {
                // A question still waiting is dropped, so the agent asks for itself.
                let ask = self
                    .script
                    .board
                    .agent(uid)
                    .and_then(|a| a.ask.as_ref().map(|q| q.id));
                if let Some(ask) = ask {
                    self.script.replies.retain(|r| r.ask != ask);
                }
                if self.script.board.dismiss(uid) {
                    if self.script.expanded == Some(uid) {
                        self.script.expanded = None;
                    }
                    self.dirty = true;
                }
            }
            _ => {}
        }
    }

    /// Bring the Claude desktop app to the front (starting it if needed). It is a packaged
    /// (Store-style) app, started through the shell by its AppUserModelId (`Get-StartApps`
    /// shows it); a second start only activates the running window.
    fn open_claude_app(&mut self) {
        match std::process::Command::new("explorer.exe")
            .arg(format!("shell:AppsFolder\\{}", tabs::CLAUDE_AUMID))
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
        {
            Ok(_) => tracing::info!("opened the Claude desktop app"),
            Err(e) => {
                tracing::warn!("could not open the Claude desktop app: {e}");
                self.show_toast(Toast::text("Could not open Claude"), TOAST_MS);
            }
        }
    }

    fn answer_ask(&mut self, ask: u64, allow: bool) {
        let pos = self.script.replies.iter().position(|r| r.ask == ask);
        if let Some(pos) = pos {
            let r = self.script.replies.remove(pos);
            // The hook may have given up meanwhile (the agent was answered in its own window).
            if r.tx.send(allow).is_err() {
                tracing::info!("permission answer came too late; the agent no longer waits");
            }
        }
        self.script.board.clear_ask(ask, true);
        tracing::info!(
            "permission question answered: {}",
            if allow { "allow" } else { "deny" }
        );
        self.dirty = true;
    }

    /// Stop the agent's process (and what it started). Asks to be clicked twice, and only
    /// touches a process that still is what it was when Pillbox first saw it.
    fn agent_stop(&mut self, uid: u64) {
        if self.script.confirm_stop != Some(uid) {
            self.script.confirm_stop = Some(uid);
            self.dirty = true;
            return;
        }
        self.script.confirm_stop = None;
        self.dirty = true;
        let Some((agent, pid)) = self
            .script
            .board
            .agent(uid)
            .and_then(|a| a.pid.map(|p| (a.agent.clone(), p)))
        else {
            return;
        };
        let exe = foreground::exe_of_pid(pid);
        let expected = match agent.as_str() {
            "claude" => Some("claude.exe"),
            "codex" => Some("codex.exe"),
            _ => None,
        };
        // Programs Windows itself depends on are never stopped from here, whatever a script
        // said its process id was.
        const NEVER: [&str; 9] = [
            "explorer.exe",
            "svchost.exe",
            "winlogon.exe",
            "csrss.exe",
            "wininit.exe",
            "lsass.exe",
            "services.exe",
            "dwm.exe",
            "pillbox.exe",
        ];
        let ok = match (&exe, expected) {
            (None, _) => false,
            (Some(name), Some(want)) => name == want,
            // A custom agent: the script gave the process id.
            (Some(name), None) => !NEVER.contains(&name.as_str()) && pid != std::process::id(),
        };
        if !ok {
            tracing::warn!("not stopping pid {pid} for {agent}: it is {exe:?} now");
            self.show_toast(Toast::text("That agent is no longer running"), TOAST_MS);
            return;
        }
        let result = std::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .creation_flags(CREATE_NO_WINDOW)
            .spawn();
        match result {
            Ok(_) => {
                tracing::info!("stopped {agent} (pid {pid})");
                self.show_toast(
                    Toast::text(format!("Stopped {}", display_name(&agent))),
                    TOAST_MS,
                );
            }
            Err(e) => {
                tracing::warn!("taskkill failed to start: {e}");
                self.show_toast(Toast::text("Could not stop it"), TOAST_MS);
            }
        }
    }

    /// A new chat of the same agent, in a terminal, in the same folder.
    fn agent_new_chat(&mut self, uid: u64) {
        let Some((agent, cwd)) = self
            .script
            .board
            .agent(uid)
            .map(|a| (a.agent.clone(), a.cwd.clone()))
        else {
            return;
        };
        if !tabs::can_new_chat(&agent) {
            return;
        }
        if !Path::new(&cwd).is_dir() {
            return self.show_toast(Toast::text("That folder is gone"), TOAST_MS);
        }
        // Windows Terminal if it is there; otherwise a plain console window.
        let wt = std::process::Command::new("wt.exe")
            .args(["-d", &cwd, &agent])
            .spawn();
        let started = wt.or_else(|_| {
            std::process::Command::new("cmd.exe")
                .args(["/c", "start", "", "/D", &cwd, &agent])
                .creation_flags(CREATE_NO_WINDOW)
                .spawn()
        });
        match started {
            Ok(_) => tracing::info!("new {agent} chat opened"),
            Err(e) => {
                tracing::warn!("new chat: {e}");
                self.show_toast(Toast::text("Could not open a terminal"), TOAST_MS);
            }
        }
    }
}
