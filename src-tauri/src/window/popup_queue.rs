use crate::agent::SessionState;
use crate::hook::server::PermissionWaiters;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use tauri::{AppHandle, Manager};
use tokio::sync::Mutex;

/// One queued permission popup. Carries everything `show_permission_popup`
/// needs so the coordinator can render it later, out of band from the hook.
#[derive(Clone)]
pub struct PopupItem {
    pub request_id: String,
    pub session_id: String,
    pub tool: String,
    pub input: String,
}

/// Latest known agent state per session, updated from the agent-event loop.
/// The coordinator reads this to notice a permission being answered in the
/// native CC terminal (session leaves `WaitingPermission`).
pub type SessionStates = Arc<Mutex<HashMap<String, SessionState>>>;

#[derive(Default)]
struct QueueInner {
    pending: VecDeque<PopupItem>,
    current: Option<PopupItem>,
    /// Set once the current popup's session has been observed in
    /// `WaitingPermission`, so a later transition away from it can be read as a
    /// native-terminal answer without racing the initial transition.
    armed: bool,
}

/// Serializes permission popups: one on screen at a time, the rest queued,
/// each dismissed as soon as its request is resolved on any surface.
#[derive(Clone)]
pub struct PopupQueue {
    inner: Arc<Mutex<QueueInner>>,
}

impl PopupQueue {
    pub fn new() -> Self {
        Self { inner: Arc::new(Mutex::new(QueueInner::default())) }
    }
    /// Enqueue a request unless it is already showing or already queued.
    pub async fn enqueue(&self, item: PopupItem) {
        let mut inner = self.inner.lock().await;
        let dup = inner.current.as_ref().map(|c| c.request_id == item.request_id).unwrap_or(false)
            || inner.pending.iter().any(|p| p.request_id == item.request_id);
        if dup {
            return;
        }
        inner.pending.push_back(item);
    }

    /// Run the coordinator forever: dismiss the on-screen popup once its
    /// request is resolved (popup submit, IM reply, timeout, or a native
    /// CC-terminal answer) and show the next still-pending request.
    pub async fn run(self, app: AppHandle, waiters: PermissionWaiters, states: SessionStates) {
        const TICK_MS: u64 = 150;
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(TICK_MS)).await;
            self.tick(&app, &waiters, &states).await;
        }
    }

    async fn tick(&self, app: &AppHandle, waiters: &PermissionWaiters, states: &SessionStates) {
        // --- Evaluate the popup currently on screen ---
        let current = self.inner.lock().await.current.clone();
        if let Some(cur) = current {
            let waiter_pending = waiters.lock().await.contains_key(&cur.request_id);
            let window_open = app.get_webview_window("permission").is_some();
            let state = states.lock().await.get(&cur.session_id).cloned();

            let mut inner = self.inner.lock().await;
            if state == Some(SessionState::WaitingPermission) {
                inner.armed = true;
            }
            // Native CC-terminal answer: the session left WaitingPermission
            // after we had armed on it.
            let left_permission =
                inner.armed && matches!(&state, Some(s) if *s != SessionState::WaitingPermission);
            let resolved = !waiter_pending || left_permission || !window_open;
            if !resolved {
                return;
            }
            inner.current = None;
            inner.armed = false;
            drop(inner);

            // Dismissed while still pending (CC moved on) → drop the stale
            // waiter so the hook stops blocking and it cannot reappear.
            if waiter_pending {
                waiters.lock().await.remove(&cur.request_id);
            }
            if let Some(win) = app.get_webview_window("permission") {
                let _ = win.destroy();
            }
        }

        // --- Show the next still-pending request ---
        let next = loop {
            let mut inner = self.inner.lock().await;
            if inner.current.is_some() {
                return;
            }
            let Some(candidate) = inner.pending.pop_front() else {
                return;
            };
            // Skip anything already resolved before it was ever shown.
            if !waiters.lock().await.contains_key(&candidate.request_id) {
                continue;
            }
            inner.current = Some(candidate.clone());
            inner.armed = false;
            break candidate;
        };

        if let Err(e) = super::show_permission_popup(
            app, &next.session_id, &next.tool, &next.input, &next.request_id,
        )
        .await
        {
            tracing::warn!("show_permission_popup failed: {}", e);
            self.inner.lock().await.current = None;
        }
    }
}
