//! M7: the logic behind the local script API and the agent list. No I/O in here.
//!
//! Vocabulary (see GLOSSARY.md): a **Flash** is a short line on the closed pill; a **Status** is
//! a live entry on the Activity tab (a build, a test run); an **Agent** is a coding assistant
//! whose **Sessions** (one per chat) are shown as live entries too.
//!
//! The server in the app turns every request into a [`Command`] with [`parse_request`], the
//! app applies it to a [`Board`], and the Activity tab draws the board. Hooks of Claude Code
//! and Codex arrive as raw JSON and are mapped by [`hook_update`].

use std::collections::{HashMap, VecDeque};

use serde::Deserialize;
use serde_json::Value;

/// Most sessions and statuses kept; the oldest go first (a runaway client cannot grow memory).
pub const MAX_ENTRIES: usize = 40;
/// Most steps kept in one session's timeline.
pub const MAX_STEPS: usize = 12;
/// Most flashes waiting behind the one on screen.
pub const MAX_FLASHES: usize = 3;
/// Longest request body accepted, bytes.
pub const MAX_BODY: usize = 256 * 1024;

// ---- text helpers ------------------------------------------------------------------------------

/// Whitespace collapsed to single spaces, control characters dropped, cut to `max` characters
/// (with an ellipsis when something was cut).
pub fn tidy(s: &str, max: usize) -> String {
    let mut out = String::new();
    let mut count = 0usize;
    let mut space = false;
    for c in s.chars() {
        if c.is_whitespace() {
            space = !out.is_empty();
            continue;
        }
        if c.is_control() {
            continue;
        }
        if space {
            out.push(' ');
            count += 1;
            space = false;
        }
        if count >= max {
            out.push('…');
            return out;
        }
        out.push(c);
        count += 1;
    }
    out
}

/// The last part of a path, either slash style.
pub fn base_name(path: &str) -> &str {
    let p = path.trim_end_matches(['/', '\\']);
    p.rsplit(['/', '\\']).next().unwrap_or(p)
}

/// A name from a client made safe to use as a key: trimmed, lower case, only letters, digits,
/// `.`, `_`, `-` and spaces, at most 24 characters. `None` when nothing is left.
pub fn agent_key(name: &str) -> Option<String> {
    let k: String = name
        .trim()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | ' '))
        .take(24)
        .collect::<String>()
        .trim()
        .to_lowercase();
    (!k.is_empty()).then_some(k)
}

/// A session id as stored; an empty one means the agent's only session.
pub fn session_key(session: &str) -> String {
    if session.trim().is_empty() {
        "default".to_string()
    } else {
        tidy(session, 64)
    }
}

/// How an agent is named on the card.
pub fn display_name(key: &str) -> String {
    match key {
        "claude" => "Claude Code".into(),
        "codex" => "Codex".into(),
        "antigravity" => "Antigravity".into(),
        "hermes" => "Hermes".into(),
        _ => {
            let mut c = key.chars();
            c.next()
                .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
                .unwrap_or_default()
        }
    }
}

// ---- flashes -----------------------------------------------------------------------------------

/// A short line for the closed pill.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(default)]
pub struct Flash {
    pub text: String,
    /// Who sent it. A new flash from the same source replaces the waiting one.
    pub source: String,
    /// How long to show it; `None` = the setting.
    pub ms: Option<u32>,
    /// Show it even in game mode.
    pub urgent: bool,
}

impl Flash {
    fn tidy(mut self) -> Option<Self> {
        self.text = tidy(&self.text, 80);
        self.source = tidy(&self.source, 32);
        self.ms = self.ms.map(|m| m.clamp(500, 60_000));
        (!self.text.is_empty()).then_some(self)
    }
}

/// Flashes waiting for the pill: at most [`MAX_FLASHES`], a repeat from the same source
/// replaces the earlier one.
#[derive(Debug, Default)]
pub struct FlashQueue {
    q: VecDeque<Flash>,
}

impl FlashQueue {
    pub fn push(&mut self, f: Flash) {
        if !f.source.is_empty()
            && let Some(slot) = self.q.iter_mut().find(|x| x.source == f.source)
        {
            *slot = f;
            return;
        }
        self.q.push_back(f);
        while self.q.len() > MAX_FLASHES {
            self.q.pop_front();
        }
    }

    pub fn pop(&mut self) -> Option<Flash> {
        self.q.pop_front()
    }

    pub fn len(&self) -> usize {
        self.q.len()
    }

    pub fn is_empty(&self) -> bool {
        self.q.is_empty()
    }

    /// Drop the waiting flashes that are not urgent (game mode came on).
    pub fn drop_quiet(&mut self) {
        self.q.retain(|f| f.urgent);
    }
}

// ---- statuses ----------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StatusState {
    #[default]
    Running,
    #[serde(alias = "success", alias = "done", alias = "passed")]
    Ok,
    #[serde(alias = "fail", alias = "error")]
    Failed,
    Info,
}

/// A request to create, update or remove a status.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(default)]
pub struct StatusReq {
    /// Stable name; the same id updates the entry in place.
    pub id: String,
    /// Fields left out keep what the entry already has (a new entry: title = id, no detail,
    /// "running", no bar). A finished state without a progress value takes the bar away.
    pub title: Option<String>,
    pub detail: Option<String>,
    pub state: Option<StatusState>,
    /// 0..=100 draws a bar.
    pub progress: Option<u8>,
    pub remove: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Status {
    /// Number for clicks to refer to; never reused while the app runs.
    pub uid: u64,
    pub id: String,
    pub title: String,
    pub detail: String,
    pub state: StatusState,
    pub progress: Option<u8>,
    pub updated_ms: i64,
}

// ---- agents ------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AgentState {
    /// Between chats, or no news for a long time.
    #[default]
    Idle,
    Working,
    NeedsYou,
    Done,
}

impl AgentState {
    /// Lower sorts first on the card: who needs you, then who works, then the rest.
    fn rank(self) -> u8 {
        match self {
            AgentState::NeedsYou => 0,
            AgentState::Working => 1,
            AgentState::Done => 2,
            AgentState::Idle => 3,
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().replace([' ', '_'], "-").as_str() {
            "working" | "busy" | "running" => Some(Self::Working),
            "needs-you" | "needs" | "waiting" | "blocked" | "input" => Some(Self::NeedsYou),
            "done" | "finished" | "idle-done" | "complete" => Some(Self::Done),
            "idle" | "ready" => Some(Self::Idle),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    pub at_ms: i64,
    pub text: String,
}

/// A permission question an agent is waiting on: Pillbox can answer Allow or Deny.
#[derive(Debug, Clone, PartialEq)]
pub struct Ask {
    pub id: u64,
    pub tool: String,
    pub what: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Agent {
    /// Number for clicks to refer to; never reused while the app runs.
    pub uid: u64,
    /// Lower-case key of the agent kind ("claude", "codex", a custom name).
    pub agent: String,
    pub session: String,
    pub cwd: String,
    pub project: String,
    pub state: AgentState,
    /// What it is doing right now.
    pub doing: String,
    /// How the last turn ended.
    pub summary: String,
    /// The prompt of the current turn.
    pub prompt: String,
    pub model: String,
    pub effort: String,
    pub started_ms: i64,
    pub updated_ms: i64,
    pub steps: VecDeque<Step>,
    /// Process to stop (the agent's own, found by `pillboxctl`).
    pub pid: Option<u32>,
    pub ask: Option<Ask>,
}

/// What one event changes in a session. `None` fields stay as they were.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(default)]
pub struct AgentUpdate {
    pub agent: String,
    pub session: String,
    pub cwd: Option<String>,
    pub state: Option<String>,
    pub doing: Option<String>,
    pub summary: Option<String>,
    pub prompt: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
    /// A line for the timeline.
    pub step: Option<String>,
    pub pid: Option<u32>,
    /// The session is over: it leaves the list.
    pub ended: bool,
    /// A permission question (tool name, what it wants to do); the state becomes "needs you".
    #[serde(skip)]
    pub ask: Option<(String, String)>,
}

/// Limits and context of one agent, from its status line.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Limit {
    pub used_pct: f32,
    pub resets_at_ms: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Usage {
    pub five_hour: Option<Limit>,
    pub seven_day: Option<Limit>,
    pub context_pct: Option<f32>,
    pub updated_ms: i64,
}

// ---- the board ---------------------------------------------------------------------------------

/// Everything the Activity tab shows.
#[derive(Debug, Default)]
pub struct Board {
    pub statuses: Vec<Status>,
    pub agents: Vec<Agent>,
    /// Latest limits per agent kind.
    pub usage: HashMap<String, Usage>,
    next_ask: u64,
    next_uid: u64,
}

impl Board {
    pub fn is_empty(&self) -> bool {
        self.statuses.is_empty() && self.agents.is_empty()
    }

    /// Create, update or remove a status. Returns whether anything changed.
    pub fn set_status(&mut self, req: &StatusReq, now_ms: i64) -> bool {
        let id = tidy(&req.id, 48);
        if id.is_empty() {
            return false;
        }
        if req.remove {
            let before = self.statuses.len();
            self.statuses.retain(|s| s.id != id);
            return self.statuses.len() != before;
        }
        let old = self.statuses.iter().find(|s| s.id == id);
        let title = req
            .title
            .as_deref()
            .map(|t| tidy(t, 60))
            .filter(|t| !t.is_empty())
            .or_else(|| old.map(|o| o.title.clone()))
            .unwrap_or_else(|| id.clone());
        let state = req
            .state
            .or_else(|| old.map(|o| o.state))
            .unwrap_or_default();
        let finished = matches!(state, StatusState::Ok | StatusState::Failed);
        let progress = match req.progress {
            Some(p) => Some(p.min(100)),
            None if finished && req.state.is_some() => None,
            None => old.and_then(|o| o.progress),
        };
        let mut status = Status {
            uid: 0,
            title,
            detail: req
                .detail
                .as_deref()
                .map(|d| tidy(d, 140))
                .or_else(|| old.map(|o| o.detail.clone()))
                .unwrap_or_default(),
            state,
            progress,
            updated_ms: now_ms,
            id,
        };
        if let Some(slot) = self.statuses.iter_mut().find(|s| s.id == status.id) {
            status.uid = slot.uid;
            *slot = status;
        } else {
            self.next_uid += 1;
            status.uid = self.next_uid;
            self.statuses.insert(0, status);
            self.statuses.truncate(MAX_ENTRIES);
        }
        true
    }

    fn find(&self, agent: &str, session: &str) -> Option<usize> {
        self.agents
            .iter()
            .position(|a| a.agent == agent && a.session == session)
    }

    /// Apply one agent event. Returns whether anything changed.
    pub fn apply_agent(&mut self, u: &AgentUpdate, now_ms: i64) -> bool {
        let Some(agent) = agent_key(&u.agent) else {
            return false;
        };
        let session = session_key(&u.session);
        let at = self.find(&agent, &session);
        if u.ended {
            return match at {
                Some(i) => {
                    self.agents.remove(i);
                    true
                }
                None => false,
            };
        }
        let i = match at {
            Some(i) => i,
            None => {
                self.next_uid += 1;
                self.agents.push(Agent {
                    uid: self.next_uid,
                    agent,
                    session,
                    cwd: String::new(),
                    project: String::new(),
                    state: AgentState::Idle,
                    doing: String::new(),
                    summary: String::new(),
                    prompt: String::new(),
                    model: String::new(),
                    effort: String::new(),
                    started_ms: now_ms,
                    updated_ms: now_ms,
                    steps: VecDeque::new(),
                    pid: None,
                    ask: None,
                });
                if self.agents.len() > MAX_ENTRIES {
                    // Forget the least recently heard of.
                    if let Some(old) = self
                        .agents
                        .iter()
                        .enumerate()
                        .filter(|(j, _)| *j + 1 != self.agents.len())
                        .min_by_key(|(_, a)| a.updated_ms)
                        .map(|(j, _)| j)
                    {
                        self.agents.remove(old);
                    }
                }
                self.agents.len() - 1
            }
        };
        let ask_id = u.ask.as_ref().map(|_| {
            self.next_ask += 1;
            self.next_ask
        });
        // The last element may have moved if an old one was removed above.
        let i = i.min(self.agents.len().saturating_sub(1));
        let Some(a) = self.agents.get_mut(i) else {
            return false;
        };
        a.updated_ms = now_ms;
        if let Some(cwd) = u.cwd.as_deref().filter(|c| !c.is_empty()) {
            a.cwd = tidy(cwd, 260);
            a.project = tidy(base_name(&a.cwd), 40);
        }
        if let Some(m) = u.model.as_deref().filter(|m| !m.is_empty()) {
            a.model = tidy(m, 40);
        }
        if let Some(e) = u.effort.as_deref().filter(|e| !e.is_empty()) {
            a.effort = tidy(e, 16);
        }
        if let Some(p) = u.pid {
            a.pid = Some(p);
        }
        if let Some(p) = u.prompt.as_deref() {
            a.prompt = tidy(p, 140);
        }
        if let Some(s) = u.summary.as_deref() {
            a.summary = tidy(s, 200);
        }
        if let Some(d) = u.doing.as_deref() {
            a.doing = tidy(d, 100);
        }
        if let Some(state) = u.state.as_deref().and_then(AgentState::parse) {
            if state == AgentState::Working && a.state != AgentState::Working {
                // A new turn: forget the last one's timeline and answer.
                if a.state == AgentState::Done || a.state == AgentState::Idle {
                    a.steps.clear();
                    a.summary.clear();
                }
            }
            a.state = state;
            if state != AgentState::NeedsYou {
                a.ask = None;
            }
        }
        if let (Some((tool, what)), Some(id)) = (&u.ask, ask_id) {
            a.state = AgentState::NeedsYou;
            a.ask = Some(Ask {
                id,
                tool: tidy(tool, 40),
                what: tidy(what, 120),
            });
        }
        if let Some(step) = u.step.as_deref() {
            let text = tidy(step, 100);
            if !text.is_empty() && a.steps.back().is_none_or(|s| s.text != text) {
                a.steps.push_back(Step {
                    at_ms: now_ms,
                    text,
                });
                while a.steps.len() > MAX_STEPS {
                    a.steps.pop_front();
                }
            }
        }
        true
    }

    /// The buttons for a permission question go away. `answered`: Pillbox answered it, so
    /// the agent carries on; otherwise nobody answered in time and the agent is asking in its
    /// own window.
    pub fn clear_ask(&mut self, id: u64, answered: bool) -> bool {
        let mut changed = false;
        for a in &mut self.agents {
            if a.ask.as_ref().is_some_and(|q| q.id == id) {
                a.ask = None;
                if a.state == AgentState::NeedsYou {
                    if answered {
                        a.state = AgentState::Working;
                        a.doing = "Continuing…".into();
                    } else {
                        a.doing = "Waiting for you in its own window".into();
                    }
                }
                changed = true;
            }
        }
        changed
    }

    /// The state of a session, if it is on the board.
    pub fn state_of(&self, agent: &str, session: &str) -> Option<AgentState> {
        let key = agent_key(agent)?;
        let session = session_key(session);
        self.find(&key, &session).map(|i| self.agents[i].state)
    }

    /// A status line payload: remember the limits; show the model and effort on the session it
    /// is for, if that session is on the board. It never creates a session and never counts as
    /// news from the agent (a status line ticks every second). Returns whether anything
    /// visible changed.
    pub fn apply_statusline(&mut self, agent: &str, info: &StatuslineInfo) -> bool {
        let Some(key) = agent_key(agent) else {
            return false;
        };
        let mut changed = false;
        let old = self.usage.get(&key).copied();
        // A payload without limits (Claude Code sends none before its first request of a
        // session) keeps the ones known, from an earlier status line or from the account.
        let new = merge_usage(old, info.usage);
        let seen = |l: Option<Limit>| l.map(|l| l.used_pct.round() as i32);
        if old.is_none_or(|o| {
            seen(o.five_hour) != seen(new.five_hour) || seen(o.seven_day) != seen(new.seven_day)
        }) {
            changed = true;
        }
        self.usage.insert(key.clone(), new);
        if let Some(i) = self.find(&key, &session_key(&info.session)) {
            let a = &mut self.agents[i];
            if !info.model.is_empty() && a.model != info.model {
                a.model = tidy(&info.model, 40);
                changed = true;
            }
            if !info.effort.is_empty() && a.effort != info.effort {
                a.effort = tidy(&info.effort, 16);
                changed = true;
            }
        }
        changed
    }

    pub fn agent(&self, uid: u64) -> Option<&Agent> {
        self.agents.iter().find(|a| a.uid == uid)
    }

    /// The session waiting on this permission question.
    pub fn agent_asking(&self, ask_id: u64) -> Option<&Agent> {
        self.agents
            .iter()
            .find(|a| a.ask.as_ref().is_some_and(|q| q.id == ask_id))
    }

    /// Take a session or status off the card by its click number. Returns whether it existed.
    pub fn dismiss(&mut self, uid: u64) -> bool {
        let (a, s) = (self.agents.len(), self.statuses.len());
        self.agents.retain(|x| x.uid != uid);
        self.statuses.retain(|x| x.uid != uid);
        self.agents.len() != a || self.statuses.len() != s
    }

    /// How many permission questions wait for an answer.
    pub fn pending_asks(&self) -> usize {
        self.agents.iter().filter(|a| a.ask.is_some()).count()
    }

    /// Latest limits for an agent kind.
    pub fn set_usage(&mut self, agent: &str, usage: Usage) {
        if let Some(k) = agent_key(agent) {
            self.usage.insert(k, usage);
        }
    }

    /// Finished work leaves after `done_ms`; a "working" agent that has said nothing for
    /// `stale_ms` becomes idle (it probably crashed); a running status that was never closed
    /// goes after six hours. Returns whether anything changed.
    pub fn expire(&mut self, now_ms: i64, done_ms: i64, stale_ms: i64) -> bool {
        let (a0, s0) = (self.agents.len(), self.statuses.len());
        let mut changed = false;
        self.agents
            .retain(|a| !(a.state == AgentState::Done && now_ms - a.updated_ms > done_ms));
        for a in &mut self.agents {
            if a.state == AgentState::Working && now_ms - a.updated_ms > stale_ms {
                a.state = AgentState::Idle;
                a.doing = "No news for a while".into();
                changed = true;
            }
        }
        self.statuses.retain(|s| {
            let limit = if s.state == StatusState::Running {
                6 * 60 * 60 * 1000
            } else {
                done_ms
            };
            now_ms - s.updated_ms <= limit
        });
        changed || self.agents.len() != a0 || self.statuses.len() != s0
    }

    /// Sessions in card order: needs you, working, done, idle; newest first within a group.
    pub fn ordered_agents(&self) -> Vec<&Agent> {
        let mut v: Vec<&Agent> = self.agents.iter().collect();
        v.sort_by(|a, b| {
            a.state
                .rank()
                .cmp(&b.state.rank())
                .then(b.updated_ms.cmp(&a.updated_ms))
        });
        v
    }

    /// How many sessions are in each state: (needs you, working, done).
    pub fn counts(&self) -> (usize, usize, usize) {
        let n = |s| self.agents.iter().filter(|a| a.state == s).count();
        (
            n(AgentState::NeedsYou),
            n(AgentState::Working),
            n(AgentState::Done),
        )
    }

    /// The agent's program was closed: its sessions are over (it sends no "session end" when
    /// it is quit). Questions still open lose their buttons. Returns whether any left.
    pub fn end_kind(&mut self, kind: &str) -> bool {
        let before = self.agents.len();
        self.agents.retain(|a| a.agent != kind);
        self.agents.len() != before
    }

    /// The one-card summary of an agent kind: Offline when its program is not running and no
    /// session reports, else the most urgent state of its sessions (needs you, working), else
    /// Idle. `lead` is the session the card's line is about.
    pub fn kind_summary(&self, kind: &str, program_running: bool) -> KindSummary<'_> {
        let mine: Vec<&Agent> = self.agents.iter().filter(|a| a.agent == kind).collect();
        let newest = |state: AgentState| {
            mine.iter()
                .filter(|a| a.state == state)
                .max_by_key(|a| a.updated_ms)
                .copied()
        };
        let (status, lead) = if let Some(a) = newest(AgentState::NeedsYou) {
            (KindStatus::NeedsYou, Some(a))
        } else if let Some(a) = newest(AgentState::Working) {
            (KindStatus::Working, Some(a))
        } else if program_running || !mine.is_empty() {
            (KindStatus::Idle, newest(AgentState::Done))
        } else {
            (KindStatus::Offline, None)
        };
        let working = mine
            .iter()
            .filter(|a| matches!(a.state, AgentState::Working | AgentState::NeedsYou))
            .count();
        KindSummary {
            status,
            lead,
            working,
        }
    }
}

/// New usage over old: each limit the new reading carries replaces the old one, a missing one
/// keeps the old. "Updated" moves only when the new reading had a limit, so old numbers still
/// show as old.
pub fn merge_usage(old: Option<Usage>, new: Usage) -> Usage {
    let Some(old) = old else {
        return new;
    };
    let has_limit = new.five_hour.is_some() || new.seven_day.is_some();
    Usage {
        five_hour: new.five_hour.or(old.five_hour),
        seven_day: new.seven_day.or(old.seven_day),
        context_pct: new.context_pct.or(old.context_pct),
        updated_ms: if has_limit {
            new.updated_ms
        } else {
            old.updated_ms
        },
    }
}

/// What the Activity tab's agent card says about an agent kind as a whole.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KindStatus {
    /// Its program is closed (and not in the background).
    Offline,
    /// Running, nothing to do.
    Idle,
    Working,
    NeedsYou,
}

impl KindStatus {
    pub fn label(self) -> &'static str {
        match self {
            KindStatus::Offline => "Offline",
            KindStatus::Idle => "Idle",
            KindStatus::Working => "Working",
            KindStatus::NeedsYou => "Needs you",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KindSummary<'a> {
    pub status: KindStatus,
    /// The session the line is about: the one that needs you, the newest working one, or the
    /// last one that finished.
    pub lead: Option<&'a Agent>,
    /// Sessions working or waiting.
    pub working: usize,
}

impl KindSummary<'_> {
    /// The card's second line: what it is doing ("Editing tools.rs · Dynamic Island"), what it
    /// waits for, when it last finished, or that the program is closed.
    pub fn line(&self, now_ms: i64) -> String {
        let place = |a: &Agent| {
            if a.project.is_empty() {
                String::new()
            } else {
                format!(" · {}", a.project)
            }
        };
        let more = if self.working > 1 {
            format!(" (+{} more)", self.working - 1)
        } else {
            String::new()
        };
        match (self.status, self.lead) {
            (KindStatus::Offline, _) => "Not running".into(),
            (KindStatus::NeedsYou, Some(a)) => {
                let what = a
                    .ask
                    .as_ref()
                    .map(|q| q.what.as_str())
                    .filter(|w| !w.is_empty())
                    .or(Some(a.doing.as_str()).filter(|d| !d.is_empty()))
                    .unwrap_or("A question");
                format!("Waiting for you: {what}{}{more}", place(a))
            }
            (KindStatus::Working, Some(a)) => {
                let doing = if a.doing.is_empty() {
                    "Thinking"
                } else {
                    a.doing.as_str()
                };
                format!("{doing}{}{more}", place(a))
            }
            (KindStatus::Idle, Some(a)) => {
                let e = elapsed(now_ms, a.updated_ms);
                let when = if e == "now" {
                    "just now".to_string()
                } else {
                    format!("{e} ago")
                };
                format!("Finished {when}{}", place(a))
            }
            _ => "Ready, nothing running".into(),
        }
    }
}

// ---- hooks -------------------------------------------------------------------------------------

fn str_of<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str)
}

/// One line saying what a tool call is about to do.
pub fn describe_tool(name: &str, input: &Value) -> String {
    let file = || {
        str_of(input, "file_path")
            .or_else(|| str_of(input, "path"))
            .or_else(|| str_of(input, "notebook_path"))
            .map(|p| base_name(p).to_string())
            .unwrap_or_default()
    };
    match name {
        "Bash" | "PowerShell" | "shell" | "local_shell" => {
            let cmd = str_of(input, "command")
                .or_else(|| str_of(input, "cmd"))
                .unwrap_or("");
            format!("Running {}", tidy(cmd, 70))
        }
        "Edit" | "MultiEdit" | "Write" | "NotebookEdit" | "apply_patch" => {
            format!("Editing {}", file())
        }
        "Read" => format!("Reading {}", file()),
        "Grep" | "Glob" | "LS" => {
            let pat = str_of(input, "pattern").unwrap_or("");
            format!("Searching {}", tidy(pat, 40))
        }
        "Task" | "Agent" => {
            let d = str_of(input, "description").unwrap_or("a task");
            format!("Subagent: {}", tidy(d, 60))
        }
        "WebFetch" => format!("Fetching {}", tidy(str_of(input, "url").unwrap_or(""), 60)),
        "WebSearch" => format!(
            "Searching the web: {}",
            tidy(str_of(input, "query").unwrap_or(""), 50)
        ),
        "TodoWrite" => "Updating its to-do list".into(),
        "AskUserQuestion" => "Asking you a question".into(),
        n => match n.strip_prefix("mcp__") {
            Some(rest) => format!(
                "Using {}",
                rest.rsplit("__").next().unwrap_or(rest).replace('_', " ")
            ),
            None => format!("Using {n}"),
        },
    }
    .trim()
    .to_string()
}

/// Map one hook payload of Claude Code or Codex to a session update. `None` for events that
/// say nothing Pillbox shows.
pub fn hook_update(agent: &str, v: &Value) -> Option<AgentUpdate> {
    let event = str_of(v, "hook_event_name")?;
    let mut u = AgentUpdate {
        agent: agent.to_string(),
        session: str_of(v, "session_id").unwrap_or("").to_string(),
        cwd: str_of(v, "cwd").map(str::to_string),
        model: str_of(v, "model").map(str::to_string),
        effort: str_of(v, "effort")
            .or_else(|| str_of(v, "reasoning_effort"))
            .map(str::to_string),
        ..AgentUpdate::default()
    };
    let tool = str_of(v, "tool_name").unwrap_or("");
    let input = v.get("tool_input").cloned().unwrap_or(Value::Null);
    match event {
        "SessionStart" => {
            u.state = Some("idle".into());
            u.doing = Some("Ready".into());
        }
        "UserPromptSubmit" => {
            let p = str_of(v, "prompt").unwrap_or("");
            u.state = Some("working".into());
            u.doing = Some("Thinking…".into());
            u.prompt = Some(p.to_string());
            u.step = Some(format!("Asked: {}", tidy(p, 70)));
        }
        "PreToolUse" if tool == "AskUserQuestion" => {
            u.state = Some("needs-you".into());
            let q = input
                .get("questions")
                .and_then(|q| q.get(0))
                .and_then(|q| str_of(q, "question"))
                .unwrap_or("a question");
            let text = format!("Asks: {}", tidy(q, 80));
            u.doing = Some(text.clone());
            u.step = Some(text);
        }
        "PreToolUse" => {
            let d = describe_tool(tool, &input);
            u.state = Some("working".into());
            u.doing = Some(d.clone());
            u.step = Some(d);
        }
        "PostToolUse" => {
            u.state = Some("working".into());
        }
        "PermissionRequest" => {
            let d = describe_tool(tool, &input);
            u.state = Some("needs-you".into());
            u.doing = Some(format!("Wants to: {d}"));
            u.step = Some(format!("Needs approval: {d}"));
            u.ask = Some((tool.to_string(), d));
        }
        "Notification" => {
            let msg = str_of(v, "message").unwrap_or("");
            let kind = str_of(v, "notification_type").unwrap_or("");
            if kind == "permission_prompt" || msg.to_lowercase().contains("permission") {
                u.state = Some("needs-you".into());
                u.doing = Some(tidy(msg, 90));
                u.step = Some(tidy(msg, 90));
            } else {
                return None;
            }
        }
        "Stop" => {
            u.state = Some("done".into());
            u.doing = Some(String::new());
            u.summary = Some(
                str_of(v, "last_assistant_message")
                    .unwrap_or("")
                    .to_string(),
            );
            u.step = Some("Finished".into());
        }
        "SessionEnd" => u.ended = true,
        _ => return None,
    }
    Some(u)
}

/// The JSON an agent's PermissionRequest hook prints to answer on the user's behalf.
pub fn permission_reply(allow: bool) -> String {
    let decision = if allow {
        serde_json::json!({ "behavior": "allow" })
    } else {
        serde_json::json!({ "behavior": "deny", "message": "Denied from the Pillbox." })
    };
    serde_json::json!({
        "hookSpecificOutput": { "hookEventName": "PermissionRequest", "decision": decision }
    })
    .to_string()
}

// ---- transcripts and status lines --------------------------------------------------------------

/// The model and thinking effort of the last assistant message in the tail of a Claude Code
/// transcript (JSON lines). The first line of a tail may be cut; it is skipped.
pub fn transcript_info(tail: &str) -> Option<(String, String)> {
    for line in tail.lines().rev() {
        if !line.contains("\"type\":\"assistant\"") {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let model = v
            .get("message")
            .and_then(|m| str_of(m, "model"))
            .unwrap_or("");
        if model.is_empty() || model.starts_with('<') {
            continue;
        }
        let effort = str_of(&v, "perTurnEffort")
            .or_else(|| str_of(&v, "effort"))
            .unwrap_or("");
        return Some((model.to_string(), effort.to_string()));
    }
    None
}

/// "claude-sonnet-5-5" → "Sonnet 5.5"; anything unfamiliar is shown as it came.
pub fn friendly_model(id: &str) -> String {
    let Some(rest) = id.strip_prefix("claude-") else {
        return id.to_string();
    };
    let parts: Vec<&str> = rest.split('-').collect();
    let Some((family, ver)) = parts.split_first() else {
        return id.to_string();
    };
    let nums: Vec<&str> = ver
        .iter()
        .take_while(|p| p.chars().all(|c| c.is_ascii_digit()) && p.len() <= 2)
        .copied()
        .collect();
    if !matches!(*family, "opus" | "sonnet" | "haiku" | "fable") || nums.is_empty() {
        return id.to_string();
    }
    let mut name = family.to_string();
    if let Some(c) = name.get_mut(0..1) {
        c.make_ascii_uppercase();
    }
    format!("{name} {}", nums.join("."))
}

/// What a status line payload tells: the limits, and which session and model it is for.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct StatuslineInfo {
    pub session: String,
    pub model: String,
    pub effort: String,
    pub cwd: String,
    pub usage: Usage,
}

fn number(v: &Value, key: &str) -> Option<f64> {
    v.get(key).and_then(Value::as_f64).filter(|n| n.is_finite())
}

fn limit_of(v: Option<&Value>) -> Option<Limit> {
    let v = v?;
    let used = number(v, "used_percentage").or_else(|| number(v, "used_percent"))?;
    let resets = number(v, "resets_at").map(|r| {
        // Seconds or milliseconds since 1970.
        if r < 1e12 {
            (r * 1000.0) as i64
        } else {
            r as i64
        }
    });
    Some(Limit {
        used_pct: used.clamp(0.0, 100.0) as f32,
        resets_at_ms: resets,
    })
}

/// Read Claude Code's status line JSON (model, limits, context window).
pub fn statusline_info(v: &Value, now_ms: i64) -> StatuslineInfo {
    let rl = v.get("rate_limits");
    let model = v.get("model");
    StatuslineInfo {
        session: str_of(v, "session_id").unwrap_or("").to_string(),
        model: model
            .and_then(|m| str_of(m, "id").or_else(|| str_of(m, "display_name")))
            .unwrap_or("")
            .to_string(),
        effort: v
            .get("effort")
            .and_then(|e| e.as_str().or_else(|| str_of(e, "level")))
            .unwrap_or("")
            .to_string(),
        cwd: str_of(v, "cwd").unwrap_or("").to_string(),
        usage: Usage {
            five_hour: limit_of(rl.and_then(|r| r.get("five_hour"))),
            seven_day: limit_of(rl.and_then(|r| r.get("seven_day"))),
            context_pct: v
                .get("context_window")
                .and_then(|c| number(c, "used_percentage"))
                .map(|p| p.clamp(0.0, 100.0) as f32),
            updated_ms: now_ms,
        },
    }
}

/// "5 min", "1 h 5 min" for how long ago something started (under a minute: "now").
pub fn elapsed(now_ms: i64, then_ms: i64) -> String {
    let mins = ((now_ms - then_ms) / 60_000).max(0);
    if mins == 0 {
        "now".into()
    } else if mins < 60 {
        format!("{mins} min")
    } else if mins < 24 * 60 {
        format!("{} h {} min", mins / 60, mins % 60)
    } else {
        format!("{} d", mins / (24 * 60))
    }
}

/// "in 2 h 10 min" style text for a limit reset.
pub fn until(now_ms: i64, at_ms: i64) -> String {
    let secs = ((at_ms - now_ms) / 1000).max(0);
    let (d, h, m) = (secs / 86_400, secs % 86_400 / 3600, secs % 3600 / 60);
    if d > 0 {
        format!("{d} d {h} h")
    } else if h > 0 {
        format!("{h} h {m} min")
    } else {
        format!("{} min", m.max(1))
    }
}

// ---- usage from the Claude account ----------------------------------------------------------------

/// Where Pillbox asks for the Claude account's usage. Not a published API: it is what
/// Claude Code's own `/usage` screen reads. It needs the account's sign-in (an OAuth access
/// token with the `user:profile` scope) and the beta header below.
pub const ACCOUNT_USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
pub const ACCOUNT_USAGE_BETA: &str = "oauth-2025-04-20";

/// The access token in Claude Code's `~\.claude\.credentials.json`
/// (`claudeAiOauth.accessToken`). Read only; Pillbox never refreshes or writes it.
pub fn access_token(credentials_json: &str) -> Result<String, TokenProblem> {
    // A byte-order mark, or padding (NULs, a second write) after the object, is tolerated:
    // only the first JSON value counts.
    let text = credentials_json.trim_start_matches('\u{feff}');
    let v: Value = serde_json::Deserializer::from_str(text)
        .into_iter::<Value>()
        .next()
        .ok_or(TokenProblem::Empty)?
        .map_err(|e| TokenProblem::NotJson(e.classify(), e.line(), e.column()))?;
    let oauth = v.get("claudeAiOauth").ok_or(TokenProblem::NoOauth)?;
    match oauth.get("accessToken") {
        Some(Value::String(t)) if !t.trim().is_empty() => Ok(t.trim().to_string()),
        Some(Value::String(_)) | None => Err(TokenProblem::NoToken),
        Some(_) => Err(TokenProblem::TokenNotText),
    }
}

/// Why no token came out of the credentials file. Says where, never what (the file holds
/// secrets).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenProblem {
    Empty,
    NotJson(serde_json::error::Category, usize, usize),
    NoOauth,
    NoToken,
    TokenNotText,
}

/// "2026-11-19T22:00:00.123456+00:00" or "…Z" to milliseconds since 1970. Seconds fractions
/// are dropped; an offset other than UTC is applied.
pub fn iso_to_ms(s: &str) -> Option<i64> {
    let s = s.trim();
    let num = |a: usize, b: usize| s.get(a..b)?.parse::<i64>().ok();
    if s.len() < 19
        || s.as_bytes()
            .get(10)
            .is_none_or(|c| !matches!(c, b'T' | b't' | b' '))
    {
        return None;
    }
    let (year, month, day) = (num(0, 4)?, num(5, 7)?, num(8, 10)?);
    let (hour, min, sec) = (num(11, 13)?, num(14, 16)?, num(17, 19)?);
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour > 23 || min > 59 || sec > 60 {
        return None;
    }
    // After the seconds: an optional fraction, then Z or ±hh:mm (none = UTC).
    let rest = &s[19..];
    let rest = rest.trim_start_matches(|c: char| c == '.' || c.is_ascii_digit());
    let offset_min = match rest.as_bytes().first() {
        None | Some(b'Z' | b'z') => 0,
        Some(sign @ (b'+' | b'-')) => {
            let h: i64 = rest.get(1..3)?.parse().ok()?;
            let m: i64 = rest.get(4..6).and_then(|x| x.parse().ok()).unwrap_or(0);
            let off = h * 60 + m;
            if *sign == b'-' { -off } else { off }
        }
        _ => return None,
    };
    let days = crate::calendar::days_from_civil(crate::calendar::Date {
        year: i32::try_from(year).ok()?,
        month: u32::try_from(month).ok()?,
        day: u32::try_from(day).ok()?,
    });
    let secs = days * 86_400 + hour * 3600 + min * 60 + sec - offset_min * 60;
    Some(secs * 1000)
}

/// The account usage answer: `{"five_hour": {"utilization": 42.0, "resets_at": "…"},
/// "seven_day": {…}, …}` with utilization in percent. `None` if it has neither limit.
pub fn account_usage(v: &Value, now_ms: i64) -> Option<Usage> {
    let limit = |key: &str| {
        let l = v.get(key)?;
        let used = l.get("utilization").and_then(Value::as_f64)? as f32;
        Some(Limit {
            used_pct: used.clamp(0.0, 100.0),
            resets_at_ms: l
                .get("resets_at")
                .and_then(Value::as_str)
                .and_then(iso_to_ms),
        })
    };
    let usage = Usage {
        five_hour: limit("five_hour"),
        seven_day: limit("seven_day"),
        context_pct: None,
        updated_ms: now_ms,
    };
    (usage.five_hour.is_some() || usage.seven_day.is_some()).then_some(usage)
}

// ---- requests ----------------------------------------------------------------------------------

/// What the server hands to the app.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    Flash(Flash),
    Status(StatusReq),
    Agent(AgentUpdate),
    /// A status line payload from this agent.
    Statusline(String, StatuslineInfo),
}

/// Turn a request path (after `/v1/`) and its JSON body into a command. Errors are short
/// messages for the client.
pub fn parse_request(path: &str, body: &[u8], now_ms: i64) -> Result<Command, String> {
    let json = || serde_json::from_slice::<Value>(body).map_err(|e| format!("bad JSON: {e}"));
    let bad = |e: serde_json::Error| format!("bad request: {e}");
    match path.trim_matches('/') {
        "flash" => {
            let f: Flash = serde_json::from_slice(body).map_err(bad)?;
            f.tidy().map(Command::Flash).ok_or("empty text".into())
        }
        "status" => {
            let s: StatusReq = serde_json::from_slice(body).map_err(bad)?;
            if tidy(&s.id, 48).is_empty() {
                return Err("a status needs an id".into());
            }
            Ok(Command::Status(s))
        }
        "agent" => {
            let u: AgentUpdate = serde_json::from_slice(body).map_err(bad)?;
            if agent_key(&u.agent).is_none() {
                return Err("an agent event needs a name".into());
            }
            Ok(Command::Agent(u))
        }
        p => {
            if let Some(agent) = p.strip_prefix("hook/") {
                let key = agent_key(agent).ok_or("bad agent name")?;
                let v = json()?;
                return hook_update(&key, &v)
                    .map(Command::Agent)
                    .ok_or_else(|| "nothing to show for that event".into());
            }
            if let Some(agent) = p.strip_prefix("statusline/") {
                let key = agent_key(agent).ok_or("bad agent name")?;
                return Ok(Command::Statusline(key, statusline_info(&json()?, now_ms)));
            }
            Err("unknown request".into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn upd(agent: &str, session: &str, state: &str) -> AgentUpdate {
        AgentUpdate {
            agent: agent.into(),
            session: session.into(),
            state: Some(state.into()),
            ..AgentUpdate::default()
        }
    }

    #[test]
    fn a_status_line_without_limits_keeps_the_known_ones() {
        let mut b = Board::default();
        let account = account_usage(
            &json!({"five_hour": {"utilization": 30.0}, "seven_day": {"utilization": 60.0}}),
            1_000,
        )
        .expect("usage");
        b.set_usage("claude", account);
        // Claude Code in a terminal ticks before its first request: no rate_limits yet.
        let empty = statusline_info(&json!({"session_id": "t", "model": {"id": "x"}}), 5_000);
        b.apply_statusline("claude", &empty);
        let u = b.usage["claude"];
        assert_eq!(u.five_hour.map(|l| l.used_pct), Some(30.0));
        assert_eq!(u.seven_day.map(|l| l.used_pct), Some(60.0));
        assert_eq!(u.updated_ms, 1_000, "still the account's reading");
        // A tick that carries the 5-hour limit replaces only that one.
        let five = statusline_info(
            &json!({"rate_limits": {"five_hour": {"used_percentage": 35.0}}}),
            9_000,
        );
        assert!(b.apply_statusline("claude", &five));
        let u = b.usage["claude"];
        assert_eq!(u.five_hour.map(|l| l.used_pct), Some(35.0));
        assert_eq!(u.seven_day.map(|l| l.used_pct), Some(60.0));
        assert_eq!(u.updated_ms, 9_000);
    }

    #[test]
    fn iso_times_become_milliseconds() {
        assert_eq!(iso_to_ms("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(iso_to_ms("1970-01-01T00:00:01.999999+00:00"), Some(1000));
        assert_eq!(iso_to_ms("2026-11-19T22:00:00Z"), Some(1_795_125_600_000));
        // 23:00 at +01:00 is 22:00 UTC; 17:00 at -05:00 too.
        assert_eq!(
            iso_to_ms("2026-11-19T23:00:00+01:00"),
            Some(1_795_125_600_000)
        );
        assert_eq!(
            iso_to_ms("2026-11-19T17:00:00-05:00"),
            Some(1_795_125_600_000)
        );
        assert_eq!(iso_to_ms("2026-11-19T22:00:00"), Some(1_795_125_600_000));
        for bad in [
            "",
            "soon",
            "2026-13-01T00:00:00Z",
            "2026-11-19 x",
            "2026-11-19T22:00:00Q",
        ] {
            assert_eq!(iso_to_ms(bad), None, "{bad}");
        }
    }

    #[test]
    fn account_usage_reads_both_limits() {
        let v = json!({
            "five_hour": {"utilization": 42.0, "resets_at": "2026-11-19T22:00:00.5+00:00"},
            "seven_day": {"utilization": 130.0, "resets_at": null},
            "seven_day_opus": {"utilization": 3.0, "resets_at": null},
            "extra_usage": null
        });
        let u = account_usage(&v, 9).expect("usage");
        let five = u.five_hour.expect("five");
        assert_eq!(five.used_pct, 42.0);
        assert_eq!(five.resets_at_ms, Some(1_795_125_600_000));
        let week = u.seven_day.expect("week");
        assert_eq!((week.used_pct, week.resets_at_ms), (100.0, None));
        assert_eq!(u.updated_ms, 9);
        assert!(account_usage(&json!({"five_hour": null}), 0).is_none());
        assert!(account_usage(&json!({"error": {"type": "authentication_error"}}), 0).is_none());
    }

    #[test]
    fn token_comes_from_the_credentials_file() {
        let file = r#"{"claudeAiOauth":{"accessToken":" abc ","refreshToken":"r","expiresAt":0}}"#;
        assert_eq!(access_token(file).as_deref(), Ok("abc"));
        // A byte-order mark and padding after the object are tolerated.
        assert_eq!(
            access_token(&format!("\u{feff}{file}\n\0\0\0")).as_deref(),
            Ok("abc")
        );
        assert_eq!(
            access_token(r#"{"claudeAiOauth":{"accessToken":""}}"#),
            Err(TokenProblem::NoToken)
        );
        assert_eq!(
            access_token(r#"{"claudeAiOauth":{"accessToken":{"v":1}}}"#),
            Err(TokenProblem::TokenNotText)
        );
        assert_eq!(access_token(r#"{"other":1}"#), Err(TokenProblem::NoOauth));
        assert!(matches!(
            access_token("not json"),
            Err(TokenProblem::NotJson(..))
        ));
        assert_eq!(access_token("   "), Err(TokenProblem::Empty));
    }

    #[test]
    fn kind_summary_follows_the_program_and_its_sessions() {
        let mut b = Board::default();
        // Closed and nothing reported: offline. Open with no session: idle.
        assert_eq!(b.kind_summary("claude", false).status, KindStatus::Offline);
        assert_eq!(b.kind_summary("claude", false).line(0), "Not running");
        let idle = b.kind_summary("claude", true);
        assert_eq!(idle.status, KindStatus::Idle);
        assert_eq!(idle.line(0), "Ready, nothing running");

        let mut work = upd("claude", "s1", "working");
        work.doing = Some("Editing tools.rs".into());
        work.cwd = Some("C:\\code\\Dynamic Island".into());
        b.apply_agent(&work, 1_000);
        let s = b.kind_summary("claude", true);
        assert_eq!(s.status, KindStatus::Working);
        assert_eq!(s.line(2_000), "Editing tools.rs · Dynamic Island");
        // A session reporting counts as running even if the program was not seen (a CLI).
        assert_eq!(b.kind_summary("claude", false).status, KindStatus::Working);

        b.apply_agent(&upd("claude", "s2", "working"), 1_500);
        assert!(
            b.kind_summary("claude", true)
                .line(2_000)
                .ends_with("(+1 more)")
        );

        // Needing you beats working.
        b.apply_agent(&upd("claude", "s2", "needs-you"), 1_600);
        let s = b.kind_summary("claude", true);
        assert_eq!(s.status, KindStatus::NeedsYou);
        assert!(
            s.line(2_000).starts_with("Waiting for you"),
            "{}",
            s.line(2_000)
        );

        // All done: idle, saying when it finished.
        b.apply_agent(&upd("claude", "s2", "done"), 50_000);
        b.apply_agent(&upd("claude", "s1", "done"), 60_000);
        let s = b.kind_summary("claude", true);
        assert_eq!(s.status, KindStatus::Idle);
        assert_eq!(
            s.line(60_000 + 3 * 60_000),
            "Finished 3 min ago · Dynamic Island"
        );
        assert_eq!(s.working, 0);

        // Quitting the program ends its sessions; other kinds stay.
        b.apply_agent(&upd("codex", "c1", "working"), 1_000);
        assert!(b.end_kind("claude"));
        assert!(!b.end_kind("claude"));
        assert_eq!(b.kind_summary("claude", false).status, KindStatus::Offline);
        assert_eq!(b.kind_summary("codex", false).status, KindStatus::Working);
    }

    #[test]
    fn tidy_collapses_and_cuts() {
        assert_eq!(tidy("  a \n\t b  ", 10), "a b");
        assert_eq!(tidy("abcdef", 3), "abc…");
        assert_eq!(tidy("abc", 3), "abc");
        assert_eq!(tidy("a\u{7}b", 5), "ab");
        assert_eq!(tidy("日本語です", 3), "日本語…");
    }

    #[test]
    fn names_are_made_safe() {
        assert_eq!(agent_key("  Claude "), Some("claude".into()));
        assert_eq!(agent_key("my/agent<1>"), Some("myagent1".into()));
        assert_eq!(agent_key("///"), None);
        assert_eq!(agent_key(&"x".repeat(80)).map(|k| k.len()), Some(24));
        assert_eq!(display_name("claude"), "Claude Code");
        assert_eq!(display_name("builder"), "Builder");
        assert_eq!(base_name("C:\\a\\b\\proj\\"), "proj");
        assert_eq!(base_name("/x/y.rs"), "y.rs");
    }

    #[test]
    fn flash_queue_replaces_same_source_and_caps() {
        let mut q = FlashQueue::default();
        let f = |t: &str, s: &str| Flash {
            text: t.into(),
            source: s.into(),
            ..Flash::default()
        };
        q.push(f("build 1", "build"));
        q.push(f("test", "test"));
        q.push(f("build 2", "build"));
        assert_eq!(q.len(), 2);
        assert_eq!(q.pop().map(|x| x.text), Some("build 2".into()));
        for i in 0..6 {
            q.push(f(&format!("n{i}"), ""));
        }
        assert_eq!(q.len(), MAX_FLASHES);
        assert_eq!(q.pop().map(|x| x.text), Some("n3".into()));
    }

    #[test]
    fn urgent_flashes_survive_game_mode() {
        let mut q = FlashQueue::default();
        q.push(Flash {
            text: "a".into(),
            ..Flash::default()
        });
        q.push(Flash {
            text: "b".into(),
            urgent: true,
            ..Flash::default()
        });
        q.drop_quiet();
        assert_eq!(q.len(), 1);
    }

    #[test]
    fn status_updates_in_place_and_removes() {
        let mut b = Board::default();
        let mut r = StatusReq {
            id: "build".into(),
            title: Some("cargo build".into()),
            ..StatusReq::default()
        };
        assert!(b.set_status(&r, 1));
        r.state = Some(StatusState::Failed);
        r.detail = Some("3 errors".into());
        assert!(b.set_status(&r, 2));
        assert_eq!(b.statuses.len(), 1);
        assert_eq!(b.statuses[0].state, StatusState::Failed);
        assert_eq!(b.statuses[0].updated_ms, 2);
        r.remove = true;
        assert!(b.set_status(&r, 3));
        assert!(b.statuses.is_empty());
        assert!(!b.set_status(&r, 4));
        assert!(!b.set_status(&StatusReq::default(), 5), "no id, no entry");
    }

    #[test]
    fn a_partial_update_keeps_the_other_fields() {
        let mut b = Board::default();
        let r = |f: &dyn Fn(&mut StatusReq)| {
            let mut r = StatusReq {
                id: "build".into(),
                ..StatusReq::default()
            };
            f(&mut r);
            r
        };
        b.set_status(
            &r(&|r| {
                r.title = Some("cargo build".into());
                r.detail = Some("compiling".into());
                r.progress = Some(40);
            }),
            1,
        );
        b.set_status(&r(&|r| r.progress = Some(75)), 2);
        let s = &b.statuses[0];
        assert_eq!(
            (s.title.as_str(), s.detail.as_str(), s.progress),
            ("cargo build", "compiling", Some(75))
        );
        assert_eq!(s.state, StatusState::Running);
        // Finishing takes the bar away unless a value is given; an empty title is not a title.
        b.set_status(
            &r(&|r| {
                r.state = Some(StatusState::Ok);
                r.title = Some("  ".into());
            }),
            3,
        );
        let s = &b.statuses[0];
        assert_eq!(
            (s.state, s.progress, s.title.as_str()),
            (StatusState::Ok, None, "cargo build")
        );
        b.set_status(&r(&|r| r.detail = Some(String::new())), 4);
        assert_eq!(b.statuses[0].detail, "", "an empty detail clears it");
    }

    #[test]
    fn status_progress_is_clamped_and_title_defaults_to_id() {
        let mut b = Board::default();
        b.set_status(
            &StatusReq {
                id: "x".into(),
                progress: Some(250),
                ..StatusReq::default()
            },
            0,
        );
        assert_eq!(b.statuses[0].progress, Some(100));
        assert_eq!(b.statuses[0].title, "x");
    }

    #[test]
    fn statuses_are_capped() {
        let mut b = Board::default();
        for i in 0..(MAX_ENTRIES + 10) {
            b.set_status(
                &StatusReq {
                    id: format!("s{i}"),
                    ..StatusReq::default()
                },
                i as i64,
            );
        }
        assert_eq!(b.statuses.len(), MAX_ENTRIES);
        assert_eq!(b.statuses[0].id, format!("s{}", MAX_ENTRIES + 9));
    }

    #[test]
    fn a_session_goes_through_a_turn() {
        let mut b = Board::default();
        let hooks = [
            json!({"hook_event_name":"SessionStart","session_id":"s1","cwd":"C:\\p\\proj"}),
            json!({"hook_event_name":"UserPromptSubmit","session_id":"s1","prompt":"fix the bug in main"}),
            json!({"hook_event_name":"PreToolUse","session_id":"s1","tool_name":"Edit","tool_input":{"file_path":"C:\\p\\proj\\src\\main.rs"}}),
            json!({"hook_event_name":"PreToolUse","session_id":"s1","tool_name":"Bash","tool_input":{"command":"cargo test"}}),
        ];
        for (i, h) in hooks.iter().enumerate() {
            let u = hook_update("claude", h).expect("maps");
            assert!(b.apply_agent(&u, i as i64 * 1000));
        }
        let a = &b.agents[0];
        assert_eq!(a.state, AgentState::Working);
        assert_eq!(a.project, "proj");
        assert_eq!(a.doing, "Running cargo test");
        assert_eq!(a.prompt, "fix the bug in main");
        let steps: Vec<&str> = a.steps.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(
            steps,
            [
                "Asked: fix the bug in main",
                "Editing main.rs",
                "Running cargo test"
            ]
        );

        let stop = json!({"hook_event_name":"Stop","session_id":"s1","last_assistant_message":"Fixed it.\nAll tests pass."});
        b.apply_agent(&hook_update("claude", &stop).expect("maps"), 9000);
        let a = &b.agents[0];
        assert_eq!(a.state, AgentState::Done);
        assert_eq!(a.summary, "Fixed it. All tests pass.");
        assert!(a.doing.is_empty());

        // The next prompt starts a fresh timeline.
        let next = json!({"hook_event_name":"UserPromptSubmit","session_id":"s1","prompt":"again"});
        b.apply_agent(&hook_update("claude", &next).expect("maps"), 10_000);
        let a = &b.agents[0];
        assert_eq!(a.state, AgentState::Working);
        assert!(a.summary.is_empty());
        assert_eq!(a.steps.len(), 1);

        let end = json!({"hook_event_name":"SessionEnd","session_id":"s1"});
        assert!(b.apply_agent(&hook_update("claude", &end).expect("maps"), 11_000));
        assert!(b.agents.is_empty());
    }

    #[test]
    fn a_permission_request_needs_you_until_answered() {
        let mut b = Board::default();
        let req = json!({"hook_event_name":"PermissionRequest","session_id":"s","tool_name":"Bash","tool_input":{"command":"rm -rf build"}});
        let u = hook_update("claude", &req).expect("maps");
        b.apply_agent(&u, 0);
        let a = &b.agents[0];
        assert_eq!(a.state, AgentState::NeedsYou);
        let id = a.ask.as_ref().expect("ask").id;
        assert_eq!(
            a.ask.as_ref().map(|q| q.what.as_str()),
            Some("Running rm -rf build")
        );
        assert!(b.clear_ask(id, true));
        assert_eq!(b.agents[0].state, AgentState::Working);
        assert!(b.agents[0].ask.is_none());
        assert!(!b.clear_ask(id, true));
    }

    #[test]
    fn an_unanswered_question_stays_needs_you_without_buttons() {
        let mut b = Board::default();
        let req = json!({"hook_event_name":"PermissionRequest","session_id":"s","tool_name":"Bash","tool_input":{"command":"a"}});
        b.apply_agent(&hook_update("claude", &req).expect("maps"), 0);
        let id = b.agents[0].ask.as_ref().expect("ask").id;
        assert_eq!(b.pending_asks(), 1);
        assert_eq!(b.agent_asking(id).map(|a| a.session.as_str()), Some("s"));
        assert!(b.clear_ask(id, false));
        assert_eq!(b.agents[0].state, AgentState::NeedsYou);
        assert!(b.agents[0].ask.is_none());
        assert_eq!(b.pending_asks(), 0);
    }

    #[test]
    fn statusline_updates_limits_and_known_sessions_only() {
        let mut b = Board::default();
        let info = statusline_info(
            &json!({"session_id":"s1","model":{"id":"claude-opus-5-5"},
                    "rate_limits":{"five_hour":{"used_percentage":40.0}}}),
            0,
        );
        assert!(b.apply_statusline("claude", &info), "first limits are news");
        assert!(b.agents.is_empty(), "no session created");
        assert!(
            !b.apply_statusline("claude", &info),
            "same numbers, nothing new"
        );
        b.apply_agent(&upd("claude", "s1", "working"), 5);
        assert!(b.apply_statusline("claude", &info));
        assert_eq!(b.agents[0].model, "claude-opus-5-5");
        assert_eq!(
            b.agents[0].updated_ms, 5,
            "a status line is not news from the agent"
        );
        let moved = statusline_info(
            &json!({"session_id":"s1","rate_limits":{"five_hour":{"used_percentage":41.0}}}),
            9,
        );
        assert!(b.apply_statusline("claude", &moved));
        assert_eq!(b.state_of("claude", "s1"), Some(AgentState::Working));
        assert_eq!(b.state_of("claude", "other"), None);
        assert_eq!(b.state_of("", "s1"), None);
        assert_eq!(b.state_of("claude", ""), None);
    }

    #[test]
    fn dismiss_takes_one_entry_off() {
        let mut b = Board::default();
        b.apply_agent(&upd("claude", "a", "done"), 0);
        b.set_status(
            &StatusReq {
                id: "x".into(),
                ..StatusReq::default()
            },
            0,
        );
        let (au, su) = (b.agents[0].uid, b.statuses[0].uid);
        assert_ne!(au, su);
        assert!(b.dismiss(su));
        assert!(b.statuses.is_empty() && b.agents.len() == 1);
        assert!(b.agent(au).is_some());
        assert!(b.dismiss(au));
        assert!(!b.dismiss(au));
    }

    #[test]
    fn elapsed_reads_well() {
        assert_eq!(elapsed(30_000, 0), "now");
        assert_eq!(elapsed(5 * 60_000, 0), "5 min");
        assert_eq!(elapsed(65 * 60_000, 0), "1 h 5 min");
        assert_eq!(elapsed(50 * 3_600_000, 0), "2 d");
    }

    #[test]
    fn ask_ids_are_unique() {
        let mut b = Board::default();
        let req = json!({"hook_event_name":"PermissionRequest","session_id":"s","tool_name":"Bash","tool_input":{"command":"a"}});
        let u = hook_update("claude", &req).expect("maps");
        b.apply_agent(&u, 0);
        let first = b.agents[0].ask.as_ref().map(|q| q.id);
        b.apply_agent(&u, 1);
        assert_ne!(first, b.agents[0].ask.as_ref().map(|q| q.id));
    }

    #[test]
    fn ask_user_question_needs_you() {
        let h = json!({"hook_event_name":"PreToolUse","session_id":"s","tool_name":"AskUserQuestion","tool_input":{"questions":[{"question":"Which one?"}]}});
        let u = hook_update("claude", &h).expect("maps");
        assert_eq!(u.state.as_deref(), Some("needs-you"));
        assert_eq!(u.doing.as_deref(), Some("Asks: Which one?"));
        assert!(u.ask.is_none(), "cannot be answered with Allow/Deny");
    }

    #[test]
    fn unknown_and_quiet_events_are_ignored() {
        assert!(hook_update("claude", &json!({"hook_event_name":"PreCompact"})).is_none());
        assert!(hook_update("claude", &json!({"nothing":1})).is_none());
        let n =
            json!({"hook_event_name":"Notification","message":"Claude is waiting for your input"});
        assert!(hook_update("claude", &n).is_none());
        let p = json!({"hook_event_name":"Notification","message":"Claude needs your permission to use Bash"});
        assert_eq!(
            hook_update("claude", &p).and_then(|u| u.state),
            Some("needs-you".into())
        );
    }

    #[test]
    fn tools_are_described() {
        let d = |n: &str, i: Value| describe_tool(n, &i);
        assert_eq!(d("Read", json!({"file_path":"a/b/c.rs"})), "Reading c.rs");
        assert_eq!(d("Grep", json!({"pattern":"fn main"})), "Searching fn main");
        assert_eq!(d("mcp__srv__do_thing", json!({})), "Using do thing");
        assert_eq!(d("Weird", json!({})), "Using Weird");
        assert_eq!(d("Bash", json!({"command":"a\n  b   c"})), "Running a b c");
    }

    #[test]
    fn working_sessions_go_stale_and_done_ones_leave() {
        let mut b = Board::default();
        b.apply_agent(&upd("claude", "a", "working"), 0);
        b.apply_agent(&upd("claude", "b", "done"), 0);
        assert!(!b.expire(1000, 3_600_000, 1_200_000));
        assert!(b.expire(1_300_000, 3_600_000, 1_200_000));
        assert_eq!(b.agents[0].state, AgentState::Idle);
        assert_eq!(b.agents.len(), 2);
        assert!(b.expire(3_700_000, 3_600_000, 1_200_000));
        assert_eq!(b.agents.len(), 1, "the done one left");
    }

    #[test]
    fn statuses_expire_but_running_ones_last_longer() {
        let mut b = Board::default();
        b.set_status(
            &StatusReq {
                id: "ok".into(),
                state: Some(StatusState::Ok),
                ..StatusReq::default()
            },
            0,
        );
        b.set_status(
            &StatusReq {
                id: "run".into(),
                ..StatusReq::default()
            },
            0,
        );
        assert!(b.expire(3_700_000, 3_600_000, 1_200_000));
        assert_eq!(b.statuses.len(), 1);
        assert_eq!(b.statuses[0].id, "run");
        assert!(b.expire(6 * 3_600_000 + 1, 3_600_000, 1_200_000));
        assert!(b.statuses.is_empty());
    }

    #[test]
    fn agents_are_ordered_by_need() {
        let mut b = Board::default();
        b.apply_agent(&upd("claude", "done", "done"), 5);
        b.apply_agent(&upd("claude", "work", "working"), 1);
        b.apply_agent(&upd("codex", "ask", "needs-you"), 0);
        b.apply_agent(&upd("claude", "work2", "working"), 9);
        let order: Vec<&str> = b
            .ordered_agents()
            .iter()
            .map(|a| a.session.as_str())
            .collect();
        assert_eq!(order, ["ask", "work2", "work", "done"]);
        assert_eq!(b.counts(), (1, 2, 1));
    }

    #[test]
    fn agents_are_capped_and_sessions_keyed_per_agent() {
        let mut b = Board::default();
        for i in 0..(MAX_ENTRIES + 5) {
            b.apply_agent(&upd("claude", &format!("s{i}"), "working"), i as i64);
        }
        assert_eq!(b.agents.len(), MAX_ENTRIES);
        assert!(
            b.agents
                .iter()
                .any(|a| a.session == format!("s{}", MAX_ENTRIES + 4))
        );
        let mut b = Board::default();
        b.apply_agent(&upd("claude", "x", "working"), 0);
        b.apply_agent(&upd("codex", "x", "working"), 0);
        assert_eq!(b.agents.len(), 2);
        b.apply_agent(&upd("", "x", "working"), 0);
        assert_eq!(b.agents.len(), 2, "no name, no entry");
    }

    #[test]
    fn timeline_is_bounded_and_skips_repeats() {
        let mut b = Board::default();
        for i in 0..30 {
            b.apply_agent(
                &AgentUpdate {
                    agent: "x".into(),
                    step: Some(format!("step {}", i / 2)),
                    ..AgentUpdate::default()
                },
                i,
            );
        }
        let a = &b.agents[0];
        assert_eq!(a.steps.len(), MAX_STEPS);
        assert_eq!(a.steps.back().map(|s| s.text.as_str()), Some("step 14"));
    }

    #[test]
    fn state_names_are_forgiving() {
        assert_eq!(AgentState::parse("Needs You"), Some(AgentState::NeedsYou));
        assert_eq!(AgentState::parse("needs_you"), Some(AgentState::NeedsYou));
        assert_eq!(AgentState::parse("BUSY"), Some(AgentState::Working));
        assert_eq!(AgentState::parse("nope"), None);
    }

    #[test]
    fn transcript_tail_gives_model_and_effort() {
        let tail = concat!(
            "cut off line {\"type\":\"assistant\"\n",
            "{\"type\":\"user\",\"message\":{}}\n",
            "{\"type\":\"assistant\",\"effort\":\"high\",\"perTurnEffort\":\"xhigh\",\"message\":{\"model\":\"claude-sonnet-5-5\"}}\n",
            "{\"type\":\"attachment\"}\n"
        );
        assert_eq!(
            transcript_info(tail),
            Some(("claude-sonnet-5-5".into(), "xhigh".into()))
        );
        assert_eq!(transcript_info("{\"type\":\"user\"}"), None);
        let synthetic = "{\"type\":\"assistant\",\"message\":{\"model\":\"<synthetic>\"}}";
        assert_eq!(transcript_info(synthetic), None);
    }

    #[test]
    fn models_get_friendly_names() {
        assert_eq!(friendly_model("claude-sonnet-5-5"), "Sonnet 5.5");
        assert_eq!(friendly_model("claude-opus-5-5"), "Opus 5.5");
        assert_eq!(friendly_model("claude-fable-5-2"), "Fable 5.2");
        assert_eq!(friendly_model("gpt-5"), "gpt-5");
        assert_eq!(
            friendly_model("claude-3-5-sonnet-20241022"),
            "claude-3-5-sonnet-20241022"
        );
    }

    #[test]
    fn statusline_gives_limits() {
        let v = json!({
            "session_id": "s1",
            "model": {"id": "claude-opus-5-5", "display_name": "Opus 5.5"},
            "cwd": "C:\\p",
            "rate_limits": {
                "five_hour": {"used_percentage": 42.5, "resets_at": 1_800_000_000},
                "seven_day": {"used_percentage": 120.0}
            },
            "context_window": {"used_percentage": 33.0}
        });
        let i = statusline_info(&v, 7);
        assert_eq!(i.session, "s1");
        assert_eq!(i.model, "claude-opus-5-5");
        let five = i.usage.five_hour.expect("five hour");
        assert!((five.used_pct - 42.5).abs() < 1e-4);
        assert_eq!(five.resets_at_ms, Some(1_800_000_000_000));
        assert_eq!(i.usage.seven_day.map(|l| l.used_pct), Some(100.0));
        assert_eq!(i.usage.context_pct, Some(33.0));
        assert_eq!(i.usage.updated_ms, 7);
        let none = statusline_info(&json!({}), 0);
        assert!(none.usage.five_hour.is_none() && none.usage.context_pct.is_none());
    }

    #[test]
    fn reset_times_read_well() {
        assert_eq!(until(0, 90 * 60 * 1000), "1 h 30 min");
        assert_eq!(until(0, 30 * 1000), "1 min");
        assert_eq!(until(0, 50 * 3600 * 1000), "2 d 2 h");
        assert_eq!(until(10, 0), "1 min");
    }

    #[test]
    fn requests_parse() {
        let flash = parse_request(
            "flash",
            br#"{"text":" Build  done ","ms":5,"source":"cargo"}"#,
            0,
        );
        assert_eq!(
            flash,
            Ok(Command::Flash(Flash {
                text: "Build done".into(),
                source: "cargo".into(),
                ms: Some(500),
                urgent: false
            }))
        );
        assert!(parse_request("flash", br#"{"text":"  "}"#, 0).is_err());
        assert!(parse_request("flash", b"nope", 0).is_err());
        assert!(parse_request("status", br#"{"title":"x"}"#, 0).is_err());
        let s = parse_request("status", br#"{"id":"b","state":"success"}"#, 0);
        assert!(matches!(s, Ok(Command::Status(r)) if r.state == Some(StatusState::Ok)));
        assert!(parse_request("agent", br#"{"agent":"  "}"#, 0).is_err());
        assert!(parse_request("agent", br#"{"agent":"mine","state":"done"}"#, 0).is_ok());
        assert!(parse_request("nothing", b"{}", 0).is_err());
        let h = br#"{"hook_event_name":"Stop","session_id":"a"}"#;
        assert!(matches!(
            parse_request("hook/claude", h, 0),
            Ok(Command::Agent(_))
        ));
        assert!(parse_request("hook/claude", br#"{"hook_event_name":"X"}"#, 0).is_err());
        assert!(parse_request("hook/%%%", h, 0).is_err());
        assert!(matches!(
            parse_request("statusline/claude", b"{}", 0),
            Ok(Command::Statusline(k, _)) if k == "claude"
        ));
    }

    #[test]
    fn permission_replies_are_valid_json() {
        let allow: Value = serde_json::from_str(&permission_reply(true)).expect("json");
        assert_eq!(
            allow["hookSpecificOutput"]["decision"]["behavior"],
            json!("allow")
        );
        let deny: Value = serde_json::from_str(&permission_reply(false)).expect("json");
        assert_eq!(
            deny["hookSpecificOutput"]["decision"]["behavior"],
            json!("deny")
        );
        assert_eq!(
            deny["hookSpecificOutput"]["hookEventName"],
            json!("PermissionRequest")
        );
    }
}
