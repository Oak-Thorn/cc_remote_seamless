use crate::agent::AgentConnector;
use crate::hook::server::{PermissionResponse, PermissionWaiters};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::info;

use super::router::BindingStore;
use super::store::MessageStore;

pub enum SlashResult {
    Reply(String),
    Inject(String),
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
        "/mute" => { bindings.set_muted(chat_id, true); SlashResult::Reply("Muted".to_string()) }
        "/unmute" => { bindings.set_muted(chat_id, false); SlashResult::Reply("Unmuted".to_string()) }
        "/full" => full(chat_id, bindings),
        "/p" => {
            if args.is_empty() { return SlashResult::Reply("Usage: /p <prompt>".to_string()); }
            if let Some(binding) = bindings.get(chat_id) {
                if let Some(agent) = agents.get(&binding.agent_id) {
                    for s in agent.discover_sessions().await {
                        if s.id == binding.session_id && s.state == crate::agent::SessionState::Busy {
                            return SlashResult::Reply("Session is busy, please wait until idle".to_string());
                        }
                    }
                }
            }
            SlashResult::Inject(args.to_string())
        }
        "/t" => { info!("Test message from Feishu: {}", args); SlashResult::Reply("ok".to_string()) }
        "/allow" => resolve_permission(chat_id, bindings, agents, permission_waiters, "allow").await,
        "/deny" => resolve_permission(chat_id, bindings, agents, permission_waiters, "deny").await,
        "/always" => resolve_permission(chat_id, bindings, agents, permission_waiters, "allowAlways").await,
        "/answer" => answer_question(args, chat_id, bindings, permission_waiters).await,
        "/change" => change_agent(args, chat_id, bindings, agents).await,
        "/clear" => clear_input(chat_id, bindings, agents).await,
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
                return SlashResult::Reply(format!(
                    "Session: {}\nAgent: {}\nState: {:?}\nDir: {}{}",
                    s.id, s.agent, s.state, s.working_dir.unwrap_or_default(), muted
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
            lines.push(format!("[{:?}] {} ({})", s.state, s.id, s.working_dir.unwrap_or_default()));
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
                return SlashResult::Reply(format!("Switched to session: {}\nDir: {}", s.id, s.working_dir.unwrap_or_default()));
            }
        }
    }
    SlashResult::Reply(format!("Session not found: {}", args))
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
            let (behavior, updated_permissions) = if action == "allowAlways" {
                ("allow".to_string(), Some(entry.suggestions))
            } else {
                (action.to_string(), None)
            };
            let _ = entry.sender.send(PermissionResponse {
                behavior,
                message: None,
                updated_permissions,
            });
            return SlashResult::Reply(format!("Permission: {}", action));
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
/stop - Stop current session task
/radar - Rediscover running agents
/answer 1 - Single select option 1
/answer 1 3 - Multi select options 1 and 3";

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

/// `/answer 1` or `/answer 1 3` (multi) or `/answer 3 自定义文本` (Other)
async fn answer_question(
    args: &str,
    chat_id: &str,
    bindings: &Arc<BindingStore>,
    waiters: &PermissionWaiters,
) -> SlashResult {
    if args.is_empty() {
        return SlashResult::Reply("Usage: /answer 1 or /answer 1 3 or /answer N <text>".to_string());
    }

    let parts: Vec<&str> = args.splitn(2, ' ').collect();
    let first = parts[0].trim();
    if first.parse::<u32>().is_err() {
        return SlashResult::Reply("Usage: /answer N or /answer N <text>".to_string());
    }

    let (answer_text, is_custom) = if parts.len() == 2 {
        let rest = parts[1].trim();
        let tokens: Vec<&str> = rest.split(|c: char| c == ',' || c == ' ')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect();
        if tokens.iter().all(|t| t.parse::<u32>().is_ok()) {
            let mut all = vec![first];
            all.extend(tokens);
            (all.join(" "), false)
        } else {
            (format!("{} {}", first, rest), true)
        }
    } else {
        (first.to_string(), false)
    };

    let binding = match bindings.get(chat_id) {
        Some(b) => b,
        None => return SlashResult::Reply("No session bound".to_string()),
    };
    let session_prefix = format!("{}:", binding.session_id);
    let mut map = waiters.lock().await;
    let key = map.keys().find(|k| k.starts_with(&session_prefix)).cloned();
    if let Some(request_id) = key {
        if let Some(entry) = map.remove(&request_id) {
            let _ = entry.sender.send(PermissionResponse {
                behavior: "allow".to_string(),
                message: Some(answer_text.clone()),
                updated_permissions: None,
            });
            let display = if is_custom { format!("Answered: {} (custom)", answer_text) } else { format!("Answered: {}", answer_text) };
            return SlashResult::Reply(display);
        }
    }
    drop(map);
    SlashResult::Inject(answer_text)
}
