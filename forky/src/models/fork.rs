//! Fork model representing a forked Claude session.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Status of a fork.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ForkStatus {
    /// Fork is currently running.
    Running,
    /// Fork completed successfully.
    Completed,
    /// Fork failed with an error.
    Failed,
}

impl ForkStatus {
    /// Convert status to string for database storage.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }

    /// Parse status from database string.
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "running" | "active" => Some(Self::Running),
            "completed" | "complete" | "done" => Some(Self::Completed),
            "failed" | "error" => Some(Self::Failed),
            _ => None,
        }
    }
}

impl std::fmt::Display for ForkStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// A fork represents a spawned Claude session.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(clippy::struct_field_names)]
pub struct Fork {
    /// Unique identifier for the fork.
    pub id: String,
    /// Session ID of the parent session (where fork was initiated from).
    pub parent_session_id: Option<String>,
    /// Session ID of the forked session (new session created).
    pub fork_session_id: Option<String>,
    /// AI provider used (e.g., "claude").
    pub ai_provider: String,
    /// Human-readable name for the fork.
    pub name: Option<String>,
    /// Current status of the fork.
    pub status: ForkStatus,
    /// Whether the fork has been marked as read.
    pub read: bool,
    /// When the fork was created.
    pub created_at: DateTime<Utc>,
    /// When the fork completed (if applicable).
    pub completed_at: Option<DateTime<Utc>>,
}

impl Fork {
    /// Create a new fork with default values.
    pub fn new(id: String, parent_session_id: Option<String>) -> Self {
        Self {
            id,
            parent_session_id,
            fork_session_id: None,
            ai_provider: "claude".to_string(),
            name: None,
            status: ForkStatus::Running,
            read: false,
            created_at: Utc::now(),
            completed_at: None,
        }
    }
}
