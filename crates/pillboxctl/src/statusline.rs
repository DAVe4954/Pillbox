//! Status line mode (`pillboxctl statusline claude`) and putting it into Claude Code's settings
//! (`pillboxctl statusline claude --apply`).
//!
//! Claude Code gives its usage limits (`rate_limits.five_hour` / `seven_day`) only to the
//! `statusLine` command, on stdin. To send them to Pillbox without losing a status line the
//! user already has, `--apply` keeps the old command in `<settings folder>\statusline-chain.json`
//! and puts pillboxctl in its place. On each refresh pillboxctl forwards the JSON to Pillbox,
//! runs the old command with the same stdin, and prints what it printed.

use std::{
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
    sync::mpsc,
    time::Duration,
};

use serde_json::{Value, json};

/// The status line command pillboxctl puts into the settings.
pub fn our_command(exe: &str, agent: &str) -> String {
    format!("\"{exe}\" statusline {agent}")
}

/// Whether a status line command is already pillboxctl's.
pub fn is_ours(command: &str) -> bool {
    let c = command.to_ascii_lowercase();
    c.contains("pillboxctl") && c.contains(" statusline")
}

/// A status line command of the old `islandctl` (the app was named Island).
fn is_legacy(command: &str) -> bool {
    let c = command.to_ascii_lowercase();
    c.contains("islandctl") && c.contains(" statusline")
}

/// What `--apply` does to a settings document.
#[derive(Debug, PartialEq)]
pub enum Plan {
    /// The status line is pillboxctl already; nothing to write.
    AlreadyOurs,
    /// Put pillboxctl in. `chain` is the old command to keep running behind it, if there was one.
    Replace { chain: Option<String> },
}

/// Decide, and (for `Replace`) change `doc` in place. Other fields of an existing `statusLine`
/// (its refresh interval, padding) are kept.
pub fn apply(doc: &mut Value, ours: &str) -> Result<Plan, String> {
    let Some(root) = doc.as_object_mut() else {
        return Err("the settings file is not a JSON object".into());
    };
    let old = root.get("statusLine").cloned();
    let old_command = old
        .as_ref()
        .and_then(|s| s.get("command"))
        .and_then(Value::as_str)
        .map(str::to_string);
    if old_command.as_deref().is_some_and(is_ours) {
        return Ok(Plan::AlreadyOurs);
    }
    // The status line of the tool's old name (`islandctl`) is replaced, not kept behind this
    // one: whatever it chained is already saved in Pillbox's settings folder.
    let old_command = old_command.filter(|c| !is_legacy(c));
    let mut line = match old {
        Some(Value::Object(m)) => m,
        _ => serde_json::Map::new(),
    };
    line.insert("type".into(), json!("command"));
    line.insert("command".into(), json!(ours));
    root.insert("statusLine".into(), Value::Object(line));
    Ok(Plan::Replace {
        chain: old_command.filter(|c| !c.trim().is_empty()),
    })
}

fn chain_path(base: Option<PathBuf>) -> Option<PathBuf> {
    Some(base?.join("statusline-chain.json"))
}

/// `{"claude": "<old command>"}` in Pillbox's settings folder.
pub fn chain_file() -> Option<PathBuf> {
    chain_path(crate::settings_folder())
}

/// The old status line command kept for `agent`, if any.
pub fn chained(agent: &str) -> Option<String> {
    let text = std::fs::read_to_string(chain_file()?).ok()?;
    let v: Value = serde_json::from_str(&text).ok()?;
    v.get(agent)
        .and_then(Value::as_str)
        .filter(|c| !c.trim().is_empty() && !is_ours(c))
        .map(str::to_string)
}

/// Remember `command` as the status line to run behind pillboxctl for `agent`.
pub fn save_chain(agent: &str, command: &str) -> Result<PathBuf, String> {
    let path = chain_file().ok_or("APPDATA is not set")?;
    let mut doc: Value = std::fs::read_to_string(&path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}));
    if let Some(m) = doc.as_object_mut() {
        m.insert(agent.into(), json!(command));
    }
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let text = serde_json::to_string_pretty(&doc).map_err(|e| e.to_string())?;
    std::fs::write(&path, text + "\n").map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(path)
}

/// A status line command that was started: wait for what it prints.
pub struct Running(mpsc::Receiver<Option<String>>);

impl Running {
    /// Its output, or None if it failed, printed nothing or took longer than `limit` (then it is
    /// left to finish on its own).
    pub fn output(self, limit: Duration) -> Option<String> {
        self.0.recv_timeout(limit).ok().flatten()
    }
}

/// Start `command` through `cmd.exe` with `stdin` as its input, without a console window.
pub fn start(command: &str, stdin: &str) -> Option<Running> {
    use std::os::windows::process::CommandExt;
    /// `CREATE_NO_WINDOW`: pillboxctl may run without a console; cmd must not open one.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    // `/S /C "<command>"`: cmd takes the outer quotes off and runs the rest as typed, so the
    // command's own quotes survive (the same as Claude Code typing it into a shell).
    let mut child = Command::new("cmd")
        .raw_arg(format!("/D /S /C \"{command}\""))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .ok()?;
    if let Some(mut input) = child.stdin.take() {
        let _ = input.write_all(stdin.as_bytes());
        // Dropping it closes the pipe, so the command sees the end of its input.
    }
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let out = child.wait_with_output().ok().and_then(|o| {
            let text = String::from_utf8_lossy(&o.stdout).trim_end().to_string();
            (!text.is_empty()).then_some(text)
        });
        let _ = tx.send(out);
    });
    Some(Running(rx))
}

#[cfg(test)]
mod tests {
    use super::*;

    const OURS: &str = "\"C:\\Pillbox\\pillboxctl.exe\" statusline claude";

    #[test]
    fn the_old_islandctl_status_line_is_replaced_without_chaining_it() {
        let mut doc = json!({
            "statusLine": {"type": "command", "command": "\"C:\\old\\islandctl.exe\" statusline claude"}
        });
        let plan = apply(&mut doc, OURS).expect("plan");
        assert_eq!(plan, Plan::Replace { chain: None });
        assert_eq!(doc["statusLine"]["command"], json!(OURS));
    }

    #[test]
    fn replaces_an_old_status_line_and_keeps_its_options() {
        let mut doc = json!({
            "model": "opus",
            "statusLine": {"type": "command", "command": "node \"C:/x/bridge.js\"", "refreshInterval": 1}
        });
        let plan = apply(&mut doc, OURS).expect("plan");
        assert_eq!(
            plan,
            Plan::Replace {
                chain: Some("node \"C:/x/bridge.js\"".into())
            }
        );
        assert_eq!(doc["statusLine"]["command"], json!(OURS));
        assert_eq!(doc["statusLine"]["refreshInterval"], json!(1));
        assert_eq!(doc["model"], json!("opus"));
    }

    #[test]
    fn adds_one_when_there_is_none() {
        let mut doc = json!({});
        assert_eq!(apply(&mut doc, OURS), Ok(Plan::Replace { chain: None }));
        assert_eq!(doc["statusLine"]["type"], json!("command"));
    }

    #[test]
    fn leaves_its_own_alone() {
        let mut doc = json!({"statusLine": {"type": "command", "command": OURS}});
        let before = doc.clone();
        assert_eq!(apply(&mut doc, OURS), Ok(Plan::AlreadyOurs));
        assert_eq!(doc, before);
        assert!(is_ours("\"D:\\a\\PillboxCtl.exe\" statusline codex"));
        assert!(!is_ours("node bridge.js"));
        assert!(apply(&mut json!([1]), OURS).is_err());
    }

    #[test]
    fn chain_file_sits_in_the_settings_folder() {
        assert_eq!(
            chain_path(Some(PathBuf::from("C:\\a"))),
            Some(PathBuf::from("C:\\a\\statusline-chain.json"))
        );
        assert_eq!(chain_path(None), None);
    }
}
