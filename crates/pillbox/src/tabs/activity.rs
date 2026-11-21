//! The Activity tab (M7): coding agents (who works, who needs you, who is done) and the live
//! statuses scripts push with `pillboxctl`. A child module of `tabs`, so it shares its helpers.

use pillbox_core::script::{
    self, Agent, AgentState, Board, KindStatus, Limit, Status, StatusState,
};

use super::{
    AMBER, Ctx, G_CHAT, G_CHECK, G_CLOSE, G_INFO, G_STOP, G_SYNC, GREEN, RED, Scroller, badge,
    button, icon_button, note, push_clipped, rounded,
};
use crate::{
    platform::gfx::{Align, Frame},
    scene::{Action, Hit, Rect, icon, label},
};
use pillbox_core::theme::Rgba;

/// Segoe Fluent Icons "open in new window".
const G_OPEN_APP: &str = "\u{E8A7}";

/// The Claude desktop app's AppUserModelId (a packaged app; `Get-StartApps` lists it). Used to
/// open it and to load its icon for the Claude card.
pub const CLAUDE_AUMID: &str = "Claude_pzs8sxrjxfjjc!Claude";

/// Heights of the Claude card: closed, running without limits, running with limits.
const CLAUDE_OFFLINE_H: f32 = 56.0;
const CLAUDE_NO_LIMITS_H: f32 = 80.0;
const CLAUDE_LIMITS_H: f32 = 108.0;

enum Row<'a> {
    Heading(&'static str),
    /// The card about Claude as a whole: status, what it does, usage limits.
    Claude,
    /// The limits line of one agent kind.
    Limits(&'a str),
    Agent(&'a Agent),
    Status(&'a Status),
}

/// "Claude Code · 5 h 42% · resets in 2 h 10 min · 7 d 12%", or why there are no numbers.
fn limits_text(kind: &str, board: &Board, now_ms: i64) -> String {
    let name = script::display_name(kind);
    let Some(u) = board.usage.get(kind) else {
        return if kind == "claude" {
            format!("{name} · limits unknown (status line not connected)")
        } else {
            format!("{name} · limits unknown")
        };
    };
    let mut parts = Vec::new();
    if let Some(l) = u.five_hour {
        parts.push(format!("5 h {:.0}%", l.used_pct));
        if let Some(r) = l.resets_at_ms.filter(|r| *r > now_ms) {
            parts.push(format!("resets in {}", script::until(now_ms, r)));
        }
    }
    if let Some(l) = u.seven_day {
        parts.push(format!("7 d {:.0}%", l.used_pct));
    }
    if parts.is_empty() {
        format!("{name} · limits unknown")
    } else {
        format!("{name} · {}", parts.join(" · "))
    }
}

/// The Claude card: the app's icon, "Claude Code", a status chip (Working, Needs you, Idle,
/// Offline) and a line saying what it does; while it runs, a bar for the 5-hour and the weekly
/// limit underneath. A click opens the Claude app.
fn claude_card_row(
    f: &mut Frame<'_>,
    cx: &Ctx<'_>,
    sc: &Scroller,
    rect: Rect,
    hits: &mut Vec<Hit>,
) {
    let k = cx.k;
    let a = &cx.ui.activity;
    let (board, now) = (a.board, a.now_ms);
    let sum = board.kind_summary("claude", a.claude_running);
    let offline = sum.status == KindStatus::Offline;
    let (l, t, r, _b) = rect;
    let hovered = cx.hovered(rect);
    let bg = if sum.status == KindStatus::NeedsYou {
        AMBER.with_alpha((if hovered { 0.22 } else { 0.16 }) * cx.alpha)
    } else {
        cx.wash(if hovered { 0.12 } else { 0.07 })
    };
    f.fill_pill(&rounded(rect, 14.0 * k), bg);
    push_clipped(sc, hits, rect, Action::AgentOpen(0));

    // A closed app looks closed: icon and name faded.
    let fade = if offline { 0.5 } else { 1.0 };
    badge(
        f,
        (l + 27.0 * k, t + 28.0 * k),
        15.0 * k,
        CLAUDE_AUMID,
        "Claude",
        cx.alpha * fade,
    );

    let (chip_text, chip_color) = match sum.status {
        KindStatus::Working => ("Working", cx.accent()),
        KindStatus::NeedsYou => ("Needs you", AMBER.fade(cx.alpha)),
        KindStatus::Idle => ("Idle", GREEN.fade(cx.alpha)),
        KindStatus::Offline => ("Offline", cx.dim()),
    };
    let chip_w = f.measure(chip_text, 11.0 * k, true) + 30.0 * k;
    let chip = (
        r - 12.0 * k - chip_w,
        t + 10.0 * k,
        r - 12.0 * k,
        t + 32.0 * k,
    );
    f.fill_pill(
        &rounded(chip, 11.0 * k),
        chip_color.with_alpha(0.16 * cx.alpha),
    );
    f.fill_ellipse(
        chip.0 + 12.0 * k,
        (chip.1 + chip.3) / 2.0,
        3.5 * k,
        3.5 * k,
        chip_color,
    );
    f.text(
        chip_text,
        chip.0 + 20.0 * k,
        chip.1,
        chip.2 - 8.0 * k,
        chip.3,
        label(11.0 * k, true, chip_color, Align::Left),
    );

    let name_color = if offline { cx.dim() } else { cx.text() };
    f.text(
        "Claude Code",
        l + 52.0 * k,
        t + 9.0 * k,
        chip.0 - 8.0 * k,
        t + 28.0 * k,
        label(14.0 * k, true, name_color, Align::Left),
    );
    let line_color = match sum.status {
        KindStatus::NeedsYou => AMBER.fade(cx.alpha),
        KindStatus::Working => cx.text().with_alpha(0.85 * cx.alpha),
        _ => cx.dim(),
    };
    f.text(
        &sum.line(now),
        l + 52.0 * k,
        t + 29.0 * k,
        r - 14.0 * k,
        t + 47.0 * k,
        label(11.5 * k, false, line_color, Align::Left),
    );
    if offline {
        // No limits while it is closed: they would be old.
        return;
    }

    let (il, ir) = (l + 14.0 * k, r - 14.0 * k);
    let y0 = t + 56.0 * k;
    f.fill_pill(&rounded((il, y0, ir, y0 + 1.0 * k), 0.5 * k), cx.wash(0.10));
    let usage = board
        .usage
        .get("claude")
        .filter(|u| u.five_hour.is_some() || u.seven_day.is_some());
    let Some(u) = usage else {
        f.text(
            a.usage_note,
            il,
            y0 + 4.0 * k,
            ir,
            y0 + 23.0 * k,
            label(10.5 * k, false, cx.dim(), Align::Left),
        );
        return;
    };
    let stale = u.updated_ms > 0 && now - u.updated_ms > 10 * 60_000;
    let rows = [("5-hour", u.five_hour), ("Weekly", u.seven_day)];
    for (i, (name, limit)) in rows.iter().enumerate() {
        let (rt, rb) = (
            y0 + (7.0 + 22.0 * i as f32) * k,
            y0 + (27.0 + 22.0 * i as f32) * k,
        );
        let mut when = limit_when(*limit, now);
        if stale && i == 0 {
            when = format!("as of {}", ago(now, u.updated_ms));
        }
        limit_bar(f, cx, (il, rt, ir, rb), name, *limit, &when);
    }
}

/// "resets in 2 h 9 min", or nothing when the reset time is unknown or passed.
fn limit_when(limit: Option<Limit>, now: i64) -> String {
    limit
        .and_then(|l| l.resets_at_ms)
        .filter(|r| *r > now)
        .map_or(String::new(), |r| {
            format!("resets in {}", script::until(now, r))
        })
}

/// One usage row: name, bar, share used, when it resets. Amber from 80 %, red from 95 %.
fn limit_bar(
    f: &mut Frame<'_>,
    cx: &Ctx<'_>,
    (l, t, r, b): Rect,
    name: &str,
    limit: Option<Limit>,
    when: &str,
) {
    let k = cx.k;
    let mid = (t + b) / 2.0;
    f.text(
        name,
        l,
        t,
        l + 54.0 * k,
        b,
        label(10.5 * k, false, cx.dim(), Align::Left),
    );
    let when_w = 112.0 * k;
    let pct_w = 40.0 * k;
    let (bl, br) = (l + 56.0 * k, r - when_w - pct_w - 6.0 * k);
    let bar = (bl, mid - 3.0 * k, br, mid + 3.0 * k);
    f.fill_pill(&rounded(bar, 3.0 * k), cx.wash(0.14));
    let pct = limit.map(|l| l.used_pct.clamp(0.0, 100.0));
    if let Some(p) = pct {
        let color = if p >= 95.0 {
            RED.fade(cx.alpha)
        } else if p >= 80.0 {
            AMBER.fade(cx.alpha)
        } else {
            cx.accent()
        };
        let fill = bl + (br - bl) * p / 100.0;
        if fill > bl + 1.0 * k {
            f.fill_pill(&rounded((bl, bar.1, fill, bar.3), 3.0 * k), color);
        }
    }
    f.text(
        &pct.map_or("–".to_string(), |p| format!("{p:.0}%")),
        br,
        t,
        br + pct_w,
        b,
        label(11.5 * k, true, cx.text(), Align::Right),
    );
    f.text(
        when,
        r - when_w,
        t,
        r,
        b,
        label(10.5 * k, false, cx.dim(), Align::Right),
    );
}

fn has_limits(kind: &str, board: &Board) -> bool {
    board
        .usage
        .get(kind)
        .is_some_and(|u| u.five_hour.is_some() || u.seven_day.is_some())
}

/// The usage card of one agent: its name and when the 5-hour window resets on top, then a bar
/// for each limit with how much of it is used. Amber from 80 %, red from 95 %.
fn limits_row(f: &mut Frame<'_>, cx: &Ctx<'_>, rect: Rect, kind: &str, board: &Board, now: i64) {
    let k = cx.k;
    let (l, t, r, b) = rect;
    let Some(u) = board.usage.get(kind) else {
        return;
    };
    f.fill_pill(&rounded(rect, 10.0 * k), cx.wash(0.06));
    let (il, ir) = (l + 10.0 * k, r - 10.0 * k);
    f.text(
        &format!("{} usage", script::display_name(kind)),
        il,
        t + 4.0 * k,
        ir,
        t + 19.0 * k,
        label(11.5 * k, true, cx.text(), Align::Left),
    );
    let mut when = Vec::new();
    if let Some(rs) = u
        .five_hour
        .and_then(|l| l.resets_at_ms)
        .filter(|r| *r > now)
    {
        when.push(format!("5 h resets in {}", script::until(now, rs)));
    }
    if u.updated_ms > 0 && now - u.updated_ms > 10 * 60_000 {
        // The numbers come with each status line refresh; old ones say so.
        when.push(format!("as of {}", ago(now, u.updated_ms)));
    }
    f.text(
        &when.join(" · "),
        il,
        t + 4.0 * k,
        ir,
        t + 19.0 * k,
        label(10.5 * k, false, cx.dim(), Align::Right),
    );
    let limits = [("5 h", u.five_hour), ("7 d", u.seven_day)];
    let gap = 14.0 * k;
    let col_w = (ir - il - gap) / 2.0;
    let (lt, lb) = (t + 21.0 * k, b - 4.0 * k);
    let mid = (lt + lb) / 2.0;
    for (i, (name, limit)) in limits.iter().enumerate() {
        let x0 = il + (col_w + gap) * i as f32;
        let x1 = x0 + col_w;
        f.text(
            name,
            x0,
            lt,
            x0 + 26.0 * k,
            lb,
            label(10.5 * k, false, cx.dim(), Align::Left),
        );
        let pct = limit.map(|l| l.used_pct.clamp(0.0, 100.0));
        f.text(
            &pct.map_or("–".to_string(), |p| format!("{p:.0}%")),
            x1 - 36.0 * k,
            lt,
            x1,
            lb,
            label(11.0 * k, true, cx.text(), Align::Right),
        );
        let (bl, br) = (x0 + 26.0 * k, x1 - 40.0 * k);
        let bar = (bl, mid - 2.5 * k, br, mid + 2.5 * k);
        f.fill_pill(&rounded(bar, 2.5 * k), cx.wash(0.16));
        if let Some(p) = pct {
            let color = if p >= 95.0 {
                RED.fade(cx.alpha)
            } else if p >= 80.0 {
                AMBER.fade(cx.alpha)
            } else {
                cx.accent()
            };
            let fill = bl + (br - bl) * p / 100.0;
            if fill > bl + 1.0 * k {
                f.fill_pill(&rounded((bl, bar.1, fill, bar.3), 2.5 * k), color);
            }
        }
    }
}

fn ago(now_ms: i64, then_ms: i64) -> String {
    let e = script::elapsed(now_ms, then_ms);
    if e == "now" {
        "just now".into()
    } else {
        format!("{e} ago")
    }
}

/// Agents that have a command Pillbox can start a new chat with.
pub fn can_new_chat(agent: &str) -> bool {
    matches!(agent, "claude" | "codex")
}

pub fn activity(
    f: &mut Frame<'_>,
    cx: &Ctx<'_>,
    (cl, ct, cr, cb): Rect,
    hits: &mut Vec<Hit>,
) -> f32 {
    let k = cx.k;
    let a = &cx.ui.activity;
    let board = a.board;
    let agents: Vec<&Agent> = board
        .ordered_agents()
        .into_iter()
        .filter(|x| !a.hidden.contains(&x.agent))
        .collect();
    let area = (cl - 6.0 * k, ct, cr + 6.0 * k, cb);
    // Claude always has its card on top (unless hidden in Settings): Offline, Idle, Working or
    // Needs you, with its limits while it runs.
    let claude_card = !a.hidden.iter().any(|h| h == "claude");
    // Limits rows for the other agents on the board and for every other agent that reported
    // some, so the usage stays in view between chats.
    let mut kinds: Vec<&str> = Vec::new();
    for ag in &agents {
        if ag.agent != "claude" && !kinds.contains(&ag.agent.as_str()) {
            kinds.push(&ag.agent);
        }
    }
    let mut reported: Vec<&str> = board
        .usage
        .iter()
        .filter(|(_, u)| u.five_hour.is_some() || u.seven_day.is_some())
        .map(|(k, _)| k.as_str())
        .filter(|k| *k != "claude" && !a.hidden.iter().any(|h| h == k))
        .collect();
    reported.sort_unstable();
    for kind in reported {
        if !kinds.contains(&kind) {
            kinds.push(kind);
        }
    }
    if !claude_card && agents.is_empty() && board.statuses.is_empty() && kinds.is_empty() {
        note(
            f,
            cx,
            area,
            "Nothing running. Coding agents and scripts show up here. Try: pillboxctl flash \"hello\"  (README, M7).",
        );
        return 0.0;
    }
    let mut rows: Vec<(Row<'_>, f32)> = Vec::new();
    if claude_card {
        let summary = board.kind_summary("claude", a.claude_running);
        let h = if summary.status == KindStatus::Offline {
            CLAUDE_OFFLINE_H
        } else if has_limits("claude", board) {
            CLAUDE_LIMITS_H
        } else {
            CLAUDE_NO_LIMITS_H
        };
        rows.push((Row::Claude, h * k));
    }
    // Limits only make sense for the agents that have them (or that have reported some).
    for kind in &kinds {
        if *kind == "codex" || board.usage.contains_key(*kind) {
            let h = if has_limits(kind, board) { 42.0 } else { 18.0 };
            rows.push((Row::Limits(kind), h * k));
        }
    }
    if claude_card && !agents.is_empty() {
        rows.push((Row::Heading("Sessions"), 22.0 * k));
    }
    for ag in &agents {
        let open = a.expanded == Some(ag.uid);
        let steps = if open { ag.steps.len().min(6) } else { 0 };
        let extra = if open { 8.0 + 14.0 * steps as f32 } else { 0.0 };
        rows.push((Row::Agent(ag), (60.0 + extra) * k));
    }
    if !board.statuses.is_empty() {
        if !agents.is_empty() || claude_card {
            rows.push((Row::Heading("Builds and scripts"), 22.0 * k));
        }
        for s in &board.statuses {
            rows.push((Row::Status(s), 44.0 * k));
        }
    }
    let gap = 5.0 * k;
    let total: f32 = rows.iter().map(|(_, h)| h + gap).sum();
    let (sc, max) = Scroller::new(area.1, area.3, total, cx.ui.scroll);
    sc.clip(f, area.0, area.2, max, cx.k);
    let mut y = 0.0;
    for (row, h) in &rows {
        let t = sc.y(y);
        y += h + gap;
        if !sc.visible(t, t + h) {
            continue;
        }
        let rect = (area.0, t, area.2, t + h);
        match row {
            Row::Heading(text) => f.text(
                text,
                rect.0 + 8.0 * k,
                t + 4.0 * k,
                rect.2,
                t + h,
                label(11.0 * k, true, cx.dim(), Align::Left),
            ),
            Row::Claude => claude_card_row(f, cx, &sc, rect, hits),
            Row::Limits(kind) if has_limits(kind, board) => {
                limits_row(f, cx, rect, kind, board, a.now_ms);
            }
            Row::Limits(kind) => f.text(
                &limits_text(kind, board, a.now_ms),
                rect.0 + 8.0 * k,
                t,
                rect.2 - 6.0 * k,
                t + h,
                label(11.0 * k, false, cx.dim(), Align::Left),
            ),
            Row::Agent(ag) => agent_row(f, cx, &sc, rect, ag, hits),
            Row::Status(s) => status_row(f, cx, &sc, rect, s, hits),
        }
    }
    f.pop_fade_clip();
    max
}

fn agent_row(
    f: &mut Frame<'_>,
    cx: &Ctx<'_>,
    sc: &Scroller,
    rect: Rect,
    ag: &Agent,
    hits: &mut Vec<Hit>,
) {
    let k = cx.k;
    let a = &cx.ui.activity;
    let now = a.now_ms;
    let (l, t, r, _b) = rect;
    let visible = sc.clip_hit(rect).is_some();
    let hovered = cx.hovered(rect);
    let needs = ag.state == AgentState::NeedsYou;
    let bg = if needs {
        AMBER.with_alpha((if hovered { 0.22 } else { 0.16 }) * cx.alpha)
    } else {
        cx.wash(if hovered { 0.12 } else { 0.06 })
    };
    f.fill_pill(&rounded(rect, 10.0 * k), bg);
    // The whole card toggles the timeline; buttons are pushed after it, so they win.
    push_clipped(sc, hits, rect, Action::AgentExpand(ag.uid));
    let dot = match ag.state {
        AgentState::Working => cx.accent(),
        AgentState::NeedsYou => AMBER.fade(cx.alpha),
        AgentState::Done => GREEN.fade(cx.alpha),
        AgentState::Idle => cx.dim(),
    };
    f.fill_ellipse(l + 14.0 * k, t + 16.0 * k, 4.5 * k, 4.5 * k, dot);

    let title = if ag.project.is_empty() {
        script::display_name(&ag.agent)
    } else {
        ag.project.clone()
    };
    let btn = 24.0 * k;
    let mid = t + 16.0 * k;
    let mut right = r - 8.0 * k;
    if hovered && visible {
        icon_button(
            f,
            cx,
            (right - btn / 2.0, mid),
            btn,
            G_CLOSE,
            cx.dim(),
            Action::ActivityDismiss(ag.uid),
            hits,
        );
        right -= btn;
        if ag.pid.is_some() {
            if a.confirm_stop == Some(ag.uid) {
                let w = 54.0 * k;
                let pill = (right - w, mid - 10.0 * k, right, mid + 10.0 * k);
                f.fill_pill(&rounded(pill, 10.0 * k), RED.fade(cx.alpha));
                f.text(
                    "Stop?",
                    pill.0,
                    pill.1,
                    pill.2,
                    pill.3,
                    label(
                        11.5 * k,
                        true,
                        Rgba::rgb(0xFFFFFF).fade(cx.alpha),
                        Align::Center,
                    ),
                );
                hits.push(Hit {
                    rect: pill,
                    action: Action::AgentStop(ag.uid),
                });
                right -= w;
            } else {
                icon_button(
                    f,
                    cx,
                    (right - btn / 2.0, mid),
                    btn,
                    G_STOP,
                    cx.dim(),
                    Action::AgentStop(ag.uid),
                    hits,
                );
                right -= btn;
            }
        }
        if ag.agent == "claude" {
            icon_button(
                f,
                cx,
                (right - btn / 2.0, mid),
                btn,
                G_OPEN_APP,
                cx.dim(),
                Action::AgentOpen(ag.uid),
                hits,
            );
            right -= btn;
        }
        if can_new_chat(&ag.agent) && !ag.cwd.is_empty() {
            icon_button(
                f,
                cx,
                (right - btn / 2.0, mid),
                btn,
                G_CHAT,
                cx.dim(),
                Action::AgentNewChat(ag.uid),
                hits,
            );
            right -= btn;
        }
    } else {
        let turn_start = ag.steps.front().map_or(ag.started_ms, |s| s.at_ms);
        let when = match ag.state {
            AgentState::Working => format!("working {}", script::elapsed(now, turn_start)),
            AgentState::Done => format!("done {}", ago(now, ag.updated_ms)),
            _ => ago(now, ag.updated_ms),
        };
        f.text(
            &when,
            l,
            t + 6.0 * k,
            r - 10.0 * k,
            t + 24.0 * k,
            label(10.5 * k, false, cx.dim(), Align::Right),
        );
        right = r - 96.0 * k;
    }
    f.text(
        &title,
        l + 28.0 * k,
        t + 6.0 * k,
        right - 4.0 * k,
        t + 24.0 * k,
        label(13.0 * k, true, cx.text(), Align::Left),
    );

    // Who, which model, how hard it thinks.
    let mut meta = vec![script::display_name(&ag.agent)];
    if !ag.model.is_empty() {
        meta.push(script::friendly_model(&ag.model));
    }
    if !ag.effort.is_empty() {
        meta.push(format!("{} effort", ag.effort));
    }
    f.text(
        &meta.join(" · "),
        l + 28.0 * k,
        t + 24.0 * k,
        r - 10.0 * k,
        t + 38.0 * k,
        label(10.5 * k, false, cx.dim(), Align::Left),
    );

    // What it is doing, or how it ended, or what it asks.
    let line = match ag.state {
        AgentState::Done if !ag.summary.is_empty() => ag.summary.as_str(),
        AgentState::Done => "Finished",
        AgentState::Idle if ag.doing.is_empty() => "Idle",
        _ => ag.doing.as_str(),
    };
    let line_color = if needs {
        AMBER.fade(cx.alpha)
    } else if ag.state == AgentState::Working {
        cx.text()
    } else {
        cx.dim()
    };
    let buttons_w = if ag.ask.is_some() { 142.0 * k } else { 0.0 };
    f.text(
        line,
        l + 28.0 * k,
        t + 38.0 * k,
        r - 10.0 * k - buttons_w,
        t + 56.0 * k,
        label(11.5 * k, false, line_color, Align::Left),
    );
    if let Some(ask) = &ag.ask
        && visible
    {
        let (bt, bb) = (t + 36.0 * k, t + 56.0 * k);
        let bw = 64.0 * k;
        let allow = (r - 8.0 * k - bw, bt, r - 8.0 * k, bb);
        let deny = (allow.0 - 6.0 * k - bw, bt, allow.0 - 6.0 * k, bb);
        button(
            f,
            cx,
            deny,
            G_CLOSE,
            "Deny",
            false,
            Action::AgentDeny(ask.id),
            hits,
        );
        button(
            f,
            cx,
            allow,
            G_CHECK,
            "Allow",
            true,
            Action::AgentAllow(ask.id),
            hits,
        );
    }

    // The timeline, newest at the bottom.
    if a.expanded == Some(ag.uid) {
        let shown = ag.steps.len().min(6);
        let skip = ag.steps.len() - shown;
        for (i, step) in ag.steps.iter().skip(skip).enumerate() {
            let y = t + 62.0 * k + 14.0 * k * i as f32;
            f.text(
                &format!("•  {}", step.text),
                l + 28.0 * k,
                y,
                r - 70.0 * k,
                y + 14.0 * k,
                label(10.5 * k, false, cx.dim(), Align::Left),
            );
            f.text(
                &script::elapsed(now, step.at_ms),
                r - 66.0 * k,
                y,
                r - 10.0 * k,
                y + 14.0 * k,
                label(
                    10.0 * k,
                    false,
                    cx.dim().with_alpha(0.7 * cx.alpha),
                    Align::Right,
                ),
            );
        }
    }
}

fn status_row(
    f: &mut Frame<'_>,
    cx: &Ctx<'_>,
    sc: &Scroller,
    rect: Rect,
    s: &Status,
    hits: &mut Vec<Hit>,
) {
    let k = cx.k;
    let now = cx.ui.activity.now_ms;
    let (l, t, r, b) = rect;
    let hovered = cx.hovered(rect);
    f.fill_pill(
        &rounded(rect, 10.0 * k),
        cx.wash(if hovered { 0.12 } else { 0.06 }),
    );
    let (glyph, color) = match s.state {
        StatusState::Running => (G_SYNC, cx.accent()),
        StatusState::Ok => (G_CHECK, GREEN.fade(cx.alpha)),
        StatusState::Failed => (G_CLOSE, RED.fade(cx.alpha)),
        StatusState::Info => (G_INFO, cx.dim()),
    };
    f.text(
        glyph,
        l + 6.0 * k,
        t,
        l + 32.0 * k,
        t + 36.0 * k,
        icon(13.0 * k, color),
    );
    let right = if hovered { r - 40.0 * k } else { r - 80.0 * k };
    f.text(
        &s.title,
        l + 36.0 * k,
        t + 5.0 * k,
        right,
        t + 22.0 * k,
        label(13.0 * k, true, cx.text(), Align::Left),
    );
    let detail = if s.detail.is_empty() {
        match s.state {
            StatusState::Running => "Running",
            StatusState::Ok => "Done",
            StatusState::Failed => "Failed",
            StatusState::Info => "",
        }
    } else {
        s.detail.as_str()
    };
    let detail_color = if s.state == StatusState::Failed {
        RED.fade(cx.alpha)
    } else {
        cx.dim()
    };
    f.text(
        detail,
        l + 36.0 * k,
        t + 22.0 * k,
        r - 10.0 * k,
        t + 38.0 * k,
        label(10.5 * k, false, detail_color, Align::Left),
    );
    if hovered {
        if sc.clip_hit(rect).is_some() {
            icon_button(
                f,
                cx,
                (r - 18.0 * k, t + 14.0 * k),
                24.0 * k,
                G_CLOSE,
                cx.dim(),
                Action::ActivityDismiss(s.uid),
                hits,
            );
        }
    } else {
        f.text(
            &ago(now, s.updated_ms),
            l,
            t + 5.0 * k,
            r - 10.0 * k,
            t + 22.0 * k,
            label(10.5 * k, false, cx.dim(), Align::Right),
        );
    }
    if let Some(p) = s.progress {
        let (x0, x1) = (l + 36.0 * k, r - 12.0 * k);
        let y = b - 6.0 * k;
        f.fill_pill(
            &rounded((x0, y - 1.5 * k, x1, y + 1.5 * k), 1.5 * k),
            cx.wash(0.16),
        );
        let fill = x0 + (x1 - x0) * f32::from(p) / 100.0;
        if fill > x0 + 2.0 * k {
            f.fill_pill(
                &rounded((x0, y - 1.5 * k, fill, y + 1.5 * k), 1.5 * k),
                cx.accent(),
            );
        }
    }
}
