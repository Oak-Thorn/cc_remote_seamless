use serde_json::{json, Value};
use std::path::{Path, PathBuf};

const OWNER_TAG: &str = "cc-remote-seamless";

/// Install (or refresh) the cc-remote hooks in `~/.claude/settings.json`.
///
/// Idempotent: each call removes any prior entries owned by us (matched by the
/// `_owner` sentinel field, regardless of port) and re-inserts a fresh set
/// pointing at `port`. Hook entries from other tools are preserved.
pub fn install_claude_hooks(port: u16) {
    let path = match settings_path() {
        Some(p) => p,
        None => {
            tracing::warn!("install_claude_hooks: cannot resolve home dir");
            return;
        }
    };
    if let Err(e) = install_at(&path, port) {
        tracing::warn!("install_claude_hooks: {}", e);
    }
}

fn install_at(path: &Path, port: u16) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("mkdir {:?} failed: {}", parent, e))?;
    }

    let existing_text = std::fs::read_to_string(path).unwrap_or_default();
    let root: Value = if existing_text.trim().is_empty() {
        json!({})
    } else {
        serde_json::from_str(&existing_text)
            .map_err(|e| format!("parse {:?} failed: {}", path, e))?
    };

    let updated = apply(root, port)?;
    let serialized = serde_json::to_string_pretty(&updated)
        .map_err(|e| format!("serialize failed: {}", e))?;

    if existing_text.trim() == serialized.trim() {
        tracing::info!("Claude Code hooks already up to date at {:?}", path);
        return Ok(());
    }

    std::fs::write(path, &serialized)
        .map_err(|e| format!("write {:?} failed: {}", path, e))?;
    tracing::info!("Claude Code hooks installed at {:?} (port={})", path, port);
    Ok(())
}

fn apply(mut root: Value, port: u16) -> Result<Value, String> {
    if !root.is_object() {
        return Err("settings root is not an object".into());
    }
    let base_url = format!("http://127.0.0.1:{}", port);

    let hooks_obj = root
        .as_object_mut()
        .unwrap()
        .entry("hooks")
        .or_insert_with(|| json!({}));
    if !hooks_obj.is_object() {
        *hooks_obj = json!({});
    }
    let hooks_map = hooks_obj.as_object_mut().unwrap();

    for (event, entry) in desired_hooks(&base_url) {
        let arr_val = hooks_map
            .entry(event.to_string())
            .or_insert_with(|| Value::Array(vec![]));
        if !arr_val.is_array() {
            *arr_val = Value::Array(vec![]);
        }
        let arr = arr_val.as_array_mut().unwrap();
        arr.retain(|item| {
            // Remove entries owned by us
            if item.get("_owner").and_then(|v| v.as_str()) == Some(OWNER_TAG) {
                return false;
            }
            // Remove orphan entries (no _owner) that target our server
            if item.get("_owner").is_none() {
                if let Some(hooks_arr) = item.get("hooks").and_then(|v| v.as_array()) {
                    for h in hooks_arr {
                        if let Some(url) = h.get("url").and_then(|v| v.as_str()) {
                            if url.contains(&base_url) {
                                return false;
                            }
                        }
                        if let Some(cmd) = h.get("command").and_then(|v| v.as_str()) {
                            if cmd.contains(&base_url) {
                                return false;
                            }
                        }
                    }
                }
            }
            true
        });
        arr.push(entry);
    }
    Ok(root)
}

fn settings_path() -> Option<PathBuf> {
    directories::BaseDirs::new()
        .map(|d| d.home_dir().join(".claude").join("settings.json"))
}

fn desired_hooks(base_url: &str) -> Vec<(&'static str, Value)> {
    let entry = |path: &str, timeout: Option<u64>| {
        let url = format!("{}{}", base_url, path);
        let cmd = format!(
            "curl -sS -X POST -H 'Content-Type: application/json' -d @- {}",
            url
        );
        let mut hook = json!({
            "type": "command",
            "command": cmd,
        });
        if let Some(t) = timeout {
            hook.as_object_mut().unwrap().insert("timeout".into(), json!(t));
        }
        json!({
            "matcher": "",
            "hooks": [hook],
            "_owner": OWNER_TAG,
        })
    };

    vec![
        ("SessionStart", entry("/hook/session-start", None)),
        ("SessionEnd", entry("/hook/session-end", None)),
        ("Stop", entry("/hook/stop", None)),
        ("StopFailure", entry("/hook/stop_failure", None)),
        ("UserPromptSubmit", entry("/hook/prompt", None)),
        ("PreToolUse", entry("/hook/pre-tool", None)),
        ("PostToolUse", entry("/hook/post-tool", None)),
        ("PostToolUseFailure", entry("/hook/post-tool-failure", None)),
        ("SubagentStart", entry("/hook/subagent_start", None)),
        ("SubagentStop", entry("/hook/subagent_stop", None)),
        ("Notification", entry("/hook/notification", None)),
        ("Elicitation", entry("/hook/elicitation", None)),
        ("PreCompact", entry("/hook/pre_compact", None)),
        ("PostCompact", entry("/hook/post_compact", None)),
        ("PermissionRequest", entry("/permission", Some(600))),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmd_of(entry: &Value) -> &str {
        entry.pointer("/hooks/0/command").unwrap().as_str().unwrap()
    }

    #[test]
    fn fresh_install_adds_all_events() {
        let out = apply(json!({}), 23399).unwrap();
        let hooks = out.get("hooks").unwrap().as_object().unwrap();
        let expected = [
            "SessionStart", "SessionEnd", "Stop", "StopFailure",
            "UserPromptSubmit", "PreToolUse", "PostToolUse",
            "PostToolUseFailure", "SubagentStart", "SubagentStop",
            "Notification", "Elicitation", "PreCompact", "PostCompact",
            "PermissionRequest",
        ];
        assert_eq!(hooks.len(), expected.len());
        for k in expected {
            assert_eq!(hooks.get(k).unwrap().as_array().unwrap().len(), 1, "{}", k);
        }
    }

    #[test]
    fn hooks_use_command_type_with_stdin() {
        let out = apply(json!({}), 23399).unwrap();
        let hooks = out.get("hooks").unwrap().as_object().unwrap();
        for (k, arr) in hooks.iter() {
            let entries = arr.as_array().unwrap();
            for entry in entries {
                if entry.get("_owner").and_then(|v| v.as_str()) == Some(OWNER_TAG) {
                    let inner = entry.pointer("/hooks/0").unwrap();
                    assert_eq!(inner.get("type").unwrap().as_str().unwrap(), "command", "event {} should use command type", k);
                    let cmd = inner.get("command").unwrap().as_str().unwrap();
                    assert!(cmd.contains("curl"), "event {} command should use curl", k);
                    assert!(cmd.contains("-d @-"), "event {} should read payload from stdin", k);
                    assert!(cmd.contains("127.0.0.1:23399"), "event {} should target correct port", k);
                }
            }
        }
    }

    #[test]
    fn removes_old_http_type_hooks_from_same_owner() {
        let initial = json!({
            "hooks": {
                "SessionStart": [
                    {
                        "matcher": "",
                        "hooks": [{"type": "http", "url": "http://127.0.0.1:23399/hook/session-start"}],
                        "_owner": OWNER_TAG
                    }
                ]
            }
        });
        let out = apply(initial, 23399).unwrap();
        let arr = out.pointer("/hooks/SessionStart").unwrap().as_array().unwrap();
        assert_eq!(arr.len(), 1);
        let inner = arr[0].pointer("/hooks/0").unwrap();
        assert_eq!(inner.get("type").unwrap().as_str().unwrap(), "command");
    }

    #[test]
    fn rerun_does_not_duplicate() {
        let v = apply(json!({}), 23399).unwrap();
        let v = apply(v, 23399).unwrap();
        let arr = v.pointer("/hooks/PreToolUse").unwrap().as_array().unwrap();
        assert_eq!(arr.len(), 1);
    }

    #[test]
    fn port_change_replaces_old_entries() {
        let v = apply(json!({}), 23399).unwrap();
        let v = apply(v, 23400).unwrap();
        let arr = v.pointer("/hooks/SessionStart").unwrap().as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert!(cmd_of(&arr[0]).contains(":23400"));
    }

    #[test]
    fn preserves_third_party_hooks() {
        let initial = json!({
            "hooks": {
                "PreToolUse": [
                    { "matcher": "", "hooks": [{ "type": "command", "command": "echo other" }] }
                ]
            }
        });
        let v = apply(initial, 23399).unwrap();
        let arr = v.pointer("/hooks/PreToolUse").unwrap().as_array().unwrap();
        assert_eq!(arr.len(), 2);
        let cmds: Vec<&str> = arr.iter().map(|e| e.pointer("/hooks/0/command").unwrap().as_str().unwrap()).collect();
        assert!(cmds.contains(&"echo other"));
    }

    #[test]
    fn preserves_top_level_unrelated_keys() {
        let initial = json!({ "theme": "dark", "model": "claude-opus-4-7" });
        let v = apply(initial, 23399).unwrap();
        assert_eq!(v.get("theme").unwrap(), "dark");
        assert_eq!(v.get("model").unwrap(), "claude-opus-4-7");
        assert!(v.pointer("/hooks/SessionStart").is_some());
    }

    #[test]
    fn removes_orphan_http_hooks_targeting_our_server() {
        let initial = json!({
            "hooks": {
                "Stop": [
                    {
                        "matcher": "",
                        "hooks": [{"type": "http", "url": "http://127.0.0.1:23399/hook/stop"}]
                    }
                ],
                "UserPromptSubmit": [
                    {
                        "matcher": "",
                        "hooks": [{"type": "http", "url": "http://127.0.0.1:23399/hook/prompt"}]
                    },
                    {
                        "matcher": "",
                        "hooks": [{"type": "command", "command": "echo third-party"}]
                    }
                ]
            }
        });
        let v = apply(initial, 23399).unwrap();
        // Orphan http entries should be removed, third-party preserved
        let stop_arr = v.pointer("/hooks/Stop").unwrap().as_array().unwrap();
        assert_eq!(stop_arr.len(), 1);
        assert!(cmd_of(&stop_arr[0]).contains("curl"));

        let prompt_arr = v.pointer("/hooks/UserPromptSubmit").unwrap().as_array().unwrap();
        assert_eq!(prompt_arr.len(), 2); // third-party + ours
        let cmds: Vec<&str> = prompt_arr.iter()
            .filter_map(|e| e.pointer("/hooks/0/command").and_then(|v| v.as_str()))
            .collect();
        assert!(cmds.iter().any(|c| c.contains("echo third-party")));
        assert!(cmds.iter().any(|c| c.contains("curl")));
    }
}
