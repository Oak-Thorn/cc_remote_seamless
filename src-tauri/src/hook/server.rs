use axum::{
    extract::State,
    http::StatusCode,
    routing::post,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot, Mutex};
use tracing::info;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookPayload {
    #[serde(alias = "sessionId")]
    pub session_id: Option<String>,
    #[serde(alias = "toolName")]
    pub tool_name: Option<String>,
    #[serde(alias = "toolInput")]
    pub tool_input: Option<serde_json::Value>,
    pub cwd: Option<String>,
}

#[derive(Debug, Clone)]
pub enum HookEvent {
    SessionStart { session_id: String, cwd: Option<String> },
    SessionEnd { session_id: String },
    PromptSubmit { session_id: String, prompt: Option<String>, cwd: Option<String> },
    PreToolUse { session_id: String, tool: String, input: String, cwd: Option<String> },
    PostToolUse { session_id: String, tool: String, input: String, cwd: Option<String> },
    PostToolUseFailure { session_id: String, tool: String, input: String, cwd: Option<String> },
    Stop { session_id: String, cwd: Option<String>, response: Option<String> },
    StopFailure { session_id: String, cwd: Option<String> },
    SubagentStart { session_id: String, cwd: Option<String> },
    SubagentStop { session_id: String, cwd: Option<String> },
    Notification { session_id: String, cwd: Option<String>, notification_type: Option<String> },
    Elicitation { session_id: String, tool: String, input: String, cwd: Option<String>, request_id: String },
    PreCompact { session_id: String, cwd: Option<String> },
    PostCompact { session_id: String, cwd: Option<String> },
    PermissionRequest { session_id: String, tool: String, input: String, cwd: Option<String>, request_id: String },
    PiSessionStart { session_id: String, cwd: Option<String>, pid: Option<i32>, inject_port: Option<u16> },
    PiSessionEnd { session_id: String },
    PiInput { session_id: String, text: String, cwd: Option<String> },
    PiPreToolUse { session_id: String, tool: String, input: String, cwd: Option<String> },
    PiPostToolUse { session_id: String, tool: String, input: String, is_error: bool, cwd: Option<String> },
    PiPermissionRequest { session_id: String, tool: String, input: String, cwd: Option<String>, request_id: String },
    PiStop { session_id: String, cwd: Option<String> },
    PiAgentStart { session_id: String, cwd: Option<String> },
    PiPreCompact { session_id: String, cwd: Option<String> },
    PiPostCompact { session_id: String, cwd: Option<String> },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionResponse {
    pub behavior: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "updatedPermissions")]
    pub updated_permissions: Option<Vec<serde_json::Value>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PermissionDecisionWrapper {
    #[serde(rename = "hookEventName")]
    hook_event_name: String,
    decision: PermissionResponse,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PermissionHookOutput {
    #[serde(rename = "hookSpecificOutput")]
    hook_specific_output: PermissionDecisionWrapper,
}

pub type HookEventSender = mpsc::UnboundedSender<HookEvent>;
pub type PermissionWaiters = Arc<Mutex<HashMap<String, PermissionWaiterEntry>>>;

pub struct PermissionWaiterEntry {
    pub sender: oneshot::Sender<PermissionResponse>,
    pub suggestions: Vec<serde_json::Value>,
}

struct HookState {
    tx: HookEventSender,
    permission_waiters: PermissionWaiters,
}

pub async fn start_hook_server(port: u16, tx: HookEventSender, permission_waiters: PermissionWaiters) -> Result<(), String> {
    let state = Arc::new(HookState { tx, permission_waiters });

    let app = Router::new()
        .route("/hook/session-start", post(handle_session_start))
        .route("/hook/session-end", post(handle_session_end))
        .route("/hook/prompt", post(handle_prompt))
        .route("/hook/pre-tool", post(handle_pre_tool))
        .route("/hook/post-tool", post(handle_post_tool))
        .route("/hook/post-tool-failure", post(handle_post_tool_failure))
        .route("/hook/stop", post(handle_stop))
        .route("/hook/stop_failure", post(handle_stop_failure))
        .route("/hook/subagent_start", post(handle_subagent_start))
        .route("/hook/subagent_stop", post(handle_subagent_stop))
        .route("/hook/notification", post(handle_notification))
        .route("/hook/elicitation", post(handle_elicitation))
        .route("/hook/pre_compact", post(handle_pre_compact))
        .route("/hook/post_compact", post(handle_post_compact))
        .route("/permission", post(handle_permission))
        .route("/pi/session_start", post(handle_pi_session_start))
        .route("/pi/session_end", post(handle_pi_session_end))
        .route("/pi/input", post(handle_pi_input))
        .route("/pi/pre_tool", post(handle_pi_pre_tool))
        .route("/pi/post_tool", post(handle_pi_post_tool))
        .route("/pi/permission", post(handle_pi_permission))
        .route("/pi/stop", post(handle_pi_stop))
        .route("/pi/agent_start", post(handle_pi_agent_start))
        .route("/pi/pre_compact", post(handle_pi_pre_compact))
        .route("/pi/post_compact", post(handle_pi_post_compact))
        .route("/health", axum::routing::get(|| async { "ok" }))
        .with_state(state);

    let addr = format!("127.0.0.1:{}", port);
    info!("Hook server listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .map_err(|e| format!("bind failed: {}", e))?;

    axum::serve(listener, app)
        .await
        .map_err(|e| format!("server error: {}", e))
}

async fn handle_session_start(
    State(state): State<Arc<HookState>>,
    Json(raw): Json<serde_json::Value>,
) -> StatusCode {
    info!("SessionStart raw payload: {}", raw);
    let session_id = raw.get("session_id").or_else(|| raw.get("sessionId"))
        .and_then(|v| v.as_str()).unwrap_or_default().to_string();
    let cwd = raw.get("cwd").or_else(|| raw.get("workingDirectory"))
        .and_then(|v| v.as_str()).map(|s| s.to_string());
    let _ = state.tx.send(HookEvent::SessionStart { session_id, cwd });
    StatusCode::OK
}

async fn handle_stop(
    State(state): State<Arc<HookState>>,
    Json(raw): Json<serde_json::Value>,
) -> StatusCode {
    info!("Stop raw payload: {}", raw);
    let session_id = raw.get("session_id").or_else(|| raw.get("sessionId"))
        .and_then(|v| v.as_str()).unwrap_or_default().to_string();
    let cwd = raw.get("cwd").and_then(|v| v.as_str()).map(|s| s.to_string());
    let response = raw.get("last_assistant_message").or_else(|| raw.get("response"))
        .and_then(|v| v.as_str()).map(|s| s.to_string());
    let _ = state.tx.send(HookEvent::Stop { session_id, cwd, response });
    StatusCode::OK
}

async fn handle_prompt(
    State(state): State<Arc<HookState>>,
    Json(raw): Json<serde_json::Value>,
) -> StatusCode {
    info!("Prompt raw payload: {}", raw);
    let session_id = raw.get("session_id").or_else(|| raw.get("sessionId"))
        .and_then(|v| v.as_str()).unwrap_or_default().to_string();
    let cwd = raw.get("cwd").and_then(|v| v.as_str()).map(|s| s.to_string());
    let prompt = raw.get("prompt").and_then(|v| v.as_str()).map(|s| s.to_string());
    let _ = state.tx.send(HookEvent::PromptSubmit { session_id, prompt, cwd });
    StatusCode::OK
}

async fn handle_pre_tool(
    State(state): State<Arc<HookState>>,
    Json(raw): Json<serde_json::Value>,
) -> StatusCode {
    info!("PreToolUse raw payload: {}", raw);
    let session_id = raw.get("session_id").or_else(|| raw.get("sessionId"))
        .and_then(|v| v.as_str()).unwrap_or_default().to_string();
    let cwd = raw.get("cwd").and_then(|v| v.as_str()).map(|s| s.to_string());
    let tool = raw.get("tool_name").or_else(|| raw.get("toolName"))
        .and_then(|v| v.as_str()).unwrap_or_default().to_string();
    let input = raw.get("tool_input").or_else(|| raw.get("toolInput"))
        .map(|v| v.to_string()).unwrap_or_default();
    let _ = state.tx.send(HookEvent::PreToolUse { session_id, tool, input, cwd });
    StatusCode::OK
}

async fn handle_session_end(
    State(state): State<Arc<HookState>>,
    Json(raw): Json<serde_json::Value>,
) -> StatusCode {
    info!("SessionEnd raw payload: {}", raw);
    let session_id = raw.get("session_id").or_else(|| raw.get("sessionId"))
        .and_then(|v| v.as_str()).unwrap_or_default().to_string();
    let _ = state.tx.send(HookEvent::SessionEnd { session_id });
    StatusCode::OK
}

async fn handle_post_tool(
    State(state): State<Arc<HookState>>,
    Json(raw): Json<serde_json::Value>,
) -> StatusCode {
    info!("PostToolUse raw payload: {}", raw);
    let session_id = raw.get("session_id").or_else(|| raw.get("sessionId"))
        .and_then(|v| v.as_str()).unwrap_or_default().to_string();
    let cwd = raw.get("cwd").and_then(|v| v.as_str()).map(|s| s.to_string());
    let tool = raw.get("tool_name").or_else(|| raw.get("toolName"))
        .and_then(|v| v.as_str()).unwrap_or_default().to_string();
    let input = raw.get("tool_input").or_else(|| raw.get("toolInput"))
        .map(|v| v.to_string()).unwrap_or_default();
    let _ = state.tx.send(HookEvent::PostToolUse { session_id, tool, input, cwd });
    StatusCode::OK
}

async fn handle_post_tool_failure(
    State(state): State<Arc<HookState>>,
    Json(raw): Json<serde_json::Value>,
) -> StatusCode {
    info!("PostToolUseFailure raw payload: {}", raw);
    let session_id = raw.get("session_id").or_else(|| raw.get("sessionId"))
        .and_then(|v| v.as_str()).unwrap_or_default().to_string();
    let cwd = raw.get("cwd").and_then(|v| v.as_str()).map(|s| s.to_string());
    let tool = raw.get("tool_name").or_else(|| raw.get("toolName"))
        .and_then(|v| v.as_str()).unwrap_or_default().to_string();
    let input = raw.get("tool_input").or_else(|| raw.get("toolInput"))
        .map(|v| v.to_string()).unwrap_or_default();
    let _ = state.tx.send(HookEvent::PostToolUseFailure { session_id, tool, input, cwd });
    StatusCode::OK
}

async fn handle_stop_failure(
    State(state): State<Arc<HookState>>,
    Json(raw): Json<serde_json::Value>,
) -> StatusCode {
    info!("StopFailure raw payload: {}", raw);
    let session_id = raw.get("session_id").or_else(|| raw.get("sessionId"))
        .and_then(|v| v.as_str()).unwrap_or_default().to_string();
    let cwd = raw.get("cwd").and_then(|v| v.as_str()).map(|s| s.to_string());
    let _ = state.tx.send(HookEvent::StopFailure { session_id, cwd });
    StatusCode::OK
}

async fn handle_subagent_start(
    State(state): State<Arc<HookState>>,
    Json(raw): Json<serde_json::Value>,
) -> StatusCode {
    info!("SubagentStart raw payload: {}", raw);
    let session_id = raw.get("session_id").or_else(|| raw.get("sessionId"))
        .and_then(|v| v.as_str()).unwrap_or_default().to_string();
    let cwd = raw.get("cwd").and_then(|v| v.as_str()).map(|s| s.to_string());
    let _ = state.tx.send(HookEvent::SubagentStart { session_id, cwd });
    StatusCode::OK
}

async fn handle_subagent_stop(
    State(state): State<Arc<HookState>>,
    Json(raw): Json<serde_json::Value>,
) -> StatusCode {
    info!("SubagentStop raw payload: {}", raw);
    let session_id = raw.get("session_id").or_else(|| raw.get("sessionId"))
        .and_then(|v| v.as_str()).unwrap_or_default().to_string();
    let cwd = raw.get("cwd").and_then(|v| v.as_str()).map(|s| s.to_string());
    let _ = state.tx.send(HookEvent::SubagentStop { session_id, cwd });
    StatusCode::OK
}

async fn handle_notification(
    State(state): State<Arc<HookState>>,
    Json(raw): Json<serde_json::Value>,
) -> StatusCode {
    info!("Notification raw payload: {}", raw);
    let session_id = raw.get("session_id").or_else(|| raw.get("sessionId"))
        .and_then(|v| v.as_str()).unwrap_or_default().to_string();
    let cwd = raw.get("cwd").and_then(|v| v.as_str()).map(|s| s.to_string());
    let notification_type = raw.get("notification_type")
        .and_then(|v| v.as_str()).map(|s| s.to_string());
    let _ = state.tx.send(HookEvent::Notification { session_id, cwd, notification_type });
    StatusCode::OK
}

async fn handle_elicitation(
    State(state): State<Arc<HookState>>,
    Json(raw): Json<serde_json::Value>,
) -> Json<PermissionHookOutput> {
    info!("Elicitation raw payload: {}", raw);
    let session_id = raw.get("session_id").or_else(|| raw.get("sessionId"))
        .and_then(|v| v.as_str()).unwrap_or_default().to_string();
    let cwd = raw.get("cwd").and_then(|v| v.as_str()).map(|s| s.to_string());
    let tool = raw.get("tool_name").or_else(|| raw.get("toolName"))
        .and_then(|v| v.as_str()).unwrap_or_default().to_string();
    let input = raw.get("tool_input").or_else(|| raw.get("toolInput"))
        .map(|v| v.to_string()).unwrap_or_default();
    let request_id = format!("eli:{}:{}", session_id, uuid::Uuid::new_v4());

    let (tx, rx) = oneshot::channel();
    state.permission_waiters.lock().await.insert(request_id.clone(), PermissionWaiterEntry { sender: tx, suggestions: vec![] });

    let _ = state.tx.send(HookEvent::Elicitation { session_id: session_id.clone(), tool: tool.clone(), input, cwd, request_id: request_id.clone() });

    info!("Elicitation waiting: tool={} id={}", tool, request_id);

    let decision = match tokio::time::timeout(std::time::Duration::from_secs(590), rx).await {
        Ok(Ok(response)) => response,
        _ => {
            state.permission_waiters.lock().await.remove(&request_id);
            PermissionResponse { behavior: "deny".to_string(), message: Some("Timeout".to_string()), updated_permissions: None }
        }
    };
    Json(PermissionHookOutput {
        hook_specific_output: PermissionDecisionWrapper {
            hook_event_name: "Elicitation".to_string(),
            decision,
        },
    })
}

async fn handle_pre_compact(
    State(state): State<Arc<HookState>>,
    Json(raw): Json<serde_json::Value>,
) -> StatusCode {
    info!("PreCompact raw payload: {}", raw);
    let session_id = raw.get("session_id").or_else(|| raw.get("sessionId"))
        .and_then(|v| v.as_str()).unwrap_or_default().to_string();
    let cwd = raw.get("cwd").and_then(|v| v.as_str()).map(|s| s.to_string());
    let _ = state.tx.send(HookEvent::PreCompact { session_id, cwd });
    StatusCode::OK
}

async fn handle_post_compact(
    State(state): State<Arc<HookState>>,
    Json(raw): Json<serde_json::Value>,
) -> StatusCode {
    info!("PostCompact raw payload: {}", raw);
    let session_id = raw.get("session_id").or_else(|| raw.get("sessionId"))
        .and_then(|v| v.as_str()).unwrap_or_default().to_string();
    let cwd = raw.get("cwd").and_then(|v| v.as_str()).map(|s| s.to_string());
    let _ = state.tx.send(HookEvent::PostCompact { session_id, cwd });
    StatusCode::OK
}

async fn handle_permission(
    State(state): State<Arc<HookState>>,
    Json(raw): Json<serde_json::Value>,
) -> Json<PermissionHookOutput> {
    info!("Permission raw payload: {}", raw);
    let session_id = raw.get("session_id").or_else(|| raw.get("sessionId"))
        .and_then(|v| v.as_str()).unwrap_or_default().to_string();
    let tool = raw.get("tool_name").or_else(|| raw.get("toolName"))
        .and_then(|v| v.as_str()).unwrap_or_default().to_string();
    let input = raw.get("tool_input").or_else(|| raw.get("toolInput"))
        .map(|v| v.to_string()).unwrap_or_default();
    let cwd = raw.get("cwd").and_then(|v| v.as_str()).map(|s| s.to_string());
    let suggestions: Vec<serde_json::Value> = raw.get("permission_suggestions")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let request_id = format!("{}:{}", session_id, uuid::Uuid::new_v4());

    let (tx, rx) = oneshot::channel();
    state.permission_waiters.lock().await.insert(request_id.clone(), PermissionWaiterEntry { sender: tx, suggestions });

    let _ = state.tx.send(HookEvent::PermissionRequest {
        session_id,
        tool: tool.clone(),
        input,
        cwd,
        request_id: request_id.clone(),
    });

    info!("Permission request waiting: tool={} id={}", tool, request_id);

    let decision = match tokio::time::timeout(std::time::Duration::from_secs(590), rx).await {
        Ok(Ok(response)) => response,
        _ => {
            state.permission_waiters.lock().await.remove(&request_id);
            PermissionResponse { behavior: "deny".to_string(), message: Some("Timeout".to_string()), updated_permissions: None }
        }
    };
    Json(PermissionHookOutput {
        hook_specific_output: PermissionDecisionWrapper {
            hook_event_name: "PermissionRequest".to_string(),
            decision,
        },
    })
}

async fn handle_pi_session_start(
    State(state): State<Arc<HookState>>,
    Json(raw): Json<serde_json::Value>,
) -> StatusCode {
    info!("Pi SessionStart payload: {}", raw);
    let session_id = raw.get("session_id").and_then(|v| v.as_str()).unwrap_or_default().to_string();
    let cwd = raw.get("cwd").and_then(|v| v.as_str()).map(|s| s.to_string());
    let pid = raw.get("pid").and_then(|v| v.as_i64()).map(|p| p as i32);
    let inject_port = raw.get("inject_port").and_then(|v| v.as_u64()).map(|p| p as u16);
    let _ = state.tx.send(HookEvent::PiSessionStart { session_id, cwd, pid, inject_port });
    StatusCode::OK
}

async fn handle_pi_session_end(
    State(state): State<Arc<HookState>>,
    Json(raw): Json<serde_json::Value>,
) -> StatusCode {
    info!("Pi SessionEnd payload: {}", raw);
    let session_id = raw.get("session_id").and_then(|v| v.as_str()).unwrap_or_default().to_string();
    let _ = state.tx.send(HookEvent::PiSessionEnd { session_id });
    StatusCode::OK
}

async fn handle_pi_input(
    State(state): State<Arc<HookState>>,
    Json(raw): Json<serde_json::Value>,
) -> StatusCode {
    info!("Pi Input payload: {}", raw);
    let session_id = raw.get("session_id").and_then(|v| v.as_str()).unwrap_or_default().to_string();
    let cwd = raw.get("cwd").and_then(|v| v.as_str()).map(|s| s.to_string());
    let text = raw.get("text").and_then(|v| v.as_str()).unwrap_or_default().to_string();
    let _ = state.tx.send(HookEvent::PiInput { session_id, text, cwd });
    StatusCode::OK
}

async fn handle_pi_pre_tool(
    State(state): State<Arc<HookState>>,
    Json(raw): Json<serde_json::Value>,
) -> StatusCode {
    info!("Pi PreTool payload: {}", raw);
    let session_id = raw.get("session_id").and_then(|v| v.as_str()).unwrap_or_default().to_string();
    let cwd = raw.get("cwd").and_then(|v| v.as_str()).map(|s| s.to_string());
    let tool = raw.get("tool_name").and_then(|v| v.as_str()).unwrap_or_default().to_string();
    let input = raw.get("tool_input").map(|v| v.to_string()).unwrap_or_default();
    let _ = state.tx.send(HookEvent::PiPreToolUse { session_id, tool, input, cwd });
    StatusCode::OK
}

async fn handle_pi_post_tool(
    State(state): State<Arc<HookState>>,
    Json(raw): Json<serde_json::Value>,
) -> StatusCode {
    info!("Pi PostTool payload: {}", raw);
    let session_id = raw.get("session_id").and_then(|v| v.as_str()).unwrap_or_default().to_string();
    let cwd = raw.get("cwd").and_then(|v| v.as_str()).map(|s| s.to_string());
    let tool = raw.get("tool_name").and_then(|v| v.as_str()).unwrap_or_default().to_string();
    let input = raw.get("output").map(|v| v.to_string()).unwrap_or_default();
    let is_error = raw.get("is_error").and_then(|v| v.as_bool()).unwrap_or(false);
    let _ = state.tx.send(HookEvent::PiPostToolUse { session_id, tool, input, is_error, cwd });
    StatusCode::OK
}

async fn handle_pi_permission(
    State(state): State<Arc<HookState>>,
    Json(raw): Json<serde_json::Value>,
) -> Json<PermissionResponse> {
    info!("Pi Permission payload: {}", raw);
    let session_id = raw.get("session_id").and_then(|v| v.as_str()).unwrap_or_default().to_string();
    let tool = raw.get("tool_name").and_then(|v| v.as_str()).unwrap_or_default().to_string();
    let input = raw.get("tool_input").map(|v| v.to_string()).unwrap_or_default();
    let cwd = raw.get("cwd").and_then(|v| v.as_str()).map(|s| s.to_string());
    let request_id = format!("pi:{}:{}", session_id, uuid::Uuid::new_v4());

    let (tx, rx) = oneshot::channel();
    state.permission_waiters.lock().await.insert(request_id.clone(), PermissionWaiterEntry { sender: tx, suggestions: vec![] });

    let _ = state.tx.send(HookEvent::PiPermissionRequest {
        session_id,
        tool: tool.clone(),
        input,
        cwd,
        request_id: request_id.clone(),
    });

    let decision = match tokio::time::timeout(std::time::Duration::from_secs(590), rx).await {
        Ok(Ok(response)) => response,
        _ => {
            state.permission_waiters.lock().await.remove(&request_id);
            PermissionResponse { behavior: "deny".to_string(), message: Some("Timeout".to_string()), updated_permissions: None }
        }
    };
    Json(decision)
}

async fn handle_pi_stop(
    State(state): State<Arc<HookState>>,
    Json(raw): Json<serde_json::Value>,
) -> StatusCode {
    let session_id = raw.get("session_id").and_then(|v| v.as_str()).unwrap_or_default().to_string();
    let cwd = raw.get("cwd").and_then(|v| v.as_str()).map(|s| s.to_string());
    info!("Pi Stop: session={}", session_id);
    let _ = state.tx.send(HookEvent::PiStop { session_id, cwd });
    StatusCode::OK
}

async fn handle_pi_agent_start(
    State(state): State<Arc<HookState>>,
    Json(raw): Json<serde_json::Value>,
) -> StatusCode {
    let session_id = raw.get("session_id").and_then(|v| v.as_str()).unwrap_or_default().to_string();
    let cwd = raw.get("cwd").and_then(|v| v.as_str()).map(|s| s.to_string());
    info!("Pi AgentStart: session={}", session_id);
    let _ = state.tx.send(HookEvent::PiAgentStart { session_id, cwd });
    StatusCode::OK
}

async fn handle_pi_pre_compact(
    State(state): State<Arc<HookState>>,
    Json(raw): Json<serde_json::Value>,
) -> StatusCode {
    let session_id = raw.get("session_id").and_then(|v| v.as_str()).unwrap_or_default().to_string();
    let cwd = raw.get("cwd").and_then(|v| v.as_str()).map(|s| s.to_string());
    info!("Pi PreCompact: session={}", session_id);
    let _ = state.tx.send(HookEvent::PiPreCompact { session_id, cwd });
    StatusCode::OK
}

async fn handle_pi_post_compact(
    State(state): State<Arc<HookState>>,
    Json(raw): Json<serde_json::Value>,
) -> StatusCode {
    let session_id = raw.get("session_id").and_then(|v| v.as_str()).unwrap_or_default().to_string();
    let cwd = raw.get("cwd").and_then(|v| v.as_str()).map(|s| s.to_string());
    info!("Pi PostCompact: session={}", session_id);
    let _ = state.tx.send(HookEvent::PiPostCompact { session_id, cwd });
    StatusCode::OK
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_state() -> (Arc<HookState>, mpsc::UnboundedReceiver<HookEvent>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let waiters = Arc::new(Mutex::new(HashMap::new()));
        (Arc::new(HookState { tx, permission_waiters: waiters }), rx)
    }

    #[tokio::test]
    async fn hook_event_sent_on_stop() {
        let (state, mut rx) = make_state();
        let payload = serde_json::json!({
            "session_id": "test_sess",
            "cwd": "/tmp/project"
        });

        handle_stop(State(state), Json(payload)).await;

        let event = rx.recv().await.unwrap();
        match event {
            HookEvent::Stop { session_id, cwd, .. } => {
                assert_eq!(session_id, "test_sess");
                assert_eq!(cwd, Some("/tmp/project".to_string()));
            }
            _ => panic!("wrong event type"),
        }
    }

    #[tokio::test]
    async fn hook_event_sent_on_pre_tool() {
        let (state, mut rx) = make_state();
        let payload = serde_json::json!({
            "session_id": "s1",
            "tool_name": "Write",
            "tool_input": {"file_path": "file.rs"}
        });

        handle_pre_tool(State(state), Json(payload)).await;

        let event = rx.recv().await.unwrap();
        match event {
            HookEvent::PreToolUse { session_id, tool, input, .. } => {
                assert_eq!(session_id, "s1");
                assert_eq!(tool, "Write");
                assert!(input.contains("file.rs"));
            }
            _ => panic!("wrong event type"),
        }
    }
}
