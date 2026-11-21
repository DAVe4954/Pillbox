//! Printing and merging the hook entries that connect Claude Code and Codex to Pillbox.
//! Pure JSON handling, so it is unit-tested.

use serde_json::{Map, Value, json};

/// Events Pillbox listens to. `PostToolUse` is left out on purpose: it would double the
/// number of hook runs for nothing Pillbox shows.
pub const EVENTS: [&str; 7] = [
    "SessionStart",
    "UserPromptSubmit",
    "PreToolUse",
    "PermissionRequest",
    "Notification",
    "Stop",
    "SessionEnd",
];

/// How long a Claude Code reply hook may wait for a click in Pillbox, seconds. Pillbox's
/// own `script.answer_seconds` (60 by default) ends the wait first.
pub const REPLY_TIMEOUT: u64 = 90;

/// The `"hooks"` object for one agent. `exe` is the full path of pillboxctl.
pub fn snippet(agent: &str, exe: &str, reply: bool) -> Value {
    let mut hooks = Map::new();
    for event in EVENTS {
        let ask = reply && event == "PermissionRequest";
        let command = format!(
            "\"{exe}\" hook {agent}{}",
            if ask { " --reply" } else { "" }
        );
        let mut entry = json!({ "type": "command", "command": command });
        if ask {
            entry["timeout"] = json!(REPLY_TIMEOUT);
        }
        hooks.insert(event.to_string(), json!([{ "hooks": [entry] }]));
    }
    Value::Object(hooks)
}

/// True if a hook group list already has a pillboxctl entry.
fn has_ours(groups: &Value) -> bool {
    groups
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|g| g.get("hooks")?.as_array())
        .flatten()
        .filter_map(|h| h.get("command")?.as_str())
        .any(|c| c.to_lowercase().contains("pillboxctl"))
}

/// A command of the tool this one was called before (`islandctl`, when the app was named
/// Island): its hooks point at an executable that is gone, so they are replaced.
fn is_legacy(command: &str) -> bool {
    command.to_lowercase().contains("islandctl")
}

/// Remove the legacy entries from one event's group list, and groups left empty by it.
fn drop_legacy(groups: &mut Value) {
    let Some(list) = groups.as_array_mut() else {
        return;
    };
    for group in list.iter_mut() {
        if let Some(hooks) = group.get_mut("hooks").and_then(Value::as_array_mut) {
            hooks.retain(|h| {
                !h.get("command")
                    .and_then(Value::as_str)
                    .is_some_and(is_legacy)
            });
        }
    }
    list.retain(|g| {
        g.get("hooks")
            .and_then(Value::as_array)
            .is_none_or(|h| !h.is_empty())
    });
}

/// Add our hook entries to the `"hooks"` object of a settings document, leaving everything
/// else (and any hooks already there) untouched. Returns the events that were added; an event
/// that already has a pillboxctl entry is skipped, so running it twice changes nothing.
pub fn merge(doc: &mut Value, ours: &Value) -> Vec<String> {
    let mut added = Vec::new();
    let Some(root) = doc.as_object_mut() else {
        return added;
    };
    let hooks = root.entry("hooks").or_insert_with(|| json!({}));
    let Some(hooks) = hooks.as_object_mut() else {
        return added;
    };
    for (event, groups) in ours.as_object().into_iter().flatten() {
        let slot = hooks.entry(event.clone()).or_insert_with(|| json!([]));
        drop_legacy(slot);
        if has_ours(slot) {
            continue;
        }
        if let (Some(list), Some(new)) = (slot.as_array_mut(), groups.as_array()) {
            list.extend(new.iter().cloned());
            added.push(event.clone());
        }
    }
    added
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXE: &str = "C:\\Program Files\\Pillbox\\pillboxctl.exe";

    #[test]
    fn snippet_covers_every_event_and_quotes_the_path() {
        let s = snippet("claude", EXE, false);
        for e in EVENTS {
            assert_eq!(
                s[e][0]["hooks"][0]["command"],
                json!(format!("\"{EXE}\" hook claude")),
                "{e}"
            );
        }
        assert!(
            s["PermissionRequest"][0]["hooks"][0]
                .get("timeout")
                .is_none()
        );
    }

    #[test]
    fn the_reply_hook_waits_only_on_permission_requests() {
        let s = snippet("claude", EXE, true);
        let p = &s["PermissionRequest"][0]["hooks"][0];
        assert!(
            p["command"]
                .as_str()
                .is_some_and(|c| c.ends_with("hook claude --reply"))
        );
        assert_eq!(p["timeout"], json!(REPLY_TIMEOUT));
        let other = &s["Stop"][0]["hooks"][0];
        assert!(
            other["command"]
                .as_str()
                .is_some_and(|c| !c.contains("--reply"))
        );
        assert!(other.get("timeout").is_none());
    }

    #[test]
    fn merge_keeps_what_is_there_and_is_idempotent() {
        let mut doc = json!({
            "model": "sonnet",
            "hooks": {
                "Stop": [{ "hooks": [{ "type": "command", "command": "notify.ps1" }] }]
            }
        });
        let ours = snippet("claude", EXE, false);
        let added = merge(&mut doc, &ours);
        assert_eq!(added.len(), EVENTS.len());
        assert_eq!(doc["model"], json!("sonnet"));
        let stop = doc["hooks"]["Stop"].as_array().map(Vec::len);
        assert_eq!(
            stop,
            Some(2),
            "the user's own hook stays, ours is added next to it"
        );
        assert_eq!(
            doc["hooks"]["Stop"][0]["hooks"][0]["command"],
            json!("notify.ps1")
        );
        assert!(merge(&mut doc, &ours).is_empty(), "second run adds nothing");
        assert_eq!(doc["hooks"]["Stop"].as_array().map(Vec::len), Some(2));
    }

    #[test]
    fn hooks_of_the_old_islandctl_are_replaced_not_duplicated() {
        let mut doc = json!({ "hooks": {
            "Stop": [
                { "hooks": [{ "type": "command", "command": "\"C:\\old\\islandctl.exe\" hook claude" }] },
                { "hooks": [{ "type": "command", "command": "echo mine" }] }
            ]
        } });
        let added = merge(&mut doc, &snippet("claude", EXE, false));
        assert!(added.contains(&"Stop".to_string()));
        let stop = doc["hooks"]["Stop"].as_array().expect("Stop list");
        let commands: Vec<&str> = stop
            .iter()
            .filter_map(|g| g["hooks"][0]["command"].as_str())
            .collect();
        assert_eq!(commands.len(), 2, "the old entry is gone: {commands:?}");
        assert!(commands.contains(&"echo mine"), "the user's own hook stays");
        assert!(commands.iter().any(|c| c.contains("pillboxctl")));
        assert!(!commands.iter().any(|c| c.contains("islandctl")));
    }

    #[test]
    fn merge_creates_a_missing_hooks_object() {
        let mut doc = json!({});
        merge(&mut doc, &snippet("codex", EXE, false));
        assert!(doc["hooks"]["UserPromptSubmit"].is_array());
        // A document that is not an object is left alone.
        let mut weird = json!([1, 2]);
        assert!(merge(&mut weird, &snippet("codex", EXE, false)).is_empty());
        let mut odd = json!({ "hooks": 5 });
        assert!(merge(&mut odd, &snippet("codex", EXE, false)).is_empty());
    }
}
