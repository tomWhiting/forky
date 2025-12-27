//! Session model representing a Claude session.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A session represents a Claude conversation session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    /// Unique session identifier from Claude.
    pub id: String,
    /// Fork ID this session belongs to (if any).
    pub fork_id: Option<String>,
    /// When the session was created.
    pub created_at: DateTime<Utc>,
}

impl Session {
    /// Create a new session.
    pub fn new(id: String, fork_id: Option<String>) -> Self {
        Self {
            id,
            fork_id,
            created_at: Utc::now(),
        }
    }
}
