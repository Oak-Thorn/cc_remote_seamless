pub mod agent;
pub mod platform;
pub mod engine;
pub mod hook;
pub mod pty;
pub mod window;
pub mod config;
pub mod logging;
pub mod auto_approve;
mod commands;

use agent::claude_code::ClaudeCodeConnector;
use agent::pi::PiConnector;
use agent::{AgentConnector, AgentEvent};
use config::PlatformConfig;
use engine::Engine;
use hook::server::{start_hook_server, HookEvent, PermissionResponse, PermissionWaiters};
use hook::installer::install_claude_hooks;
use platform::feishu::FeishuPlatform;
use platform::telegram::TelegramPlatform;
use platform::{IMMessage, IMPlatform};
use std::collections::HashMap;
use std::sync::Arc;
use tauri::{Emitter, Manager};
use tokio::sync::{mpsc, Mutex};

pub fn run() {
    let log_dir = logging::log_dir();
    let _ = std::fs::create_dir_all(&log_dir);
    logging::init(false, log_dir);
    logging::cleanup_old_logs(logging::RETAIN_DAYS);

    std::panic::set_hook(Box::new(|info| {
        tracing::error!("PANIC: {}", info);
        eprintln!("{}", info);
    }));

    engine::init_start_time();

    let app_config = config::load();
    let hook_port = app_config.hook_port();

    install_pi_extension();
    install_claude_hooks(hook_port);

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .setup(move |app| {
            let handle = app.handle().clone();

            let idle_sound = app_config.general.as_ref()
                .map(|g| g.sounds.idle.clone())
                .unwrap_or_else(|| "Glass".to_string());
            let permission_sound = app_config.general.as_ref()
                .map(|g| g.sounds.permission.clone())
                .unwrap_or_else(|| "Hero".to_string());
            let resource_dir = handle.path().resource_dir().ok().map(|p| p.to_string_lossy().to_string());
            let mut engine = Engine::new(":memory:", idle_sound, permission_sound, resource_dir).expect("create engine");
            let connector = Arc::new(ClaudeCodeConnector::new("/tmp"));
            engine.register_agent(connector.clone());

            let pi_connector = Arc::new(PiConnector::new());
            engine.register_agent(pi_connector.clone());

            // Resolve sidecar path for feishu platforms
            let sidecar_path = {
                let exe_sidecar = std::env::current_exe()
                    .ok()
                    .and_then(|p| p.parent().map(|d| d.to_path_buf()))
                    .unwrap_or_default()
                    .join("sidecar")
                    .join("feishu-gateway");
                if exe_sidecar.exists() {
                    exe_sidecar.to_string_lossy().to_string()
                } else {
                    let manifest_dir = env!("CARGO_MANIFEST_DIR");
                    std::path::Path::new(manifest_dir)
                        .parent().unwrap()
                        .join("sidecar").join("feishu-gateway").join("feishu-gateway")
                        .to_string_lossy().to_string()
                }
            };

            // Register all platforms from config
            let (im_tx, im_rx) = mpsc::unbounded_channel::<IMMessage>();
            for (platform_id, platform_cfg) in &app_config.platforms {
                match platform_cfg {
                    PlatformConfig::Feishu { app_id, app_secret, .. } => {
                        tracing::info!("Registering Feishu platform '{}', sidecar={}", platform_id, sidecar_path);
                        let feishu: Arc<dyn IMPlatform> = Arc::new(
                            FeishuPlatform::new(platform_id, app_id, app_secret, &sidecar_path)
                        );
                        feishu.subscribe(im_tx.clone());
                        engine.register_platform(feishu.clone());
                        let feishu_connect = feishu.clone();
                        tauri::async_runtime::spawn(async move {
                            if let Err(e) = feishu_connect.connect().await {
                                tracing::error!("Feishu '{}' connect failed: {}", feishu_connect.id(), e);
                            } else {
                                tracing::info!("Feishu platform '{}' connected", feishu_connect.id());
                            }
                        });
                    }
                    PlatformConfig::Telegram { bot_token, .. } => {
                        tracing::info!("Registering Telegram platform '{}'", platform_id);
                        let tg: Arc<dyn IMPlatform> = Arc::new(
                            TelegramPlatform::new(platform_id, bot_token, platform_cfg.chat_id())
                        );
                        tg.subscribe(im_tx.clone());
                        engine.register_platform(tg.clone());
                        let tg_connect = tg.clone();
                        tauri::async_runtime::spawn(async move {
                            if let Err(e) = tg_connect.connect().await {
                                tracing::error!("Telegram '{}' connect failed: {}", tg_connect.id(), e);
                            } else {
                                tracing::info!("Telegram platform '{}' connected", tg_connect.id());
                            }
                        });
                    }
                }
            }

            // Build agent_platforms mapping from config
            let mut agent_platforms: HashMap<String, Vec<String>> = HashMap::new();
            for (agent_id, agent_cfg) in &app_config.agents {
                agent_platforms.insert(agent_id.clone(), agent_cfg.platforms.clone());
            }
            engine.set_agent_platforms(agent_platforms);

            // Register platform → chat_id mapping
            for (platform_id, platform_cfg) in &app_config.platforms {
                engine.set_platform_chat_id(platform_id, platform_cfg.chat_id());
            }

            let permission_waiters: PermissionWaiters = Arc::new(Mutex::new(HashMap::new()));
            engine.permission_waiters = Some(permission_waiters.clone());

            let engine = Arc::new(Mutex::new(engine));

            // Hook server
            let (hook_tx, mut hook_rx) = mpsc::unbounded_channel::<HookEvent>();
            let waiters_for_server = permission_waiters.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = start_hook_server(hook_port, hook_tx, waiters_for_server).await {
                    tracing::error!("Hook server failed: {}", e);
                }
            });

            // Permission popup coordinator: serialize popups into a single
            // queue and dismiss each as soon as its request is resolved on any
            // surface (popup submit, IM reply, timeout, or native CC terminal).
            let popup_queue = window::popup_queue::PopupQueue::new();
            let session_states: window::popup_queue::SessionStates =
                Arc::new(Mutex::new(HashMap::new()));
            {
                let pq = popup_queue.clone();
                let coord_handle = handle.clone();
                let coord_waiters = permission_waiters.clone();
                let coord_states = session_states.clone();
                tauri::async_runtime::spawn(async move {
                    pq.run(coord_handle, coord_waiters, coord_states).await;
                });
            }

            // IM message dispatch (all platforms → Engine)
            {
                let engine_for_im = engine.clone();
                let handle_for_im = handle.clone();
                let mut im_rx = im_rx;
                tauri::async_runtime::spawn(async move {
                    while let Some(msg) = im_rx.recv().await {
                        tracing::info!("IM message received (im->app): platform={} chat_id={} text={}", msg.platform, msg.chat_id, msg.text);
                        let eng = engine_for_im.lock().await;
                        eng.handle_im_message(msg).await;
                        let binding_change = eng.last_binding_change.lock().unwrap().take();
                        drop(eng);
                        if let Some(session_id) = binding_change {
                            let _ = handle_for_im.emit("binding-changed", serde_json::json!({
                                "session_id": session_id,
                            }));
                        }
                    }
                });
            }

            // Collect agent → chat_ids mapping for auto-binding
            let mut agent_chat_ids: HashMap<String, Vec<String>> = HashMap::new();
            for (agent_id, agent_cfg) in &app_config.agents {
                let chat_ids: Vec<String> = agent_cfg.platforms.iter()
                    .filter_map(|pid| app_config.platforms.get(pid))
                    .map(|p| p.chat_id().to_string())
                    .collect();
                agent_chat_ids.insert(agent_id.clone(), chat_ids);
            }

            // Hook event dispatch
            let connector_for_hooks = connector.clone();
            let pi_for_hooks = pi_connector.clone();
            let handle_for_hooks = handle.clone();
            let engine_for_hooks = engine.clone();
            let agent_chat_ids_for_hooks = agent_chat_ids.clone();
            let permission_waiters_for_hooks = permission_waiters.clone();
            let popup_queue_for_hooks = popup_queue.clone();
            tauri::async_runtime::spawn(async move {
                while let Some(event) = hook_rx.recv().await {
                    // Auto-bind active session only on events that signal the
                    // user's attention is (or should be) on this window:
                    // permission requests, question elicitations, and message
                    // I/O (prompt submit + agent stop). Lifecycle/tool events
                    // (SessionStart, Pre/PostToolUse, compaction, …) no longer
                    // drift the active binding — they fire constantly while a
                    // session works and caused active to thrash across windows.
                    let (sid, agent_id) = match &event {
                        HookEvent::PromptSubmit { session_id, .. } => (Some(session_id.as_str()), "claude-code"),
                        HookEvent::PermissionRequest { session_id, .. } => (Some(session_id.as_str()), "claude-code"),
                        HookEvent::Stop { session_id, .. } => (Some(session_id.as_str()), "claude-code"),
                        HookEvent::Elicitation { session_id, .. } => (Some(session_id.as_str()), "claude-code"),
                        HookEvent::PiInput { session_id, .. } => (Some(session_id.as_str()), "pi"),
                        HookEvent::PiPermissionRequest { session_id, .. } => (Some(session_id.as_str()), "pi"),
                        HookEvent::PiStop { session_id, .. } => (Some(session_id.as_str()), "pi"),
                        _ => (None, "claude-code"),
                    };
                    if let Some(session_id) = sid {
                        let eng = engine_for_hooks.lock().await;
                        let chat_ids = agent_chat_ids_for_hooks.get(agent_id).cloned().unwrap_or_default();
                        for chat_id in &chat_ids {
                            if !eng.bindings.is_pinned(chat_id) {
                                let current = eng.bindings.get(chat_id);
                                if current.as_ref().map(|b| b.session_id.as_str()) != Some(session_id) {
                                    eng.bindings.bind(chat_id, agent_id, session_id);
                                    tracing::info!("Active session updated: {} -> chat_id={}", session_id, chat_id);
                                }
                            }
                        }
                        drop(eng);
                    }

                    if let HookEvent::PermissionRequest { ref session_id, ref tool, ref input, ref request_id, .. } = event {
                        if auto_approve::is_enabled() {
                            if let Some(entry) = permission_waiters_for_hooks.lock().await.remove(request_id) {
                                let _ = entry.sender.send(PermissionResponse::allow());
                                tracing::info!("Auto-approved permission: tool={} id={}", tool, request_id);
                            }
                        } else {
                            let _ = handle_for_hooks.emit("permission-request", serde_json::json!({
                                "session_id": session_id,
                                "tool": tool,
                                "input": input,
                                "request_id": request_id,
                            }));
                            popup_queue_for_hooks.enqueue(window::popup_queue::PopupItem {
                                request_id: request_id.clone(),
                                session_id: session_id.clone(),
                                tool: tool.clone(),
                                input: input.clone(),
                            }).await;
                            let has_always = permission_waiters_for_hooks.lock().await
                                .get(request_id)
                                .map(|e| !e.suggestions.is_empty())
                                .unwrap_or(false);
                            let card = build_permission_card(tool, input, has_always);
                            let eng = engine_for_hooks.lock().await;
                            eng.forward_card_to_platforms("claude-code", session_id, card).await;
                            drop(eng);
                        }
                    }
                    if let HookEvent::Elicitation { ref session_id, ref tool, ref input, ref request_id, .. } = event {
                        let _ = handle_for_hooks.emit("permission-request", serde_json::json!({
                            "session_id": session_id,
                            "tool": tool,
                            "input": input,
                            "request_id": request_id,
                        }));
                        popup_queue_for_hooks.enqueue(window::popup_queue::PopupItem {
                            request_id: request_id.clone(),
                            session_id: session_id.clone(),
                            tool: tool.clone(),
                            input: input.clone(),
                        }).await;
                        let card = build_permission_card(tool, input, false);
                        let eng = engine_for_hooks.lock().await;
                        eng.forward_card_to_platforms("claude-code", session_id, card).await;
                        drop(eng);
                    }
                    if let HookEvent::PromptSubmit { ref session_id, ref prompt, .. } = event {
                        if let Some(text) = prompt {
                            let eng = engine_for_hooks.lock().await;
                            let now = chrono::Utc::now().timestamp();
                            eng.messages.insert(session_id, "cli", text, now).ok();
                            eng.forward_to_platforms("claude-code", session_id, text).await;
                            drop(eng);
                            let _ = handle_for_hooks.emit("messages-updated", serde_json::json!({
                                "session_id": session_id,
                            }));
                        }
                    }
                    if let HookEvent::Stop { ref session_id, ref response, .. } = event {
                        if let Some(text) = response {
                            let eng = engine_for_hooks.lock().await;
                            let now = chrono::Utc::now().timestamp();
                            eng.messages.insert(session_id, "agent", text, now).ok();
                            eng.bindings.store_last_output_for_session("claude-code", session_id, text);
                            if let Some(chat_id) = eng.bindings.find_chat_for_session("claude-code", session_id) {
                                if !eng.bindings.is_muted(&chat_id) {
                                    let display_text = truncate_middle(text, 200);
                                    eng.forward_to_platforms("claude-code", session_id, &display_text).await;
                                }
                            }
                            drop(eng);
                            let _ = handle_for_hooks.emit("messages-updated", serde_json::json!({
                                "session_id": session_id,
                            }));
                        }
                    }
                    if let HookEvent::PiPermissionRequest { ref session_id, ref tool, ref input, ref request_id, .. } = event {
                        if auto_approve::is_enabled() {
                            if let Some(entry) = permission_waiters_for_hooks.lock().await.remove(request_id) {
                                let _ = entry.sender.send(PermissionResponse::allow());
                                tracing::info!("Auto-approved permission: tool={} id={}", tool, request_id);
                            }
                        } else {
                            let _ = handle_for_hooks.emit("permission-request", serde_json::json!({
                                "session_id": session_id,
                                "tool": tool,
                                "input": input,
                                "request_id": request_id,
                            }));
                            popup_queue_for_hooks.enqueue(window::popup_queue::PopupItem {
                                request_id: request_id.clone(),
                                session_id: session_id.clone(),
                                tool: tool.clone(),
                                input: input.clone(),
                            }).await;
                            let card = build_permission_card(tool, input, false);
                            let eng = engine_for_hooks.lock().await;
                            eng.forward_card_to_platforms("pi", session_id, card).await;
                            drop(eng);
                        }
                    }
                    if let HookEvent::PiInput { ref session_id, ref text, .. } = event {
                        let eng = engine_for_hooks.lock().await;
                        let now = chrono::Utc::now().timestamp();
                        eng.messages.insert(session_id, "cli", text, now).ok();
                        eng.forward_to_platforms("pi", session_id, text).await;
                        drop(eng);
                        let _ = handle_for_hooks.emit("messages-updated", serde_json::json!({
                            "session_id": session_id,
                        }));
                    }
                    let pi_event = matches!(&event,
                        HookEvent::PiSessionStart { .. } | HookEvent::PiSessionEnd { .. }
                        | HookEvent::PiInput { .. } | HookEvent::PiPreToolUse { .. }
                        | HookEvent::PiPostToolUse { .. } | HookEvent::PiPermissionRequest { .. }
                        | HookEvent::PiStop { .. } | HookEvent::PiAgentStart { .. }
                        | HookEvent::PiPreCompact { .. } | HookEvent::PiPostCompact { .. }
                    );
                    if pi_event {
                        pi_for_hooks.handle_hook_event(event);
                    } else {
                        connector_for_hooks.handle_hook_event(event);
                    }
                }
            });

            // Agent event dispatch
            let (agent_tx, mut agent_rx) = mpsc::unbounded_channel::<AgentEvent>();
            connector.subscribe(agent_tx.clone());
            pi_connector.subscribe(agent_tx);
            connector.discover_existing_sessions();
            pi_connector.discover_existing_sessions();

            // Periodic reconciliation
            let connector_for_reconcile = connector.clone();
            let pi_for_reconcile = pi_connector.clone();
            tauri::async_runtime::spawn(async move {
                let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));
                loop {
                    interval.tick().await;
                    connector_for_reconcile.reconcile_from_files();
                    pi_for_reconcile.reap_stale();
                }
            });

            // Auto-bind most recent session per agent to its configured chat_ids
            {
                let engine_for_bind = engine.clone();
                let connector_for_bind = connector.clone();
                let pi_for_bind = pi_connector.clone();
                let agent_chat_ids_for_bind = agent_chat_ids.clone();
                tauri::async_runtime::spawn(async move {
                    let eng = engine_for_bind.lock().await;
                    let cc_sessions = connector_for_bind.discover_sessions().await;
                    if let Some(latest) = cc_sessions.last() {
                        if let Some(chat_ids) = agent_chat_ids_for_bind.get("claude-code") {
                            for chat_id in chat_ids {
                                eng.bindings.bind(chat_id, "claude-code", &latest.id);
                                tracing::info!("Initial bind: session={} -> chat_id={}", latest.id, chat_id);
                            }
                        }
                    }
                    let pi_sessions = pi_for_bind.discover_sessions().await;
                    if let Some(latest) = pi_sessions.last() {
                        if let Some(chat_ids) = agent_chat_ids_for_bind.get("pi") {
                            for chat_id in chat_ids {
                                eng.bindings.bind(chat_id, "pi", &latest.id);
                                tracing::info!("Initial bind: pi session={} -> chat_id={}", latest.id, chat_id);
                            }
                        }
                    }
                });
            }

            let engine_for_events = engine.clone();
            let handle_for_events = handle.clone();
            let session_states_for_events = session_states.clone();
            tauri::async_runtime::spawn(async move {
                while let Some(event) = agent_rx.recv().await {
                    let is_output = matches!(&event, AgentEvent::Output { .. });
                    let output_session_id = if let AgentEvent::Output { ref session_id, .. } = event {
                        Some(session_id.clone())
                    } else {
                        None
                    };
                    if let AgentEvent::StateChange { ref session_id, ref state } = event {
                        session_states_for_events.lock().await.insert(session_id.clone(), state.clone());
                    }
                    let eng = engine_for_events.lock().await;
                    eng.handle_agent_event(event).await;
                    drop(eng);
                    if is_output {
                        let _ = handle_for_events.emit("messages-updated", serde_json::json!({
                            "session_id": output_session_id.unwrap(),
                        }));
                    }
                    let _ = handle_for_events.emit("sessions-updated", ());
                }
            });

            app.manage(engine);
            app.manage(permission_waiters);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_sessions,
            commands::get_messages,
            commands::get_all_messages,
            commands::bind_session,
            commands::inject_input,
            commands::respond_permission,
            commands::get_permission_request,
            commands::frontend_log,
            commands::pin_session,
            commands::get_active_session,
            commands::get_config_path,
            commands::get_log_dir,
            commands::open_log_dir,
            commands::set_log_to_file,
            commands::set_auto_approve,
            commands::get_home_dir,
            commands::open_config_dir,
            commands::read_config_file,
            commands::start_feishu_register,
            commands::open_settings,
            commands::open_terminal,
            commands::play_sound,
            commands::set_sound_preference,
            commands::list_available_sounds,
            commands::list_available_icons,
            commands::read_icon_svg,
            commands::open_custom_dir,
            commands::fix_svg_icons,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn build_permission_card(tool: &str, input: &str, has_always: bool) -> serde_json::Value {
    let obj: serde_json::Value = serde_json::from_str(input).unwrap_or_default();
    let mut lines = Vec::new();

    fn truncate(s: &str, max: usize) -> String {
        if s.len() <= max { s.to_string() } else { format!("{}...", &s[..max]) }
    }

    match tool {
        "AskUserQuestion" => {
            let questions = obj.get("questions").and_then(|v| v.as_array());
            let multi_question = questions.map(|q| q.len() > 1).unwrap_or(false);
            if let Some(questions) = questions {
                for (qi, q) in questions.iter().enumerate() {
                    // Prefix multi-question prompts with Q1./Q2. so the user
                    // knows which question each option block belongs to.
                    let q_prefix = if multi_question { format!("Q{} ", qi + 1) } else { String::new() };
                    if let Some(header) = q.get("header").and_then(|v| v.as_str()) {
                        lines.push(format!("**{}{}**", q_prefix, header));
                    }
                    if let Some(question) = q.get("question").and_then(|v| v.as_str()) {
                        lines.push(question.to_string());
                    }
                    let multi = q.get("multiSelect").and_then(|v| v.as_bool()).unwrap_or(false);
                    if multi {
                        lines.push("_(可多选)_".to_string());
                    }
                    if let Some(opts) = q.get("options").and_then(|v| v.as_array()) {
                        for (i, opt) in opts.iter().enumerate() {
                            let label = opt.get("label").and_then(|v| v.as_str()).unwrap_or("");
                            let desc = opt.get("description").and_then(|v| v.as_str()).unwrap_or("");
                            lines.push(format!("{}. **{}** - {}", i + 1, label, desc));
                        }
                        lines.push(format!("{}. **Other** - 自定义文本", opts.len() + 1));
                    }
                    if multi_question {
                        lines.push(String::new());
                    }
                }
            }
        }
        "Bash" => {
            if let Some(cmd) = obj.get("command").and_then(|v| v.as_str()) {
                lines.push(format!("**Command**\n```\n{}\n```", truncate(cmd, 300)));
            }
            if let Some(desc) = obj.get("description").and_then(|v| v.as_str()) {
                lines.push(format!("**Description:** {}", desc));
            }
        }
        "Write" | "Read" => {
            if let Some(path) = obj.get("file_path").and_then(|v| v.as_str()) {
                lines.push(format!("**File:** `{}`", path));
            }
            if let Some(content) = obj.get("content").and_then(|v| v.as_str()) {
                lines.push(format!("**Content**\n```\n{}\n```", truncate(content, 200)));
            }
        }
        "Edit" => {
            if let Some(path) = obj.get("file_path").and_then(|v| v.as_str()) {
                lines.push(format!("**File:** `{}`", path));
            }
            if let Some(old) = obj.get("old_string").and_then(|v| v.as_str()) {
                lines.push(format!("**Replace**\n```\n{}\n```", truncate(old, 150)));
            }
            if let Some(new) = obj.get("new_string").and_then(|v| v.as_str()) {
                lines.push(format!("**With**\n```\n{}\n```", truncate(new, 150)));
            }
        }
        "WebFetch" | "WebSearch" => {
            if let Some(url) = obj.get("url").and_then(|v| v.as_str()) {
                lines.push(format!("**URL:** {}", url));
            }
            if let Some(q) = obj.get("query").and_then(|v| v.as_str()) {
                lines.push(format!("**Query:** {}", q));
            }
        }
        _ => {
            if let Some(map) = obj.as_object() {
                for (k, v) in map.iter().take(4) {
                    let val = match v.as_str() {
                        Some(s) => truncate(s, 150),
                        None => truncate(&v.to_string(), 150),
                    };
                    lines.push(format!("**{}:** {}", k.replace('_', " "), val));
                }
            } else if !input.is_empty() {
                lines.push(truncate(input, 300));
            }
        }
    }

    let header_template = match tool {
        "Bash" | "Write" | "Edit" | "NotebookEdit" => "orange",
        "AskUserQuestion" => "purple",
        _ => "blue",
    };

    let title = match tool {
        "AskUserQuestion" => "Question".to_string(),
        _ => format!("Permission Request: {}", tool),
    };

    let note = match tool {
        "AskUserQuestion" => {
            let multi_question = obj.get("questions")
                .and_then(|v| v.as_array())
                .map(|q| q.len() > 1)
                .unwrap_or(false);
            if multi_question {
                "多问题：用 /answer Q<号> <选项> 逐题回答（如 /answer Q1 2、/answer Q4 1 2 多选）。答齐所有问题后自动提交。也可用桌面弹窗".to_string()
            } else {
                "回复 /answer N（单选）、/answer N M（多选）、/answer N 文本（Other）".to_string()
            }
        }
        _ if has_always => "回复 /allow（本次放行）、/always（永久放行此类操作）或 /deny（拒绝）。也可用桌面弹窗".to_string(),
        _ => "回复 /allow（放行）或 /deny（拒绝）。也可用桌面弹窗".to_string(),
    };

    let body = lines.join("\n");
    serde_json::json!({
        "header": {
            "template": header_template,
            "title": {"tag": "plain_text", "content": title}
        },
        "elements": [
            {"tag": "markdown", "content": body},
            {"tag": "note", "elements": [{"tag": "plain_text", "content": note}]}
        ]
    })
}

fn truncate_middle(text: &str, keep: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= keep * 2 {
        return text.to_string();
    }
    let head: String = chars[..keep].iter().collect();
    let tail: String = chars[chars.len() - keep..].iter().collect();
    format!("{}\n\n... (omitted {} chars) ...\n\n{}", head, chars.len() - keep * 2, tail)
}

fn install_pi_extension() {
    let target_dir = match directories::BaseDirs::new() {
        Some(d) => d.home_dir().join(".pi").join("agent").join("extensions"),
        None => return,
    };
    if let Err(e) = std::fs::create_dir_all(&target_dir) {
        tracing::warn!("install_pi_extension: mkdir {:?} failed: {}", target_dir, e);
        return;
    }
    let target = target_dir.join("cc-remote.ts");

    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let candidates = [
        std::path::PathBuf::from(manifest_dir).parent().unwrap()
            .join("resource").join("pi-extension").join("cc-remote.ts"),
        std::env::current_exe().ok()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()))
            .unwrap_or_default()
            .join("resource").join("pi-extension").join("cc-remote.ts"),
    ];
    let source = match candidates.iter().find(|p| p.exists()) {
        Some(p) => p,
        None => {
            tracing::warn!("install_pi_extension: source not found in {:?}", candidates);
            return;
        }
    };
    let new_content = match std::fs::read_to_string(source) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("install_pi_extension: read source failed: {}", e);
            return;
        }
    };
    let needs_write = match std::fs::read_to_string(&target) {
        Ok(existing) => existing != new_content,
        Err(_) => true,
    };
    if needs_write {
        if let Err(e) = std::fs::write(&target, &new_content) {
            tracing::warn!("install_pi_extension: write {:?} failed: {}", target, e);
        } else {
            tracing::info!("Pi extension installed to {:?}", target);
        }
    } else {
        tracing::info!("Pi extension up to date at {:?}", target);
    }
}
