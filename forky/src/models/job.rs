//! Job model representing a task within a fork.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Status of a job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JobStatus {
    /// Job is pending execution.
    Pending,
    /// Job is currently running.
    Running,
    /// Job completed successfully.
    Completed,
    /// Job failed with an error.
    Failed,
}

impl JobStatus {
    /// Convert status to string for database storage.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }

    /// Parse status from database string.
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "pending" => Some(Self::Pending),
            "running" => Some(Self::Running),
            "completed" => Some(Self::Completed),
            "failed" => Some(Self::Failed),
            _ => None,
        }
    }
}

impl std::fmt::Display for JobStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// A job represents a specific task or message sent to a fork.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    /// Unique identifier for the job.
    pub id: String,
    /// Description of the job (the message sent).
    pub description: String,
    /// Current status of the job.
    pub status: JobStatus,
    /// Fork ID this job belongs to.
    pub fork_id: String,
    /// Session ID used for this job.
    pub session_id: Option<String>,
    /// Output from the job (if completed).
    pub output: Option<String>,
    /// When the job was created.
    pub created_at: DateTime<Utc>,
    /// When the job completed (if applicable).
    pub completed_at: Option<DateTime<Utc>>,
}

impl Job {
    /// Create a new job.
    pub fn new(id: String, description: String, fork_id: String) -> Self {
        Self {
            id,
            description,
            status: JobStatus::Running,
            fork_id,
            session_id: None,
            output: None,
            created_at: Utc::now(),
            completed_at: None,
        }
    }
}
