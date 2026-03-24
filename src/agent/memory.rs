use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

use crate::llm::types::{ChatResponse, Message, MessageContent, Role, StopReason};
use crate::tools::registry::ToolResult;

// --- Session ---

#[derive(Debug, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub messages: Vec<Message>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(default)]
    pub needs_consolidation: bool,
}

impl Session {
    pub fn new(id: &str) -> Self {
        Self {
            id: id.to_string(),
            messages: Vec::new(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            needs_consolidation: false,
        }
    }

    pub fn add_message(&mut self, role: Role, content: &str) {
        self.messages.push(Message {
            role,
            content: MessageContent::Text { text: content.to_string() },
        });
        self.updated_at = Utc::now();
    }

    pub fn add_tool_use_message(&mut self, response: &ChatResponse) {
        self.messages.push(Message::assistant_tool_use(
            response.text.clone(),
            response.tool_calls.clone(),
        ));
        self.updated_at = Utc::now();
    }

    pub fn add_tool_result(&mut self, tool_use_id: &str, result: ToolResult) {
        let content = match &result {
            ToolResult::Success(s) => s.clone(),
            ToolResult::Error(e) => format!("Error: {e}"),
        };
        self.messages.push(Message::tool_result(tool_use_id, &content));
        self.updated_at = Utc::now();
    }

    pub fn message_count(&self) -> usize {
        self.messages.len()
    }

    /// Return messages formatted for LLM context
    pub fn messages_for_context(&self) -> Vec<Message> {
        self.messages.clone()
    }
}

// --- SessionStore ---

pub struct SessionStore {
    sessions: HashMap<String, Session>,
    data_dir: PathBuf,
}

impl SessionStore {
    pub fn new(data_dir: PathBuf) -> Self {
        Self {
            sessions: HashMap::new(),
            data_dir,
        }
    }

    pub fn get_or_load(&mut self, id: &str) -> &mut Session {
        if !self.sessions.contains_key(id) {
            let session = self.load_from_disk(id).unwrap_or_else(|_| Session::new(id));
            self.sessions.insert(id.to_string(), session);
        }
        self.sessions.get_mut(id).unwrap()
    }

    pub fn persist(&self, id: &str) -> Result<()> {
        if let Some(session) = self.sessions.get(id) {
            let sessions_dir = self.data_dir.join("sessions");
            std::fs::create_dir_all(&sessions_dir)?;
            let path = sessions_dir.join(format!("{id}.jsonl"));
            let content: String = session
                .messages
                .iter()
                .map(|m| serde_json::to_string(m).unwrap_or_default())
                .collect::<Vec<_>>()
                .join("\n");
            std::fs::write(&path, content)?;
            tracing::debug!("Persisted session {id} ({} messages)", session.messages.len());
        }
        Ok(())
    }

    pub fn persist_all(&self) -> Result<()> {
        for id in self.sessions.keys() {
            self.persist(id)?;
        }
        Ok(())
    }

    fn load_from_disk(&self, id: &str) -> Result<Session> {
        let path = self.data_dir.join(format!("sessions/{id}.jsonl"));
        let content = std::fs::read_to_string(&path)?;
        let messages: Vec<Message> = content
            .lines()
            .filter(|line| !line.trim().is_empty())
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect();

        Ok(Session {
            id: id.to_string(),
            messages,
            created_at: Utc::now(), // approximate — could parse from file metadata
            updated_at: Utc::now(),
            needs_consolidation: false,
        })
    }
}

// --- MemoryManager ---

pub struct MemoryManager {
    data_dir: PathBuf,
}

impl MemoryManager {
    pub fn new(data_dir: PathBuf) -> Self {
        Self { data_dir }
    }

    pub fn read_memory(&self) -> Result<String> {
        let path = self.data_dir.join("memory/MEMORY.md");
        Ok(std::fs::read_to_string(&path).unwrap_or_default())
    }

    pub fn append_memory(&self, key: &str, value: &str) -> Result<()> {
        let path = self.data_dir.join("memory/MEMORY.md");
        let mut content = std::fs::read_to_string(&path).unwrap_or_default();
        let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M");
        content.push_str(&format!("\n- [{timestamp}] {key}: {value}"));
        std::fs::write(&path, content)?;
        Ok(())
    }

    pub fn append_daily_note(&self, note: &str) -> Result<()> {
        let date = chrono::Local::now().format("%Y-%m-%d");
        let path = self.data_dir.join(format!("memory/{date}.md"));

        let mut content = if path.exists() {
            std::fs::read_to_string(&path)?
        } else {
            format!("## {date}\n")
        };

        content.push_str(&format!("\n- {note}"));
        std::fs::write(&path, content)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_add_messages() {
        let mut session = Session::new("test");
        session.add_message(Role::User, "Hello");
        session.add_message(Role::Assistant, "Hi there!");
        assert_eq!(session.message_count(), 2);
        assert_eq!(session.messages[0].content_text(), "Hello");
        assert_eq!(session.messages[1].content_text(), "Hi there!");
    }

    #[test]
    fn test_session_roundtrip_jsonl() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("sessions")).unwrap();
        let mut store = SessionStore::new(dir.path().to_path_buf());

        // Create and populate session
        {
            let session = store.get_or_load("abc");
            session.add_message(Role::User, "Hello");
            session.add_message(Role::Assistant, "Hi!");
        }
        store.persist("abc").unwrap();

        // Load from fresh store
        let mut store2 = SessionStore::new(dir.path().to_path_buf());
        let session2 = store2.get_or_load("abc");
        assert_eq!(session2.message_count(), 2);
        assert_eq!(session2.messages[0].content_text(), "Hello");
    }

    #[test]
    fn test_session_store_creates_new() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = SessionStore::new(dir.path().to_path_buf());
        let session = store.get_or_load("new-session");
        assert_eq!(session.id, "new-session");
        assert_eq!(session.message_count(), 0);
    }

    #[test]
    fn test_memory_manager_append_and_read() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("memory")).unwrap();
        let mgr = MemoryManager::new(dir.path().to_path_buf());

        mgr.append_memory("name", "Jiekai").unwrap();
        mgr.append_memory("color", "blue").unwrap();

        let memory = mgr.read_memory().unwrap();
        assert!(memory.contains("name: Jiekai"));
        assert!(memory.contains("color: blue"));
    }

    #[test]
    fn test_daily_note() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("memory")).unwrap();
        let mgr = MemoryManager::new(dir.path().to_path_buf());

        mgr.append_daily_note("User prefers Celsius").unwrap();
        mgr.append_daily_note("Created morning cron job").unwrap();

        let date = chrono::Local::now().format("%Y-%m-%d");
        let path = dir.path().join(format!("memory/{date}.md"));
        let content = std::fs::read_to_string(path).unwrap();
        assert!(content.contains("Celsius"));
        assert!(content.contains("morning cron"));
    }
}
