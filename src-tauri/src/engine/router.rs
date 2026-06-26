use std::collections::HashMap;
use std::sync::RwLock;

#[derive(Debug, Clone)]
pub struct Binding {
    pub agent_id: String,
    pub session_id: String,
    pub muted: bool,
    pub pinned: bool,
    pub last_output: Option<String>,
}

pub struct BindingStore {
    bindings: RwLock<HashMap<String, Binding>>,
    /// Per-chat record of the session the user last *routed a prompt to*
    /// (via `/p` or `/switch`). Distinct from the live binding, which drifts
    /// on every hook event — this is the baseline used to detect that drift
    /// and warn the user before a prompt lands on an unexpected window.
    last_routed: RwLock<HashMap<String, String>>,
}

impl BindingStore {
    pub fn new() -> Self {
        Self {
            bindings: RwLock::new(HashMap::new()),
            last_routed: RwLock::new(HashMap::new()),
        }
    }

    pub fn bind(&self, chat_id: &str, agent_id: &str, session_id: &str) {
        let mut map = self.bindings.write().unwrap();
        let muted = map.get(chat_id).map(|b| b.muted).unwrap_or(false);
        map.insert(chat_id.to_string(), Binding {
            agent_id: agent_id.to_string(),
            session_id: session_id.to_string(),
            muted,
            pinned: false,
            last_output: None,
        });
    }

    pub fn bind_pinned(&self, chat_id: &str, agent_id: &str, session_id: &str) {
        let mut map = self.bindings.write().unwrap();
        let muted = map.get(chat_id).map(|b| b.muted).unwrap_or(false);
        map.insert(chat_id.to_string(), Binding {
            agent_id: agent_id.to_string(),
            session_id: session_id.to_string(),
            muted,
            pinned: true,
            last_output: None,
        });
    }

    pub fn is_pinned(&self, chat_id: &str) -> bool {
        let map = self.bindings.read().unwrap();
        map.get(chat_id).map(|b| b.pinned).unwrap_or(false)
    }

    /// Set the pinned flag on an existing binding. Returns false when the chat
    /// has no binding to pin. Pinning stops hook-driven active drift; unpinning
    /// restores it.
    pub fn set_pinned(&self, chat_id: &str, pinned: bool) -> bool {
        let mut map = self.bindings.write().unwrap();
        match map.get_mut(chat_id) {
            Some(b) => { b.pinned = pinned; true }
            None => false,
        }
    }

    pub fn bind_pinned_session(&self, session_id: &str) {
        let mut map = self.bindings.write().unwrap();
        for (_, binding) in map.iter_mut() {
            if binding.session_id == session_id {
                binding.pinned = true;
                return;
            }
        }
        // If no existing binding points to this session, pin the first one found
        if let Some((_, binding)) = map.iter_mut().next() {
            binding.session_id = session_id.to_string();
            binding.pinned = true;
        }
    }

    pub fn get_active_session_id(&self) -> Option<String> {
        let map = self.bindings.read().unwrap();
        map.values().next().map(|b| b.session_id.clone())
    }

    pub fn unbind(&self, chat_id: &str) {
        let mut map = self.bindings.write().unwrap();
        map.remove(chat_id);
    }

    pub fn get(&self, chat_id: &str) -> Option<Binding> {
        let map = self.bindings.read().unwrap();
        map.get(chat_id).cloned()
    }

    pub fn find_chat_for_session(&self, agent_id: &str, session_id: &str) -> Option<String> {
        let map = self.bindings.read().unwrap();
        map.iter()
            .find(|(_, b)| b.agent_id == agent_id && b.session_id == session_id)
            .map(|(chat_id, _)| chat_id.clone())
    }

    pub fn set_muted(&self, chat_id: &str, muted: bool) {
        let mut map = self.bindings.write().unwrap();
        if let Some(b) = map.get_mut(chat_id) {
            b.muted = muted;
        }
    }

    pub fn is_muted(&self, chat_id: &str) -> bool {
        let map = self.bindings.read().unwrap();
        map.get(chat_id).map(|b| b.muted).unwrap_or(false)
    }

    pub fn store_last_output(&self, chat_id: &str, text: &str) {
        let mut map = self.bindings.write().unwrap();
        if let Some(b) = map.get_mut(chat_id) {
            b.last_output = Some(text.to_string());
        }
    }

    pub fn store_last_output_for_session(&self, agent_id: &str, session_id: &str, text: &str) {
        let mut map = self.bindings.write().unwrap();
        for b in map.values_mut() {
            if b.agent_id == agent_id && b.session_id == session_id {
                b.last_output = Some(text.to_string());
            }
        }
    }

    pub fn get_last_output(&self, chat_id: &str) -> Option<String> {
        let map = self.bindings.read().unwrap();
        map.get(chat_id).and_then(|b| b.last_output.clone())
    }

    /// Read the session this chat last *routed a prompt to* (drift baseline).
    pub fn get_last_routed(&self, chat_id: &str) -> Option<String> {
        let map = self.last_routed.read().unwrap();
        map.get(chat_id).cloned()
    }

    /// Record the session this chat just routed a prompt to. Call on every
    /// successful `/p` and on `/switch`, so the next `/p` can detect drift.
    pub fn set_last_routed(&self, chat_id: &str, session_id: &str) {
        let mut map = self.last_routed.write().unwrap();
        map.insert(chat_id.to_string(), session_id.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bind_and_get() {
        let store = BindingStore::new();
        store.bind("chat_1", "claude", "sess_a");
        let binding = store.get("chat_1").unwrap();
        assert_eq!(binding.agent_id, "claude");
        assert_eq!(binding.session_id, "sess_a");
    }

    #[test]
    fn unbind_removes() {
        let store = BindingStore::new();
        store.bind("chat_1", "claude", "sess_a");
        store.unbind("chat_1");
        assert!(store.get("chat_1").is_none());
    }

    #[test]
    fn find_chat_for_session() {
        let store = BindingStore::new();
        store.bind("chat_1", "claude", "sess_a");
        store.bind("chat_2", "claude", "sess_b");
        let found = store.find_chat_for_session("claude", "sess_a");
        assert_eq!(found, Some("chat_1".to_string()));
    }

    #[test]
    fn set_pinned_toggles_existing_binding() {
        let store = BindingStore::new();
        store.bind("chat_1", "claude", "sess_a");
        assert!(!store.is_pinned("chat_1"));

        assert!(store.set_pinned("chat_1", true));
        assert!(store.is_pinned("chat_1"));

        assert!(store.set_pinned("chat_1", false));
        assert!(!store.is_pinned("chat_1"));
    }

    #[test]
    fn set_pinned_returns_false_without_binding() {
        let store = BindingStore::new();
        assert!(!store.set_pinned("chat_x", true));
    }

    #[test]
    fn pin_survives_rebind_of_same_chat() {
        // bind() preserves muted; pin is independent. After pinning, a manual
        // re-bind clears pin (new Binding), which is expected — only the hook
        // loop checks is_pinned before re-binding.
        let store = BindingStore::new();
        store.bind("chat_1", "claude", "sess_a");
        store.set_pinned("chat_1", true);
        store.bind("chat_1", "claude", "sess_b");
        assert!(!store.is_pinned("chat_1"));
    }
}