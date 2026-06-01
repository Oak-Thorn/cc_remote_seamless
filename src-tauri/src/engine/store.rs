use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredMessage {
    pub id: i64,
    pub session_id: String,
    pub source: String,
    pub text: String,
    pub timestamp: i64,
}

pub struct MessageStore {
    conn: Mutex<Connection>,
}

impl MessageStore {
    pub fn open(path: &str) -> Result<Self, String> {
        let conn = Connection::open(path).map_err(|e| e.to_string())?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                source TEXT NOT NULL,
                text TEXT NOT NULL,
                timestamp INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_session ON messages(session_id);"
        ).map_err(|e| e.to_string())?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    pub fn open_in_memory() -> Result<Self, String> {
        Self::open(":memory:")
    }

    pub fn insert(&self, session_id: &str, source: &str, text: &str, timestamp: i64) -> Result<i64, String> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO messages (session_id, source, text, timestamp) VALUES (?1, ?2, ?3, ?4)",
            params![session_id, source, text, timestamp],
        ).map_err(|e| e.to_string())?;
        Ok(conn.last_insert_rowid())
    }

    pub fn get_by_session(&self, session_id: &str, limit: usize) -> Vec<StoredMessage> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, session_id, source, text, timestamp FROM messages WHERE session_id = ?1 ORDER BY timestamp DESC LIMIT ?2"
        ).unwrap();
        stmt.query_map(params![session_id, limit as i64], |row| {
            Ok(StoredMessage {
                id: row.get(0)?,
                session_id: row.get(1)?,
                source: row.get(2)?,
                text: row.get(3)?,
                timestamp: row.get(4)?,
            })
        }).unwrap().filter_map(|r| r.ok()).collect()
    }
    pub fn clear_by_session(&self, session_id: &str) {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM messages WHERE session_id = ?1", params![session_id]).ok();
    }

    pub fn get_all(&self, limit: usize) -> Vec<StoredMessage> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, session_id, source, text, timestamp FROM messages ORDER BY timestamp DESC LIMIT ?1"
        ).unwrap();
        stmt.query_map(params![limit as i64], |row| {
            Ok(StoredMessage {
                id: row.get(0)?,
                session_id: row.get(1)?,
                source: row.get(2)?,
                text: row.get(3)?,
                timestamp: row.get(4)?,
            })
        }).unwrap().filter_map(|r| r.ok()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_and_query() {
        let store = MessageStore::open_in_memory().unwrap();
        store.insert("sess_1", "cli", "hello", 1000).unwrap();
        store.insert("sess_1", "feishu", "world", 1001).unwrap();
        let msgs = store.get_by_session("sess_1", 10);
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].text, "world");
        assert_eq!(msgs[1].text, "hello");
    }

    #[test]
    fn different_sessions_isolated() {
        let store = MessageStore::open_in_memory().unwrap();
        store.insert("sess_1", "cli", "msg1", 1000).unwrap();
        store.insert("sess_2", "feishu", "msg2", 1001).unwrap();
        let msgs = store.get_by_session("sess_1", 10);
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].text, "msg1");
    }
}
