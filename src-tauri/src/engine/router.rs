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
}

impl BindingStore {
    pub fn new() -> Self {
        Self {
            bindings: RwLock::new(HashMap::new()),
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
}
