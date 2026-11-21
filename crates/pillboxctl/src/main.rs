//! `pillboxctl`: the command line for Pillbox's local script API (M7).
//!
//! It talks to `http://127.0.0.1:<port>/v1/...` with the token Pillbox keeps in
//! `data\api.json`. Hooks of coding agents run it on every tool call, so in `hook` mode it
//! never fails and never waits long: if Pillbox is not running it exits 0 at once.
//!
//! Exit codes: 0 done, 1 Pillbox refused, 2 wrong usage, 3 Pillbox is not running.

mod hooks;
mod platform;
mod statusline;

use std::{
    collections::{HashMap, HashSet},
    io::Read,
    path::PathBuf,
    process::ExitCode,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde_json::{Value, json};

const HELP: &str = "\
pillboxctl: talk to Pillbox (the floating pill for Windows 10 and 11).

USAGE
  pillboxctl flash <text> [--source NAME] [--ms N] [--urgent]
      A short line on the closed pill. A new flash with the same --source replaces the old one.
  pillboxctl status <id> [--title T] [--detail D] [--state running|ok|failed|info]
                        [--progress 0-100] [--remove]
      A live entry on the Activity tab. The same id updates it in place.
  pillboxctl agent <name> [--session S] [--state working|needs-you|done|idle] [--doing T]
                         [--summary T] [--step T] [--cwd DIR] [--model M] [--effort E]
                         [--pid N] [--end]
      Report any agent by name (a custom one appears on its first call).
  pillboxctl hook <claude|codex> [--reply]
      The hook command for Claude Code / Codex: reads the hook JSON on stdin. Never fails.
  pillboxctl statusline <claude|codex> [--apply]
      Use as a status line command: sends the limits to Pillbox and prints a short line,
      or the line of the status line command it replaced. --apply puts it into Claude Code's
      settings (after a backup) and keeps the old status line running behind it.
  pillboxctl hooks <claude|codex> [--reply] [--apply]
      Prints the hook entries to add to the agent's settings (or adds them with --apply,
      after making a backup).
  pillboxctl ping

Add --quiet to any command to hide errors (the exit code still tells).
Pillbox must be running, with 'script.enabled' on (the default).
";

#[derive(Debug)]
enum Fail {
    Usage(String),
    NotRunning,
    Refused(String),
}

impl Fail {
    fn code(&self) -> u8 {
        match self {
            Fail::Refused(_) => 1,
            Fail::Usage(_) => 2,
            Fail::NotRunning => 3,
        }
    }

    fn message(&self) -> String {
        match self {
            Fail::Usage(m) | Fail::Refused(m) => m.clone(),
            Fail::NotRunning => "the Pillbox is not running (or its script API is off)".into(),
        }
    }
}

// ---- arguments ---------------------------------------------------------------------------------

const VALUE_FLAGS: [&str; 13] = [
    "source", "ms", "title", "detail", "state", "progress", "session", "doing", "summary", "step",
    "cwd", "model", "effort",
];
const SWITCHES: [&str; 8] = [
    "urgent", "remove", "end", "quiet", "reply", "apply", "help", "pid-auto",
];

#[derive(Debug, Default)]
struct Opts {
    positional: Vec<String>,
    values: HashMap<String, String>,
    switches: HashSet<String>,
}

impl Opts {
    fn value(&self, k: &str) -> Option<&str> {
        self.values.get(k).map(String::as_str)
    }

    fn has(&self, k: &str) -> bool {
        self.switches.contains(k)
    }
}

fn parse_args(args: &[String]) -> Result<Opts, Fail> {
    let mut o = Opts::default();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let Some(name) = a.strip_prefix("--") else {
            o.positional.push(a.clone());
            continue;
        };
        if name == "pid" {
            let v = it
                .next()
                .ok_or_else(|| Fail::Usage("--pid needs a number".into()))?;
            o.values.insert("pid".into(), v.clone());
        } else if VALUE_FLAGS.contains(&name) {
            let v = it
                .next()
                .ok_or_else(|| Fail::Usage(format!("--{name} needs a value")))?;
            o.values.insert(name.to_string(), v.clone());
        } else if SWITCHES.contains(&name) {
            o.switches.insert(name.to_string());
        } else {
            return Err(Fail::Usage(format!(
                "unknown option --{name} (try: pillboxctl help)"
            )));
        }
    }
    Ok(o)
}

// ---- talking to Pillbox ----------------------------------------------------------------------

struct Info {
    port: u16,
    token: String,
}

/// Pillbox's settings folder: `PILLBOX_DATA_DIR`, else `%APPDATA%\Pillbox`.
fn settings_folder() -> Option<PathBuf> {
    Some(match std::env::var_os("PILLBOX_DATA_DIR") {
        Some(d) => PathBuf::from(d),
        None => PathBuf::from(std::env::var_os("APPDATA")?).join("Pillbox"),
    })
}

/// `<settings folder>\data\api.json`; the same folder logic as Pillbox's own.
fn info_path() -> Option<PathBuf> {
    Some(settings_folder()?.join("data").join("api.json"))
}

fn read_info() -> Result<Info, Fail> {
    let text = info_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .ok_or(Fail::NotRunning)?;
    let v: Value = serde_json::from_str(&text).map_err(|_| Fail::NotRunning)?;
    let port = v
        .get("port")
        .and_then(Value::as_u64)
        .and_then(|p| u16::try_from(p).ok())
        .ok_or(Fail::NotRunning)?;
    let token = v
        .get("token")
        .and_then(Value::as_str)
        .ok_or(Fail::NotRunning)?
        .to_string();
    Ok(Info { port, token })
}

struct Reply {
    status: u16,
    body: String,
}

fn request(
    info: &Info,
    method: &str,
    path: &str,
    body: Option<&str>,
    pid: Option<u32>,
    timeout: Duration,
) -> Result<Reply, Fail> {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_millis(300))
        .timeout(timeout)
        .build();
    let mut req = agent
        .request(method, &format!("http://127.0.0.1:{}/v1/{path}", info.port))
        .set("Authorization", &format!("Bearer {}", info.token));
    if let Some(p) = pid {
        req = req.set("X-Pillbox-Pid", &p.to_string());
    }
    let result = match body {
        Some(b) => req.set("Content-Type", "application/json").send_string(b),
        None => req.call(),
    };
    match result {
        Ok(r) => Ok(Reply {
            status: r.status(),
            body: r.into_string().unwrap_or_default(),
        }),
        Err(ureq::Error::Status(code, r)) => Ok(Reply {
            status: code,
            body: r.into_string().unwrap_or_default(),
        }),
        // Nothing listens on that port: Pillbox quit and left an old file, or it is not mine.
        Err(ureq::Error::Transport(t)) if t.kind() == ureq::ErrorKind::ConnectionFailed => {
            Err(Fail::NotRunning)
        }
        Err(ureq::Error::Transport(t)) => Err(Fail::Refused(format!("no answer: {t}"))),
    }
}

/// Send a JSON command and turn Pillbox's answer into success or a message.
fn send(path: &str, body: &Value, pid: Option<u32>) -> Result<(), Fail> {
    let info = read_info()?;
    let r = request(
        &info,
        "POST",
        path,
        Some(&body.to_string()),
        pid,
        Duration::from_secs(3),
    )?;
    match r.status {
        200 | 204 => Ok(()),
        404 => Err(Fail::Refused(
            "Pillbox did not accept the token (restarted? it will work again on the next call)"
                .into(),
        )),
        _ => {
            let why = serde_json::from_str::<Value>(&r.body)
                .ok()
                .and_then(|v| v.get("error")?.as_str().map(str::to_string))
                .unwrap_or(r.body);
            Err(Fail::Refused(format!("refused: {why}")))
        }
    }
}

/// The process that started this hook: the first `claude.exe` / `codex.exe` above us.
fn agent_pid(agent: &str) -> Option<u32> {
    let want = format!("{agent}.exe");
    let procs: HashMap<u32, platform::Proc> = platform::processes()
        .into_iter()
        .map(|p| (p.pid, p))
        .collect();
    let mut at = std::process::id();
    for _ in 0..16 {
        let p = procs.get(&at)?;
        if p.exe == want {
            return Some(p.pid);
        }
        if p.parent == at || p.parent == 0 {
            return None;
        }
        at = p.parent;
    }
    None
}

fn read_stdin() -> String {
    let mut raw = Vec::new();
    let _ = std::io::stdin().read_to_end(&mut raw);
    decode_text(&raw)
}

/// Text from a pipe. Windows tools disagree about encodings: Claude Code and Codex send
/// UTF-8, but a PowerShell pipeline may add a byte-order mark or send UTF-16.
fn decode_text(raw: &[u8]) -> String {
    if let Some(rest) = raw.strip_prefix(&[0xFF, 0xFE]) {
        let units: Vec<u16> = rest
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_le_bytes(*c))
            .collect();
        return String::from_utf16_lossy(&units);
    }
    let raw = raw.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(raw);
    String::from_utf8_lossy(raw).into_owned()
}

fn known_agent(name: Option<&String>) -> Result<&str, Fail> {
    match name.map(String::as_str) {
        Some(a @ ("claude" | "codex")) => Ok(a),
        _ => Err(Fail::Usage("name the agent: claude or codex".into())),
    }
}

// ---- commands ----------------------------------------------------------------------------------

fn cmd_flash(o: &Opts) -> Result<(), Fail> {
    let text = o.positional.get(1..).unwrap_or_default().join(" ");
    if text.trim().is_empty() {
        return Err(Fail::Usage("pillboxctl flash <text>".into()));
    }
    let mut body = json!({ "text": text, "urgent": o.has("urgent") });
    if let Some(s) = o.value("source") {
        body["source"] = json!(s);
    }
    if let Some(ms) = o.value("ms") {
        let ms: u32 = ms
            .parse()
            .map_err(|_| Fail::Usage("--ms must be a number of milliseconds".into()))?;
        body["ms"] = json!(ms);
    }
    send("flash", &body, None)
}

fn cmd_status(o: &Opts) -> Result<(), Fail> {
    let id = o
        .positional
        .get(1)
        .ok_or_else(|| Fail::Usage("pillboxctl status <id> [--title ...]".into()))?;
    let mut body = json!({ "id": id, "remove": o.has("remove") });
    for k in ["title", "detail", "state"] {
        if let Some(v) = o.value(k) {
            body[k] = json!(v);
        }
    }
    if let Some(p) = o.value("progress") {
        let p: u8 = p
            .parse()
            .map_err(|_| Fail::Usage("--progress must be 0 to 100".into()))?;
        body["progress"] = json!(p.min(100));
    }
    send("status", &body, None)
}

fn cmd_agent(o: &Opts) -> Result<(), Fail> {
    let name = o
        .positional
        .get(1)
        .ok_or_else(|| Fail::Usage("pillboxctl agent <name> [--state ...]".into()))?;
    let mut body = json!({ "agent": name, "ended": o.has("end") });
    for k in [
        "session", "state", "doing", "summary", "step", "cwd", "model", "effort",
    ] {
        if let Some(v) = o.value(k) {
            body[k] = json!(v);
        }
    }
    if let Some(p) = o.value("pid") {
        let p: u32 = p
            .parse()
            .map_err(|_| Fail::Usage("--pid must be a process id".into()))?;
        body["pid"] = json!(p);
    }
    send("agent", &body, None)
}

/// Hook mode: whatever happens, the agent must not notice us. Always exit 0.
fn cmd_hook(o: &Opts) -> ExitCode {
    let Ok(agent) = known_agent(o.positional.get(1)) else {
        return ExitCode::SUCCESS;
    };
    let body = read_stdin();
    if body.trim().is_empty() {
        return ExitCode::SUCCESS;
    }
    let Ok(info) = read_info() else {
        return ExitCode::SUCCESS;
    };
    // The reply hook holds a permission question until Pillbox answers (or gives up).
    let ask = o.has("reply") && body.contains("\"PermissionRequest\"");
    let path = if ask {
        format!("hook/{agent}?wait=600")
    } else {
        format!("hook/{agent}")
    };
    let timeout = Duration::from_secs(if ask { 620 } else { 2 });
    // Looking for the agent's process costs a few milliseconds; only the events that can end
    // in a Stop button need it.
    let pid = if body.contains("\"SessionStart\"") || body.contains("\"UserPromptSubmit\"") || ask {
        agent_pid(agent)
    } else {
        None
    };
    if let Ok(r) = request(&info, "POST", &path, Some(&body), pid, timeout)
        && ask
        && r.status == 200
        && !r.body.is_empty()
    {
        println!("{}", r.body);
    }
    ExitCode::SUCCESS
}

/// Status line mode: forward the limits, print "Claude 5h 42%" (or the replaced status line's
/// own output). Never fails: a status line that errors shows nothing.
fn cmd_statusline(o: &Opts) -> ExitCode {
    let agent = known_agent(o.positional.get(1)).unwrap_or("claude");
    let body = read_stdin();
    let parsed: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
    // The old status line runs while the limits go to Pillbox.
    let chained = statusline::chained(agent).and_then(|c| statusline::start(&c, &body));
    if let Ok(info) = read_info() {
        let _ = request(
            &info,
            "POST",
            &format!("statusline/{agent}"),
            Some(&body),
            None,
            Duration::from_secs(1),
        );
    }
    if let Some(line) = chained.and_then(|r| r.output(Duration::from_secs(3))) {
        println!("{line}");
        return ExitCode::SUCCESS;
    }
    let name = if agent == "codex" { "Codex" } else { "Claude" };
    let five = parsed
        .pointer("/rate_limits/five_hour/used_percentage")
        .and_then(Value::as_f64);
    match five {
        Some(p) => println!("{name} 5h {p:.0}%"),
        None => println!("{name}"),
    }
    ExitCode::SUCCESS
}

fn settings_target(agent: &str) -> Option<PathBuf> {
    let home = PathBuf::from(std::env::var_os("USERPROFILE")?);
    Some(match agent {
        "codex" => home.join(".codex").join("hooks.json"),
        _ => home.join(".claude").join("settings.json"),
    })
}

fn cmd_hooks(o: &Opts) -> Result<(), Fail> {
    let agent = known_agent(o.positional.get(1))?;
    let exe = std::env::current_exe()
        .map_err(|e| Fail::Refused(format!("cannot find myself: {e}")))?
        .display()
        .to_string();
    let ours = hooks::snippet(agent, &exe, o.has("reply"));
    let target =
        settings_target(agent).ok_or_else(|| Fail::Refused("USERPROFILE is not set".into()))?;
    if !o.has("apply") {
        let doc = json!({ "hooks": ours });
        println!(
            "Add this to {} (next to any hooks you already have):\n",
            target.display()
        );
        println!("{}", serde_json::to_string_pretty(&doc).unwrap_or_default());
        println!(
            "\nOr let pillboxctl do it, with a backup first:\n  pillboxctl hooks {agent}{} --apply",
            if o.has("reply") { " --reply" } else { "" }
        );
        if agent == "claude" {
            println!(
                "\nTo show your usage limits on the Activity tab, set the \"statusLine\" command to:\n  \"{exe}\" statusline claude\nor let pillboxctl do it (your old status line keeps running behind it):\n  pillboxctl statusline claude --apply"
            );
        }
        return Ok(());
    }
    let mut doc = read_settings(&target)?;
    let added = hooks::merge(&mut doc, &ours);
    if added.is_empty() {
        println!(
            "Already connected: {} has Pillbox's hooks.",
            target.display()
        );
        return Ok(());
    }
    println!("Adding hooks for: {}", added.join(", "));
    write_with_backup(&target, &doc)?;
    println!("Done. New chats pick the hooks up; restart running ones.");
    Ok(())
}

/// Read an agent's settings file; a missing one is empty, a broken one is left alone.
fn read_settings(target: &std::path::Path) -> Result<Value, Fail> {
    let text = match std::fs::read_to_string(target) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => "{}".to_string(),
        Err(e) => {
            return Err(Fail::Refused(format!(
                "cannot read {}: {e}",
                target.display()
            )));
        }
    };
    serde_json::from_str(&text).map_err(|e| {
        Fail::Refused(format!(
            "{} is not valid JSON ({e}); not touching it",
            target.display()
        ))
    })
}

/// `pillboxctl statusline claude --apply`: pillboxctl becomes Claude Code's status line, and the
/// old one keeps running behind it (see `statusline`).
fn cmd_statusline_apply(o: &Opts) -> Result<(), Fail> {
    let agent = known_agent(o.positional.get(1))?;
    if agent != "claude" {
        return Err(Fail::Usage(
            "only Claude Code has a status line command to set".into(),
        ));
    }
    let exe = std::env::current_exe()
        .map_err(|e| Fail::Refused(format!("cannot find myself: {e}")))?
        .display()
        .to_string();
    let target =
        settings_target(agent).ok_or_else(|| Fail::Refused("USERPROFILE is not set".into()))?;
    let mut doc = read_settings(&target)?;
    match statusline::apply(&mut doc, &statusline::our_command(&exe, agent))
        .map_err(Fail::Refused)?
    {
        statusline::Plan::AlreadyOurs => {
            println!(
                "Already connected: the status line in {} is pillboxctl.",
                target.display()
            );
            return Ok(());
        }
        statusline::Plan::Replace { chain } => {
            // Keep the old command first: if this fails, the settings are not touched.
            if let Some(old) = &chain {
                let kept = statusline::save_chain(agent, old).map_err(Fail::Refused)?;
                println!("Your old status line keeps running behind pillboxctl: {old}");
                println!("(kept in {})", kept.display());
            }
        }
    }
    write_with_backup(&target, &doc)?;
    println!(
        "Done. Claude Code now sends its usage limits to the Activity tab; new chats pick it up."
    );
    Ok(())
}

/// Write `doc` to `target` through a temporary file, after copying the old one aside.
fn write_with_backup(target: &std::path::Path, doc: &Value) -> Result<(), Fail> {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    if target.exists() {
        let backup = target.with_extension(format!("json.pillbox-backup-{stamp}"));
        std::fs::copy(target, &backup)
            .map_err(|e| Fail::Refused(format!("could not make a backup: {e}")))?;
        println!("Backup: {}", backup.display());
    }
    if let Some(dir) = target.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let tmp = target.with_extension("json.pillbox-tmp");
    let out = serde_json::to_string_pretty(doc)
        .map_err(|e| Fail::Refused(format!("could not write JSON: {e}")))?;
    std::fs::write(&tmp, out + "\n")
        .and_then(|()| std::fs::rename(&tmp, target))
        .map_err(|e| Fail::Refused(format!("could not write {}: {e}", target.display())))?;
    Ok(())
}

fn cmd_ping() -> Result<(), Fail> {
    let info = read_info()?;
    let r = request(&info, "GET", "ping", None, None, Duration::from_secs(2))?;
    if r.status == 200 {
        println!("The Pillbox is running (port {}).", info.port);
        Ok(())
    } else {
        Err(Fail::Refused(format!("unexpected answer {}", r.status)))
    }
}

fn run(args: &[String], quiet: &mut bool) -> Result<Option<ExitCode>, Fail> {
    let o = parse_args(args)?;
    *quiet = o.has("quiet");
    let cmd = o.positional.first().map(String::as_str).unwrap_or("help");
    if o.has("help") || matches!(cmd, "help" | "-h") {
        print!("{HELP}");
        return Ok(None);
    }
    match cmd {
        "flash" => cmd_flash(&o)?,
        "status" => cmd_status(&o)?,
        "agent" => cmd_agent(&o)?,
        "hooks" => cmd_hooks(&o)?,
        "ping" => cmd_ping()?,
        "hook" => return Ok(Some(cmd_hook(&o))),
        "statusline" if o.has("apply") => cmd_statusline_apply(&o)?,
        "statusline" => return Ok(Some(cmd_statusline(&o))),
        other => {
            return Err(Fail::Usage(format!(
                "unknown command '{other}' (try: pillboxctl help)"
            )));
        }
    }
    Ok(None)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut quiet = args.iter().any(|a| a == "--quiet");
    match run(&args, &mut quiet) {
        Ok(Some(code)) => code,
        Ok(None) => ExitCode::SUCCESS,
        Err(f) => {
            if !quiet {
                eprintln!("pillboxctl: {}", f.message());
            }
            ExitCode::from(f.code())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(s: &str) -> Vec<String> {
        s.split_whitespace().map(str::to_string).collect()
    }

    #[test]
    fn options_and_positionals_separate() {
        let o = parse_args(&args("flash Build done --source cargo --urgent --ms 3000"))
            .expect("parses");
        assert_eq!(o.positional, ["flash", "Build", "done"]);
        assert_eq!(o.value("source"), Some("cargo"));
        assert_eq!(o.value("ms"), Some("3000"));
        assert!(o.has("urgent") && !o.has("remove"));
    }

    #[test]
    fn bad_options_are_usage_errors() {
        assert!(matches!(
            parse_args(&args("flash x --nope")),
            Err(Fail::Usage(_))
        ));
        assert!(matches!(
            parse_args(&args("flash x --source")),
            Err(Fail::Usage(_))
        ));
        assert!(matches!(
            parse_args(&args("agent a --pid")),
            Err(Fail::Usage(_))
        ));
    }

    #[test]
    fn a_value_may_look_like_a_flag() {
        let o = parse_args(&args("status build --detail --weird")).expect("parses");
        assert_eq!(o.value("detail"), Some("--weird"));
    }

    #[test]
    fn exit_codes_are_distinct() {
        let codes = [
            Fail::Refused(String::new()).code(),
            Fail::Usage(String::new()).code(),
            Fail::NotRunning.code(),
        ];
        assert_eq!(codes, [1, 2, 3]);
    }

    #[test]
    fn piped_text_is_decoded_whatever_the_encoding() {
        assert_eq!(decode_text(b"{\"a\":1}"), "{\"a\":1}");
        assert_eq!(decode_text(b"\xEF\xBB\xBF{\"a\":1}"), "{\"a\":1}");
        let utf16: Vec<u8> = [0xFF, 0xFE]
            .into_iter()
            .chain("{\"a\":1}".encode_utf16().flat_map(u16::to_le_bytes))
            .collect();
        assert_eq!(decode_text(&utf16), "{\"a\":1}");
        assert_eq!(decode_text(b""), "");
        assert_eq!(decode_text("é".as_bytes()), "é");
    }

    #[test]
    fn only_known_agents_have_hooks() {
        assert!(known_agent(Some(&"claude".to_string())).is_ok());
        assert!(known_agent(Some(&"codex".to_string())).is_ok());
        assert!(known_agent(Some(&"hermes".to_string())).is_err());
        assert!(known_agent(None).is_err());
    }
}
