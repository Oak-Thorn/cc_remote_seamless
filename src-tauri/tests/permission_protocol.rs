//! End-to-end protocol test against the real hook HTTP server.
//!
//! Spins up `start_hook_server` on an ephemeral port, then drives the two
//! Claude-Code interactions that previously failed:
//!   1. AskUserQuestion (PermissionRequest) → answered with updatedInput.answers
//!   2. A normal tool (Bash) permission → allow / deny
//! and asserts the JSON returned to CC matches CC's PermissionRequest zod union.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use cc_remote_seamless_lib::hook::server::{
    start_hook_server, HookEvent, PermissionResponse, PermissionWaiters,
};
use tokio::sync::{mpsc, Mutex};

async fn boot() -> (u16, mpsc::UnboundedReceiver<HookEvent>, PermissionWaiters) {
    let (tx, rx) = mpsc::unbounded_channel();
    let waiters: PermissionWaiters = Arc::new(Mutex::new(HashMap::new()));
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener); // free it; server rebinds the same port immediately below

    let w = waiters.clone();
    tokio::spawn(async move {
        let _ = start_hook_server(port, tx, w).await;
    });

    // wait until /health responds
    let client = reqwest::Client::new();
    for _ in 0..50 {
        if client
            .get(format!("http://127.0.0.1:{}/health", port))
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    (port, rx, waiters)
}

/// Drain the event channel until we observe the request_id CC's hook created,
/// then resolve that waiter with `resp`. Mirrors what the engine/slash layer does.
async fn resolve_pending(
    waiters: &PermissionWaiters,
    resp_for: impl Fn(&serde_json::Value) -> PermissionResponse,
) {
    // The waiter is inserted synchronously inside the handler before it sends
    // the event, so a short spin is enough.
    for _ in 0..100 {
        let mut map = waiters.lock().await;
        if let Some(key) = map.keys().next().cloned() {
            let entry = map.remove(&key).unwrap();
            let resp = resp_for(&entry.input);
            let _ = entry.sender.send(resp);
            return;
        }
        drop(map);
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("no pending waiter appeared");
}

#[tokio::test]
async fn ask_user_question_answer_round_trip() {
    let (port, _rx, waiters) = boot().await;
    let client = reqwest::Client::new();

    let payload = serde_json::json!({
        "session_id": "sess-1",
        "hook_event_name": "PermissionRequest",
        "tool_name": "AskUserQuestion",
        "tool_input": {
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
        }
    });

    // Resolver task: pretend the user picked option 3 via /answer, i.e. send
    // allow + updatedInput = { questions, answers: { question: label } }.
    // updatedInput must satisfy AskUserQuestion's input schema, so it echoes
    // back the original `questions` array alongside the answers.
    let w = waiters.clone();
    let resolver = tokio::spawn(async move {
        resolve_pending(&w, |input| {
            let questions = input["questions"].clone();
            let q = input["questions"][0]["question"].as_str().unwrap();
            let label = input["questions"][0]["options"][2]["label"].as_str().unwrap();
            PermissionResponse::allow_with(
                Some(serde_json::json!({ "questions": questions, "answers": { q: label } })),
                None,
            )
        })
        .await;
    });

    let body: serde_json::Value = client
        .post(format!("http://127.0.0.1:{}/permission", port))
        .json(&payload)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    resolver.await.unwrap();

    assert_eq!(
        body,
        serde_json::json!({
            "hookSpecificOutput": {
                "hookEventName": "PermissionRequest",
                "decision": {
                    "behavior": "allow",
                    "updatedInput": {
                        "questions": [{
                            "header": "方向",
                            "question": "选哪个方向?",
                            "multiSelect": false,
                            "options": [
                                {"label": "补序列图", "description": "d1"},
                                {"label": "补测试章节", "description": "d2"},
                                {"label": "校对与纠错", "description": "d3"}
                            ]
                        }],
                        "answers": { "选哪个方向?": "校对与纠错" }
                    }
                }
            }
        }),
        "AskUserQuestion answer must come back as decision.updatedInput with questions + answers"
    );
}

#[tokio::test]
async fn bash_permission_allow_round_trip() {
    let (port, _rx, waiters) = boot().await;
    let client = reqwest::Client::new();

    let payload = serde_json::json!({
        "session_id": "sess-2",
        "hook_event_name": "PermissionRequest",
        "tool_name": "Bash",
        "tool_input": { "command": "ls" }
    });

    let w = waiters.clone();
    let resolver = tokio::spawn(async move {
        resolve_pending(&w, |_| PermissionResponse::allow()).await;
    });

    let body: serde_json::Value = client
        .post(format!("http://127.0.0.1:{}/permission", port))
        .json(&payload)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    resolver.await.unwrap();

    assert_eq!(
        body["hookSpecificOutput"]["decision"],
        serde_json::json!({ "behavior": "allow" }),
        "allow must serialize as a bare behavior with no message/updatedInput"
    );
}

#[tokio::test]
async fn bash_permission_deny_round_trip() {
    let (port, _rx, waiters) = boot().await;
    let client = reqwest::Client::new();

    let payload = serde_json::json!({
        "session_id": "sess-3",
        "hook_event_name": "PermissionRequest",
        "tool_name": "Bash",
        "tool_input": { "command": "rm -rf /" }
    });

    let w = waiters.clone();
    let resolver = tokio::spawn(async move {
        resolve_pending(&w, |_| PermissionResponse::deny("Denied by user")).await;
    });

    let body: serde_json::Value = client
        .post(format!("http://127.0.0.1:{}/permission", port))
        .json(&payload)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    resolver.await.unwrap();

    assert_eq!(
        body["hookSpecificOutput"]["decision"],
        serde_json::json!({ "behavior": "deny", "message": "Denied by user" }),
        "deny must carry message and no updatedInput"
    );
}
