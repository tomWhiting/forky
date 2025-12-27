//! Claude NDJSON event parsing.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Event type from Claude stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventType {
    /// System event.
    System,
    /// Assistant message.
    Assistant,
    /// User message.
    User,
    /// Result event (completion).
    Result,
    /// Error event.
    Error,
    /// Unknown event type.
    #[serde(other)]
    Unknown,
}

/// A parsed Claude NDJSON event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeEvent {
    /// Event type.
    #[serde(rename = "type")]
    pub event_type: Option<EventType>,

    /// Session ID (if present).
    pub session_id: Option<String>,

    /// Message content (for assistant/user events).
    pub message: Option<String>,

    /// Result data (for result events).
    pub result: Option<String>,

    /// Cost in USD (if present).
    pub cost_usd: Option<f64>,

    /// Whether this is a final/complete event.
    pub is_final: Option<bool>,

    /// Raw JSON value for additional fields.
    #[serde(flatten)]
    pub extra: Value,
}

impl ClaudeEvent {
    /// Parse a line of NDJSON into a Claude event.
    pub fn parse(line: &str) -> Option<Self> {
        let line = line.trim();
        if line.is_empty() {
            return None;
        }

        // Try to parse as JSON
        let value: Value = serde_json::from_str(line).ok()?;

        // Extract event type
        let event_type = value.get("type").and_then(Value::as_str).map(|s| match s {
            "system" => EventType::System,
            "assistant" => EventType::Assistant,
            "user" => EventType::User,
            "result" => EventType::Result,
            "error" => EventType::Error,
            _ => EventType::Unknown,
        });

        // Extract session_id from various locations
        let session_id = value
            .get("session_id")
            .and_then(Value::as_str)
            .map(String::from)
            .or_else(|| {
                value
                    .get("sessionId")
                    .and_then(Value::as_str)
                    .map(String::from)
            });

        // Extract message content
        let message = value
            .get("message")
            .and_then(Value::as_str)
            .map(String::from)
            .or_else(|| {
                value
                    .get("content")
                    .and_then(Value::as_str)
                    .map(String::from)
            });

        // Extract result
        let result = value
            .get("result")
            .and_then(Value::as_str)
            .map(String::from);

        // Extract cost
        let cost_usd = value.get("cost_usd").and_then(Value::as_f64);

        // Extract is_final
        let is_final = value.get("is_final").and_then(Value::as_bool);

        Some(Self {
            event_type,
            session_id,
            message,
            result,
            cost_usd,
            is_final,
            extra: value,
        })
    }

    /// Check if this is a result event.
    pub fn is_result(&self) -> bool {
        self.event_type == Some(EventType::Result)
    }

    /// Check if this is an assistant message.
    pub fn is_assistant(&self) -> bool {
        self.event_type == Some(EventType::Assistant)
    }

    /// Get the text content from the event.
    pub fn get_text(&self) -> Option<&str> {
        self.message.as_deref().or(self.result.as_deref())
    }
}
