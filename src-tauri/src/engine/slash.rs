use crate::agent::AgentConnector;
use crate::hook::server::{PermissionResponse, PermissionWaiters};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::info;

use super::router::BindingStore;
use super::store::MessageStore;

pub enum SlashResult {
    Reply(String),
    /// Inject `text` into the bound session. `receipt` is sent to the user
    /// after a successful injection (window label + any active-drift warning).
    Inject { text: String, receipt: String },
    BindingChanged { reply: String, session_id: String },
    Noop,
}

pub async fn execute(
    text: &str,
    chat_id: &str,
    bindings: &Arc<BindingStore>,
    agents: &HashMap<String, Arc<dyn AgentConnector>>,
    permission_waiters: &PermissionWaiters,
    _messages: &Arc<MessageStore>,
) -> SlashResult {
    let text = text.trim();
    let (cmd, args) = match text.find(' ') {
        Some(i) => (&text[..i], text[i+1..].trim()),
        None => (text, ""),
    };
    let cmd_lower = cmd.to_lowercase();

    match cmd_lower.as_str() {
        "/help" => SlashResult::Reply(HELP_TEXT.to_string()),
        "/status" => status(chat_id, bindings, agents).await,
        "/list" => list(agents).await,
        "/switch" => switch(args, chat_id, bindings, agents).await,
        "/pin" => pin(args, chat_id, bindings, agents).await,
        "/unpin" => unpin(chat_id, bindings, agents).await,
        "/mute" => { bindings.set_muted(chat_id, true); SlashResult::Reply("Muted".to_string()) }
        "/unmute" => { bindings.set_muted(chat_id, false); SlashResult::Reply("Unmuted".to_string()) }
        "/full" => full(chat_id, bindings),
        "/p" => {
            if args.is_empty() { return SlashResult::Reply("Usage: /p <prompt>".to_string()); }
            let binding = match bindings.get(chat_id) {
                Some(b) => b,
                None => return SlashResult::Reply("No session bound. Use /switch <id> or /radar first.".to_string()),
            };
            // Resolve the target session's working dir for a readable label,
            // and reject if it is currently busy.
            let mut working_dir = None;
            if let Some(agent) = agents.get(&binding.agent_id) {
                for s in agent.discover_sessions().await {
                    if s.id == binding.session_id {
                        if s.state == crate::agent::SessionState::Busy {
                            return SlashResult::Reply("Session is busy, please wait until idle".to_string());
                        }
                        working_dir = s.working_dir.clone();
                    }
                }
            }
            let receipt = build_send_receipt(chat_id, &binding.session_id, working_dir.as_deref(), bindings);
            bindings.set_last_routed(chat_id, &binding.session_id);
            SlashResult::Inject { text: args.to_string(), receipt }
        }
        "/t" => { info!("Test message from Feishu: {}", args); SlashResult::Reply("ok".to_string()) }
        "/allow" => resolve_permission(chat_id, bindings, agents, permission_waiters, "allow").await,
        "/deny" => resolve_permission(chat_id, bindings, agents, permission_waiters, "deny").await,
        "/always" => resolve_permission(chat_id, bindings, agents, permission_waiters, "allowAlways").await,
        "/answer" => answer_question(args, chat_id, bindings, permission_waiters).await,
        "/change" => change_agent(args, chat_id, bindings, agents).await,
        "/clear" => clear_input(chat_id, bindings, agents).await,
        "/enter" => enter(chat_id, bindings, agents).await,
        "/stop" => stop_session(chat_id, bindings, agents).await,
        "/skill" => search_skills(args),
        "/radar" => radar(agents).await,
        _ => SlashResult::Reply(format!("Unknown command: {}", cmd)),
    }
}

async fn status(chat_id: &str, bindings: &Arc<BindingStore>, agents: &HashMap<String, Arc<dyn AgentConnector>>) -> SlashResult {
    let binding = match bindings.get(chat_id) {
        Some(b) => b,
        None => return SlashResult::Reply("No session bound".to_string()),
    };
    for agent in agents.values() {
        for s in agent.discover_sessions().await {
            if s.id == binding.session_id {
                let muted = if bindings.is_muted(chat_id) { " [muted]" } else { "" };
                let pinned = if bindings.is_pinned(chat_id) { " [pinned]" } else { "" };
                return SlashResult::Reply(format!(
                    "Session: {}\nAgent: {}\nState: {:?}\nDir: {}{}{}",
                    s.id, s.agent, s.state, s.working_dir.unwrap_or_default(), muted, pinned
                ));
            }
        }
    }
    SlashResult::Reply(format!("Session {} (not active)", binding.session_id))
}

async fn list(agents: &HashMap<String, Arc<dyn AgentConnector>>) -> SlashResult {
    let mut lines = vec![];
    for agent in agents.values() {
        for s in agent.discover_sessions().await {
            let state = format!("{:?}", s.state).to_lowercase();
            lines.push(format!("==》  <<{}>>  {}  ({})", state, s.id, s.working_dir.unwrap_or_default()));
        }
    }
    if lines.is_empty() {
        SlashResult::Reply("No active sessions".to_string())
    } else {
        SlashResult::Reply(lines.join("\n"))
    }
}

async fn switch(args: &str, chat_id: &str, bindings: &Arc<BindingStore>, agents: &HashMap<String, Arc<dyn AgentConnector>>) -> SlashResult {
    if args.is_empty() {
        return SlashResult::Reply("Usage: /switch <session_id>".to_string());
    }
    for agent in agents.values() {
        for s in agent.discover_sessions().await {
            if s.id.starts_with(args) || s.id == args {
                bindings.bind(chat_id, &s.agent, &s.id);
                // Reset the drift baseline: an explicit switch is the new
                // "expected" window, so the next /p shouldn't warn about it.
                bindings.set_last_routed(chat_id, &s.id);
                let label = super::label::window_label(&s.id, s.working_dir.as_deref());
                return SlashResult::Reply(format!("Switched to {}\nDir: {}", label, s.working_dir.unwrap_or_default()));
            }
        }
    }
    SlashResult::Reply(format!("Session not found: {}", args))
}

/// `/pin` pins the chat's current binding so hook events stop drifting active
/// away from it. `/pin <id>` first switches to the matching session, then pins.
async fn pin(args: &str, chat_id: &str, bindings: &Arc<BindingStore>, agents: &HashMap<String, Arc<dyn AgentConnector>>) -> SlashResult {
    // With an argument, resolve + bind to the target first (reusing switch).
    if !args.is_empty() {
        for agent in agents.values() {
            for s in agent.discover_sessions().await {
                if s.id.starts_with(args) || s.id == args {
                    bindings.bind(chat_id, &s.agent, &s.id);
                    bindings.set_last_routed(chat_id, &s.id);
                    bindings.set_pinned(chat_id, true);
                    let label = super::label::window_label(&s.id, s.working_dir.as_deref());
                    return SlashResult::Reply(format!("📌 已固定 {}\nhook 事件不再漂移 active，用 /unpin 解除", label));
                }
            }
        }
        return SlashResult::Reply(format!("Session not found: {}", args));
    }

    // No argument: pin whatever the chat is currently bound to.
    let binding = match bindings.get(chat_id) {
        Some(b) => b,
        None => return SlashResult::Reply("No session bound. Use /switch <id> first.".to_string()),
    };
    let working_dir = session_working_dir(&binding.agent_id, &binding.session_id, agents).await;
    bindings.set_pinned(chat_id, true);
    let label = super::label::window_label(&binding.session_id, working_dir.as_deref());
    SlashResult::Reply(format!("📌 已固定 {}\nhook 事件不再漂移 active，用 /unpin 解除", label))
}

/// `/unpin` releases the pin, letting hook-driven active drift resume.
async fn unpin(chat_id: &str, bindings: &Arc<BindingStore>, agents: &HashMap<String, Arc<dyn AgentConnector>>) -> SlashResult {
    let binding = match bindings.get(chat_id) {
        Some(b) => b,
        None => return SlashResult::Reply("No session bound".to_string()),
    };
    if !bindings.is_pinned(chat_id) {
        return SlashResult::Reply("当前未固定".to_string());
    }
    bindings.set_pinned(chat_id, false);
    let working_dir = session_working_dir(&binding.agent_id, &binding.session_id, agents).await;
    let label = super::label::window_label(&binding.session_id, working_dir.as_deref());
    SlashResult::Reply(format!("已解除固定 {}\nactive 将随 hook 事件自动漂移", label))
}

/// Look up a session's working dir for labeling. Returns None when the session
/// is no longer discoverable.
async fn session_working_dir(agent_id: &str, session_id: &str, agents: &HashMap<String, Arc<dyn AgentConnector>>) -> Option<String> {
    let agent = agents.get(agent_id)?;
    agent.discover_sessions().await.into_iter()
        .find(|s| s.id == session_id)
        .and_then(|s| s.working_dir)
}

/// Build the receipt shown after a successful `/p`. Confirms which window the
/// prompt is going to, and — when the active session has drifted away from the
/// one this chat last routed to — warns the user before the prompt lands on an
/// unexpected window.
fn build_send_receipt(
    chat_id: &str,
    target_session: &str,
    target_dir: Option<&str>,
    bindings: &Arc<BindingStore>,
) -> String {
    let target_label = super::label::window_label(target_session, target_dir);
    match bindings.get_last_routed(chat_id) {
        Some(prev) if prev != target_session => {
            let prev_label = super::label::window_label(&prev, None);
            format!(
                "⚠️ active 已从 [{}] 漂移到 [{}]\n本次将发往 {}。如需回到原窗口请 /switch {}",
                prev_label,
                target_label,
                target_label,
                &prev.chars().take(6).collect::<String>(),
            )
        }
        _ => format!("→ {} 已发送", target_label),
    }
}

fn full(chat_id: &str, bindings: &Arc<BindingStore>) -> SlashResult {
    match bindings.get_last_output(chat_id) {
        Some(text) => SlashResult::Reply(text),
        None => SlashResult::Reply("No output yet".to_string()),
    }
}

async fn resolve_permission(
    chat_id: &str,
    bindings: &Arc<BindingStore>,
    agents: &HashMap<String, Arc<dyn AgentConnector>>,
    waiters: &PermissionWaiters,
    action: &str,
) -> SlashResult {
    let binding = match bindings.get(chat_id) {
        Some(b) => b,
        None => return SlashResult::Reply("No session bound".to_string()),
    };
    let session_prefix = format!("{}:", binding.session_id);
    let mut map = waiters.lock().await;
    let key = map.keys().find(|k| k.starts_with(&session_prefix)).cloned();
    if let Some(request_id) = key {
        if let Some(entry) = map.remove(&request_id) {
            // "allowAlways" only carries weight when Claude Code supplied
            // permission_suggestions; without them an empty updatedPermissions
            // would silently degrade to a one-shot allow, so report that.
            let (response, reply) = match action {
                "deny" => (PermissionResponse::deny("Denied by user"), "Permission: deny".to_string()),
                "allowAlways" if !entry.suggestions.is_empty() => (
                    PermissionResponse::allow_with(None, Some(entry.suggestions)),
                    "Permission: allow always".to_string(),
                ),
                "allowAlways" => (
                    PermissionResponse::allow(),
                    "Permission: allow (no always-rule available, allowed once)".to_string(),
                ),
                _ => (PermissionResponse::allow(), "Permission: allow".to_string()),
            };
            let _ = entry.sender.send(response);
            return SlashResult::Reply(reply);
        }
    }
    drop(map);

    // No waiter — fall back to terminal injection (Claude Code waiting in its own UI)
    let keystroke = match action {
        "allow" => "y",
        "deny" => "n",
        "allowAlways" => "a",
        _ => "y",
    };
    if let Some(agent) = agents.get(&binding.agent_id) {
        match agent.inject_input(&binding.session_id, keystroke).await {
            Ok(_) => SlashResult::Reply(format!("Permission: {} (injected)", action)),
            Err(e) => SlashResult::Reply(format!("Inject failed: {}", e)),
        }
    } else {
        SlashResult::Reply("Agent not found".to_string())
    }
}

const HELP_TEXT: &str = "\
/status - Show current session state
/list - List all active sessions
/switch <id> - Switch to session
/pin [id] - Pin current (or <id>) session against active drift
/unpin - Release pin, resume active drift
/change [agent] - Switch agent (list if no arg)
/skill <keyword> - Search skills by name/description
/mute - Mute output forwarding
/unmute - Unmute output forwarding
/full - Show last full output
/p <text> - Send prompt
/t <text> - Test (log only)
/allow - Allow permission
/deny - Deny permission
/always - Allow always
/clear - Clear session input box
/enter - Submit current input box (send Return)
/stop - Stop current session task
/radar - Rediscover running agents
/answer 1 - Single select option 1
/answer 1 3 - Multi select options 1 and 3
/answer Q2 1 - Answer question 2 in a multi-question prompt
/answer N <text> - Other / custom answer";

async fn radar(agents: &HashMap<String, Arc<dyn AgentConnector>>) -> SlashResult {
    for agent in agents.values() {
        agent.rediscover();
    }
    let mut lines = vec![];
    for agent in agents.values() {
        for s in agent.discover_sessions().await {
            lines.push(format!("[{}] {} {:?} ({})", s.agent, s.id, s.state, s.working_dir.unwrap_or_default()));
        }
    }
    if lines.is_empty() {
        SlashResult::Reply("Radar complete. No sessions found.".to_string())
    } else {
        SlashResult::Reply(format!("Radar complete. {} session(s):\n{}", lines.len(), lines.join("\n")))
    }
}

fn search_skills(keyword: &str) -> SlashResult {
    if keyword.is_empty() {
        return SlashResult::Reply("Usage: /skill <keyword>\nSearches skill name and description".to_string());
    }
    let home = match std::env::var("HOME") {
        Ok(h) => h,
        Err(_) => return SlashResult::Reply("Cannot determine HOME".to_string()),
    };
    let claude_home = std::path::Path::new(&home).join(".claude");
    let results = search_skills_in(&claude_home, keyword);

    if results.is_empty() {
        SlashResult::Reply(format!("No skills matching \"{}\"", keyword))
    } else {
        SlashResult::Reply(format!("Found {} skill(s):\n\n{}", results.len(), results.join("\n\n")))
    }
}

fn parse_skill_frontmatter(content: &str) -> (String, String) {
    let mut name = String::new();
    let mut desc = String::new();
    if content.starts_with("---") {
        if let Some(end) = content[3..].find("---") {
            let front = &content[3..3 + end];
            for line in front.lines() {
                if let Some(v) = line.strip_prefix("name:") {
                    name = v.trim().to_string();
                } else if let Some(v) = line.strip_prefix("description:") {
                    desc = v.trim().to_string();
                }
            }
        }
    }
    (name, desc)
}

fn truncate_str(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let end: usize = s.char_indices().nth(max).map(|(i, _)| i).unwrap_or(s.len());
        format!("{}...", &s[..end])
    }
}

struct SkillEntry {
    display_name: String,
    description: String,
}

/// Collect all skills from the given claude home directory.
/// Searches both `<home>/skills/` (local) and `<home>/plugins/cache/` (plugin).
fn collect_skills(claude_home: &std::path::Path) -> Vec<SkillEntry> {
    let mut results = Vec::new();

    // 1. Local skills: <claude_home>/skills/<name>/SKILL.md
    let skills_dir = claude_home.join("skills");
    if let Ok(entries) = std::fs::read_dir(&skills_dir) {
        for entry in entries.flatten() {
            if !entry.path().is_dir() { continue; }
            let skill_file = entry.path().join("SKILL.md");
            if !skill_file.exists() { continue; }
            if let Ok(content) = std::fs::read_to_string(&skill_file) {
                let (name, desc) = parse_skill_frontmatter(&content);
                let display_name = if name.is_empty() {
                    entry.file_name().to_string_lossy().to_string()
                } else {
                    name
                };
                results.push(SkillEntry { display_name, description: desc });
            }
        }
    }

    // 2. Plugin skills: <claude_home>/plugins/cache/<registry>/<pkg>/<version>/skills/<name>/SKILL.md
    let plugins_cache = claude_home.join("plugins").join("cache");
    if let Ok(registries) = std::fs::read_dir(&plugins_cache) {
        for registry in registries.flatten() {
            if !registry.path().is_dir() { continue; }
            if let Ok(pkgs) = std::fs::read_dir(registry.path()) {
                for pkg in pkgs.flatten() {
                    if !pkg.path().is_dir() { continue; }
                    let namespace = pkg.file_name().to_string_lossy().to_string();
                    if let Ok(versions) = std::fs::read_dir(pkg.path()) {
                        for version in versions.flatten() {
                            if !version.path().is_dir() { continue; }
                            let pkg_skills_dir = version.path().join("skills");
                            if let Ok(skill_entries) = std::fs::read_dir(&pkg_skills_dir) {
                                for skill_entry in skill_entries.flatten() {
                                    if !skill_entry.path().is_dir() { continue; }
                                    let skill_file = skill_entry.path().join("SKILL.md");
                                    if !skill_file.exists() { continue; }
                                    if let Ok(content) = std::fs::read_to_string(&skill_file) {
                                        let (name, desc) = parse_skill_frontmatter(&content);
                                        let skill_name = if name.is_empty() {
                                            skill_entry.file_name().to_string_lossy().to_string()
                                        } else {
                                            name
                                        };
                                        let display_name = format!("{}:{}", namespace, skill_name);
                                        results.push(SkillEntry { display_name, description: desc });
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    results
}

/// Search skills by keyword using contains matching on name and description.
fn search_skills_in(claude_home: &std::path::Path, keyword: &str) -> Vec<String> {
    let keyword_lower = keyword.to_lowercase();
    let all_skills = collect_skills(claude_home);
    let mut results = Vec::new();
    for skill in all_skills {
        if skill.display_name.to_lowercase().contains(&keyword_lower)
            || skill.description.to_lowercase().contains(&keyword_lower)
        {
            results.push(format!("**{}**\n  {}", skill.display_name, truncate_str(&skill.description, 100)));
        }
    }
    results
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn create_skill_file(dir: &std::path::Path, frontmatter: &str) {
        fs::create_dir_all(dir).unwrap();
        fs::write(dir.join("SKILL.md"), frontmatter).unwrap();
    }

    #[test]
    fn test_search_finds_local_skill_by_partial_name() {
        let tmp = TempDir::new().unwrap();
        let claude_home = tmp.path();

        create_skill_file(
            &claude_home.join("skills").join("git-commit"),
            "---\nname: git-commit\ndescription: Execute git add and commit\n---\n",
        );

        let results = search_skills_in(claude_home, "git");
        assert_eq!(results.len(), 1);
        assert!(results[0].contains("git-commit"));
    }

    #[test]
    fn test_search_finds_plugin_skill_by_partial_name() {
        let tmp = TempDir::new().unwrap();
        let claude_home = tmp.path();

        create_skill_file(
            &claude_home.join("plugins/cache/ecc-registry/ecc/1.0.0/skills/tdd-workflow"),
            "---\nname: tdd-workflow\ndescription: Test-driven development workflow\n---\n",
        );

        let results = search_skills_in(claude_home, "tdd");
        assert_eq!(results.len(), 1);
        assert!(results[0].contains("ecc:tdd-workflow"));
    }

    #[test]
    fn test_search_finds_skill_by_partial_description() {
        let tmp = TempDir::new().unwrap();
        let claude_home = tmp.path();

        create_skill_file(
            &claude_home.join("skills").join("my-tool"),
            "---\nname: my-tool\ndescription: A tool for database migrations\n---\n",
        );

        let results = search_skills_in(claude_home, "migration");
        assert_eq!(results.len(), 1);
        assert!(results[0].contains("my-tool"));
    }

    #[test]
    fn test_search_returns_results_from_both_local_and_plugins() {
        let tmp = TempDir::new().unwrap();
        let claude_home = tmp.path();

        create_skill_file(
            &claude_home.join("skills").join("my-review"),
            "---\nname: my-review\ndescription: Personal code review helper\n---\n",
        );
        create_skill_file(
            &claude_home.join("plugins/cache/ecc-registry/ecc/1.0.0/skills/code-review"),
            "---\nname: code-review\ndescription: Expert code review specialist\n---\n",
        );

        let results = search_skills_in(claude_home, "review");
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_search_case_insensitive() {
        let tmp = TempDir::new().unwrap();
        let claude_home = tmp.path();

        create_skill_file(
            &claude_home.join("skills").join("TDD-Helper"),
            "---\nname: TDD-Helper\ndescription: Helps with TDD\n---\n",
        );

        let results = search_skills_in(claude_home, "tdd");
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_search_no_match_returns_empty() {
        let tmp = TempDir::new().unwrap();
        let claude_home = tmp.path();

        create_skill_file(
            &claude_home.join("skills").join("git-commit"),
            "---\nname: git-commit\ndescription: Execute git commit\n---\n",
        );

        let results = search_skills_in(claude_home, "kubernetes");
        assert!(results.is_empty());
    }

    fn question_input() -> serde_json::Value {
        serde_json::json!({
            "questions": [{
                "header": "方向",
                "question": "选哪个方向?",
                "multiSelect": false,
                "options": [
                    {"label": "补序列图", "description": "d1"},
                    {"label": "补测试章节", "description": "d2"},
                    {"label": "校对与纠错", "description": "d3"}
                ]
            }]
        })
    }

    fn q0() -> serde_json::Value {
        question_input()["questions"][0].clone()
    }

    #[test]
    fn answer_single_index_maps_to_label() {
        let (q, v) = answer_for_question(&q0(), "3", false).unwrap();
        assert_eq!(q, "选哪个方向?");
        assert_eq!(v, "校对与纠错");
    }

    #[test]
    fn answer_multi_index_joins_labels() {
        let (_, v) = answer_for_question(&q0(), "1 2", false).unwrap();
        assert_eq!(v, "补序列图, 补测试章节");
    }

    #[test]
    fn answer_custom_text_strips_index_marker() {
        // "/answer 4 自定义内容" → is_custom, value is just the free text
        let (_, v) = answer_for_question(&q0(), "4 自定义内容", true).unwrap();
        assert_eq!(v, "自定义内容");
    }

    #[test]
    fn answer_out_of_range_index_falls_back_to_text() {
        let (_, v) = answer_for_question(&q0(), "9", false).unwrap();
        assert_eq!(v, "9");
    }

    #[test]
    fn answer_non_question_returns_none() {
        let input = serde_json::json!({ "command": "ls" });
        assert!(answer_for_question(&input, "1", false).is_none());
    }

    #[test]
    fn parse_target_detects_qn_prefix() {
        assert_eq!(parse_question_target("Q2 1 3"), (Some(1), "1 3"));
        assert_eq!(parse_question_target("q4 2"), (Some(3), "2"));
    }

    #[test]
    fn parse_target_absent_returns_none() {
        assert_eq!(parse_question_target("1 3"), (None, "1 3"));
        assert_eq!(parse_question_target("3 自定义"), (None, "3 自定义"));
    }

    #[test]
    fn parse_target_q0_is_not_a_valid_target() {
        // Q0 has no question 0 (1-based), so it's treated as plain text.
        assert_eq!(parse_question_target("Q0 1"), (None, "Q0 1"));
    }
}

async fn clear_input(chat_id: &str, bindings: &Arc<BindingStore>, agents: &HashMap<String, Arc<dyn AgentConnector>>) -> SlashResult {
    let binding = match bindings.get(chat_id) {
        Some(b) => b,
        None => return SlashResult::Reply("No session bound".to_string()),
    };
    if let Some(agent) = agents.get(&binding.agent_id) {
        // Ctrl+C discards current input in Claude Code
        match agent.inject_input(&binding.session_id, "\x03").await {
            Ok(_) => SlashResult::Reply("Input cleared".to_string()),
            Err(e) => SlashResult::Reply(format!("Clear failed: {}", e)),
        }
    } else {
        SlashResult::Reply("Agent not found".to_string())
    }
}

/// `/enter` sends a bare Return keystroke to the bound session, submitting
/// whatever is already in its input box. Useful when the input was populated
/// out-of-band and only needs a final submit.
async fn enter(chat_id: &str, bindings: &Arc<BindingStore>, agents: &HashMap<String, Arc<dyn AgentConnector>>) -> SlashResult {
    let binding = match bindings.get(chat_id) {
        Some(b) => b,
        None => return SlashResult::Reply("No session bound".to_string()),
    };
    if let Some(agent) = agents.get(&binding.agent_id) {
        match agent.inject_input(&binding.session_id, "\r").await {
            Ok(_) => SlashResult::Reply("Enter sent".to_string()),
            Err(e) => SlashResult::Reply(format!("Enter failed: {}", e)),
        }
    } else {
        SlashResult::Reply("Agent not found".to_string())
    }
}

async fn stop_session(chat_id: &str, bindings: &Arc<BindingStore>, agents: &HashMap<String, Arc<dyn AgentConnector>>) -> SlashResult {
    let binding = match bindings.get(chat_id) {
        Some(b) => b,
        None => return SlashResult::Reply("No session bound".to_string()),
    };
    if let Some(agent) = agents.get(&binding.agent_id) {
        // Escape key to interrupt Claude Code
        match agent.inject_input(&binding.session_id, "\x1b").await {
            Ok(_) => SlashResult::Reply("Stop signal sent".to_string()),
            Err(e) => SlashResult::Reply(format!("Stop failed: {}", e)),
        }
    } else {
        SlashResult::Reply("Agent not found".to_string())
    }
}

async fn change_agent(args: &str, chat_id: &str, bindings: &Arc<BindingStore>, agents: &HashMap<String, Arc<dyn AgentConnector>>) -> SlashResult {
    if args.is_empty() {
        let current = bindings.get(chat_id).map(|b| b.agent_id.clone()).unwrap_or_default();
        let mut lines = vec![format!("Current agent: {}", if current.is_empty() { "none" } else { &current })];
        lines.push("Available agents:".to_string());
        for id in agents.keys() {
            let marker = if *id == current { " *" } else { "" };
            lines.push(format!("  - {}{}", id, marker));
        }
        return SlashResult::Reply(lines.join("\n"));
    }
    let target = args.trim();
    if !agents.contains_key(target) {
        return SlashResult::Reply(format!("Agent not found: {}\nAvailable: {}", target, agents.keys().cloned().collect::<Vec<_>>().join(", ")));
    }
    // Rebind to first session of the new agent
    let agent = &agents[target];
    let sessions = agent.discover_sessions().await;
    if let Some(first) = sessions.first() {
        bindings.bind(chat_id, target, &first.id);
        let reply = format!("Switched to agent: {}\nSession: {} ({})", target, first.id, first.working_dir.clone().unwrap_or_default());
        SlashResult::BindingChanged { reply, session_id: first.id.clone() }
    } else {
        bindings.bind(chat_id, target, "");
        SlashResult::Reply(format!("Switched to agent: {} (no active sessions)", target))
    }
}

/// `/answer 1` / `/answer 1 3` / `/answer 3 文本` answers the first question.
/// `/answer Q2 1` / `/answer Q4 1 2` answers a specific question in a
/// multi-question prompt; answers accumulate until every question is filled,
/// then the response is sent to Claude Code.
async fn answer_question(
    args: &str,
    chat_id: &str,
    bindings: &Arc<BindingStore>,
    waiters: &PermissionWaiters,
) -> SlashResult {
    if args.is_empty() {
        return SlashResult::Reply(ANSWER_USAGE.to_string());
    }

    // Detect a leading QN target (case-insensitive): "/answer Q2 1 3".
    let trimmed = args.trim();
    let (target_idx, rest) = parse_question_target(trimmed);
    let rest = rest.trim();
    if rest.is_empty() {
        return SlashResult::Reply(ANSWER_USAGE.to_string());
    }

    let first_tok = rest.split_whitespace().next().unwrap_or("");
    if first_tok.parse::<u32>().is_err() {
        return SlashResult::Reply(ANSWER_USAGE.to_string());
    }

    // Split the remainder into "all numeric indices" (single/multi select) vs
    // "first token + free text" (Other).
    let toks: Vec<&str> = rest.split(|c: char| c == ',' || c == ' ')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();
    let (answer_text, is_custom) = if toks.iter().all(|t| t.parse::<u32>().is_ok()) {
        (toks.join(" "), false)
    } else {
        // Other: first token is index marker, the rest is verbatim text.
        let parts: Vec<&str> = rest.splitn(2, ' ').collect();
        (rest.to_string(), parts.len() == 2)
    };

    let binding = match bindings.get(chat_id) {
        Some(b) => b,
        None => return SlashResult::Reply("No session bound".to_string()),
    };
    let session_prefix = format!("{}:", binding.session_id);
    let mut map = waiters.lock().await;
    let key = match map.keys().find(|k| k.starts_with(&session_prefix)).cloned() {
        Some(k) => k,
        None => {
            drop(map);
            return SlashResult::Inject { text: answer_text, receipt: "Answered".to_string() };
        }
    };

    let entry = map.get_mut(&key).unwrap();
    let questions = entry.input.get("questions").and_then(|v| v.as_array()).cloned();
    let questions = match questions {
        Some(q) if !q.is_empty() => q,
        _ => {
            // Not an AskUserQuestion — legacy passthrough to CC as a single allow.
            let e = map.remove(&key).unwrap();
            let _ = e.sender.send(PermissionResponse::allow_with(None, None));
            drop(map);
            return SlashResult::Reply(format!("Answered: {}", answer_text));
        }
    };

    let q_idx = target_idx.unwrap_or(0);
    if q_idx >= questions.len() {
        return SlashResult::Reply(format!("无效问题号 Q{}，本提示共 {} 个问题", q_idx + 1, questions.len()));
    }

    let (question_text, value) = match answer_for_question(&questions[q_idx], &answer_text, is_custom) {
        Some(pair) => pair,
        None => return SlashResult::Reply("无法解析该问题，请改用桌面弹窗".to_string()),
    };
    entry.pending_answers.insert(question_text, value);

    // Which questions are still unanswered?
    let answered: std::collections::HashSet<&String> = entry.pending_answers.keys().collect();
    let missing: Vec<usize> = questions.iter().enumerate()
        .filter(|(_, q)| {
            q.get("question").and_then(|v| v.as_str())
                .map(|qt| !answered.contains(&qt.to_string()))
                .unwrap_or(false)
        })
        .map(|(i, _)| i + 1)
        .collect();

    if missing.is_empty() {
        // All answered — send to CC and clear the waiter.
        let entry = map.remove(&key).unwrap();
        let answers = serde_json::Value::Object(
            entry.pending_answers.iter()
                .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
                .collect(),
        );
        // updatedInput must satisfy AskUserQuestion's input schema, which
        // requires the original `questions` array. Echo it back alongside the
        // answers, otherwise CC rejects the override with a schema error.
        let updated_input = serde_json::json!({
            "questions": questions,
            "answers": answers,
        });
        let _ = entry.sender.send(PermissionResponse::allow_with(Some(updated_input.clone()), None));
        drop(map);
        SlashResult::Reply(format!("Answered: {}", updated_input.get("answers").unwrap()))
    } else {
        let progress = format!(
            "已记录 Q{}。还需回答：{}（用 /answer Q<号> <选项> 继续）",
            q_idx + 1,
            missing.iter().map(|n| format!("Q{}", n)).collect::<Vec<_>>().join(" "),
        );
        drop(map);
        SlashResult::Reply(progress)
    }
}

const ANSWER_USAGE: &str = "用法: /answer N（单选）, /answer N M（多选）, /answer N 文本（Other）; 多问题用 /answer Q2 1";

/// Parse an optional leading "QN" / "qN" target. Returns (Some(0-based idx), remainder)
/// or (None, original) when no target prefix is present.
fn parse_question_target(s: &str) -> (Option<usize>, &str) {
    let mut it = s.splitn(2, char::is_whitespace);
    let head = it.next().unwrap_or("");
    if (head.starts_with('Q') || head.starts_with('q')) && head.len() > 1 {
        if let Ok(n) = head[1..].parse::<usize>() {
            if n >= 1 {
                return (Some(n - 1), it.next().unwrap_or(""));
            }
        }
    }
    (None, s)
}

/// Resolve one question's answer: map numeric indices to option labels (multi
/// joined with ", "), or use custom text verbatim. Returns (question_text, value).
fn answer_for_question(q: &serde_json::Value, answer_text: &str, is_custom: bool) -> Option<(String, String)> {
    let question = q.get("question").and_then(|v| v.as_str())?;
    let options = q.get("options").and_then(|v| v.as_array());

    let value = if is_custom {
        // Drop the leading index marker, keep the free text.
        answer_text.splitn(2, ' ').nth(1).unwrap_or(answer_text).trim().to_string()
    } else {
        let labels: Vec<String> = answer_text
            .split_whitespace()
            .filter_map(|t| t.parse::<usize>().ok())
            .filter_map(|idx| {
                options
                    .and_then(|opts| opts.get(idx.checked_sub(1)?))
                    .and_then(|o| o.get("label"))
                    .and_then(|l| l.as_str())
                    .map(|s| s.to_string())
            })
            .collect();
        if labels.is_empty() { answer_text.to_string() } else { labels.join(", ") }
    };

    Some((question.to_string(), value))
}
