//! Message model representing a message in a fork conversation.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Role of a message sender.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    /// Message from the user.
    User,
    /// Message from the assistant.
    Assistant,
    /// System message.
    System,
}

impl MessageRole {
    /// Convert role to string for database storage.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Assistant => "assistant",
            Self::System => "system",
        }
    }

    /// Parse role from database string.
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "user" => Some(Self::User),
            "assistant" => Some(Self::Assistant),
            "system" => Some(Self::System),
            _ => None,
        }
    }
}

impl std::fmt::Display for MessageRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// A message in a fork conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    /// Unique identifier for the message (auto-incremented by DB).
    pub id: i64,
    /// Fork ID this message belongs to.
    pub fork_id: String,
    /// Session ID this message was sent in.
    pub session_id: Option<String>,
    /// Role of the message sender.
    pub role: MessageRole,
    /// Content of the message.
    pub content: String,
    /// When the message was created.
    pub created_at: DateTime<Utc>,
}

impl Message {
    /// Create a new message (id will be set by database).
    pub fn new(fork_id: String, role: MessageRole, content: String) -> Self {
        Self {
            id: 0, // Will be set by database on insert
            fork_id,
            session_id: None,
            role,
            content,
            created_at: Utc::now(),
        }
    }
}
