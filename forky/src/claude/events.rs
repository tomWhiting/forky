//! Claude NDJSON event parsing.
//!
//! This module parses the streaming JSON output from Claude CLI.
//! Each event has a UUID and optionally links to a parent via `parent_tool_use_id`,
//! forming chains that can be stored as edges in a graph database.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Event type from Claude stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventType {
    /// System event (init, hook_response).
    System,
    /// Stream event (deltas during streaming).
    StreamEvent,
    /// Assistant message (complete).
    Assistant,
    /// User message (text or tool_result).
    User,
    /// Result event (session completion).
    Result,
    /// Error event.
    Error,
    /// Unknown event type.
    #[serde(other)]
    Unknown,
}

/// Subtype for system events.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SystemSubtype {
    /// Session initialization.
    Init,
    /// Hook response.
    HookResponse,
    /// Unknown subtype.
    #[serde(other)]
    Unknown,
}

/// A tool use block from an assistant message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolUse {
    /// Unique ID for this tool use (for linking tool_result).
    pub id: String,
    /// Tool name.
    pub name: String,
    /// Tool input as JSON.
    pub input: Value,
}

/// A tool result block from a user message (response to tool_use).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    /// The tool_use ID this result responds to.
    pub tool_use_id: String,
    /// Brief summary of the result content.
    pub content_summary: Option<String>,
    /// Whether the tool call was an error.
    pub is_error: bool,
}

/// Token usage statistics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TokenUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_creation_tokens: u64,
}

/// A parsed Claude NDJSON event with full schema support.
///
/// Every event has a UUID and session_id. Events can form chains via
/// `parent_tool_use_id`, which links to a tool_use block's ID in a parent message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeEvent {
    /// Unique identifier for this event.
    pub uuid: Option<String>,

    /// Session ID this event belongs to.
    pub session_id: Option<String>,

    /// Parent tool use ID (for subagent nesting / event chaining).
    pub parent_tool_use_id: Option<String>,

    /// Event type.
    #[serde(rename = "type")]
    pub event_type: Option<EventType>,

    /// Subtype (for system events).
    pub subtype: Option<String>,

    /// Message content (text extracted from content blocks).
    pub message: Option<String>,

    /// Thinking content (extracted from thinking blocks).
    pub thinking: Option<String>,

    /// Result data (for result events).
    pub result: Option<String>,

    /// Model used (e.g., "claude-opus-4-5-20251101").
    pub model: Option<String>,

    /// Claude message ID (msg_*).
    pub message_id: Option<String>,

    /// Role (assistant, user).
    pub role: Option<String>,

    /// Tool uses in this message (assistant messages).
    pub tool_uses: Vec<ToolUse>,

    /// Tool results in this message (user messages responding to tool_use).
    pub tool_results: Vec<ToolResult>,

    /// Token usage statistics.
    pub usage: Option<TokenUsage>,

    /// Cost in USD (if present).
    pub cost_usd: Option<f64>,

    /// Total cost in USD (from result events).
    pub total_cost_usd: Option<f64>,

    /// Whether this is a final/complete event.
    pub is_final: Option<bool>,

    /// Duration in milliseconds (from result events).
    pub duration_ms: Option<u64>,

    /// Number of turns (from result events).
    pub num_turns: Option<u32>,

    /// Tool use IDs contained in this event (for linking children).
    #[serde(default)]
    pub tool_use_ids: Vec<String>,

    /// Raw JSON value for the complete event.
    #[serde(flatten)]
    pub raw: Value,
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
            "stream_event" => EventType::StreamEvent,
            "assistant" => EventType::Assistant,
            "user" => EventType::User,
            "result" => EventType::Result,
            "error" => EventType::Error,
            _ => EventType::Unknown,
        });

        // Extract UUID
        let uuid = value.get("uuid").and_then(Value::as_str).map(String::from);

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

        // Extract parent_tool_use_id (for event chaining)
        let parent_tool_use_id = value
            .get("parent_tool_use_id")
            .and_then(Value::as_str)
            .map(String::from);

        // Extract subtype (for system events)
        let subtype = value
            .get("subtype")
            .and_then(Value::as_str)
            .map(String::from);

        // Extract from message object (for assistant/user events)
        let msg_obj = value.get("message");

        // Model from message.model
        let model = msg_obj
            .and_then(|m| m.get("model"))
            .and_then(Value::as_str)
            .map(String::from);

        // Message ID from message.id
        let message_id = msg_obj
            .and_then(|m| m.get("id"))
            .and_then(Value::as_str)
            .map(String::from);

        // Role from message.role
        let role = msg_obj
            .and_then(|m| m.get("role"))
            .and_then(Value::as_str)
            .map(String::from);

        // Extract text content from message.content blocks
        let message = extract_text_content(&value);

        // Extract thinking blocks (Claude's reasoning)
        let thinking = extract_thinking(&value);

        // Extract tool uses from message.content blocks
        let tool_uses = extract_tool_uses(&value);
        let tool_use_ids: Vec<String> = tool_uses.iter().map(|t| t.id.clone()).collect();

        // Extract tool results from user messages
        let tool_results = extract_tool_results(&value);

        // Extract usage from message.usage
        let usage = extract_usage(&value);

        // Extract result
        let result = value
            .get("result")
            .and_then(Value::as_str)
            .map(String::from);

        // Extract costs
        let cost_usd = value.get("cost_usd").and_then(Value::as_f64);
        let total_cost_usd = value.get("total_cost_usd").and_then(Value::as_f64);

        // Extract is_final / is_error
        let is_final = value
            .get("is_final")
            .and_then(Value::as_bool)
            .or_else(|| value.get("is_error").and_then(Value::as_bool));

        // Extract duration and turns
        let duration_ms = value.get("duration_ms").and_then(Value::as_u64);
        let num_turns = value
            .get("num_turns")
            .and_then(Value::as_u64)
            .map(|n| n as u32);

        Some(Self {
            uuid,
            session_id,
            parent_tool_use_id,
            event_type,
            subtype,
            message,
            thinking,
            result,
            model,
            message_id,
            role,
            tool_uses,
            tool_results,
            usage,
            cost_usd,
            total_cost_usd,
            is_final,
            duration_ms,
            num_turns,
            tool_use_ids,
            raw: value,
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

    /// Check if this is a system init event.
    pub fn is_init(&self) -> bool {
        self.event_type == Some(EventType::System) && self.subtype.as_deref() == Some("init")
    }

    /// Check if this event is nested (part of a subagent conversation).
    pub fn is_nested(&self) -> bool {
        self.parent_tool_use_id.is_some()
    }

    /// Get the text content from the event.
    pub fn get_text(&self) -> Option<&str> {
        self.message.as_deref().or(self.result.as_deref())
    }

    /// Get the event type as a string for labeling.
    pub fn type_label(&self) -> &str {
        match &self.event_type {
            Some(EventType::System) => "system",
            Some(EventType::StreamEvent) => "stream_event",
            Some(EventType::Assistant) => "assistant",
            Some(EventType::User) => "user",
            Some(EventType::Result) => "result",
            Some(EventType::Error) => "error",
            Some(EventType::Unknown) | None => "unknown",
        }
    }
}

/// Strip system reminders and other injected noise from content.
fn strip_noise(text: &str) -> String {
    use regex::Regex;

    // Remove <system-reminder>...</system-reminder> blocks
    let re = Regex::new(r"(?s)<system-reminder>.*?</system-reminder>").unwrap();
    let cleaned = re.replace_all(text, "");

    // Trim whitespace
    cleaned.trim().to_string()
}

/// Extract text content from message.content array.
/// Handles both text blocks and tool_result blocks.
/// Strips system reminders and other noise.
fn extract_text_content(value: &Value) -> Option<String> {
    if let Some(message) = value.get("message") {
        if let Some(content) = message.get("content").and_then(Value::as_array) {
            let mut texts = Vec::new();

            for block in content {
                match block.get("type").and_then(Value::as_str) {
                    Some("text") => {
                        if let Some(text) = block.get("text").and_then(Value::as_str) {
                            let cleaned = strip_noise(text);
                            if !cleaned.is_empty() {
                                texts.push(cleaned);
                            }
                        }
                    }
                    Some("tool_result") => {
                        if let Some(result_content) = block.get("content") {
                            match result_content {
                                Value::String(s) => {
                                    let cleaned = strip_noise(s);
                                    if !cleaned.is_empty() {
                                        texts.push(cleaned);
                                    }
                                }
                                Value::Array(arr) => {
                                    for item in arr {
                                        if let Some(text) = item.get("text").and_then(Value::as_str)
                                        {
                                            let cleaned = strip_noise(text);
                                            if !cleaned.is_empty() {
                                                texts.push(cleaned);
                                            }
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    _ => {}
                }
            }

            if !texts.is_empty() {
                return Some(texts.join("\n"));
            }
        }
    }

    // Fallback to direct message/content fields
    value
        .get("message")
        .and_then(Value::as_str)
        .map(|s| strip_noise(s))
        .filter(|s| !s.is_empty())
        .or_else(|| {
            value
                .get("content")
                .and_then(Value::as_str)
                .map(|s| strip_noise(s))
                .filter(|s| !s.is_empty())
        })
}

/// Extract tool_use blocks from message content.
fn extract_tool_uses(value: &Value) -> Vec<ToolUse> {
    let mut tool_uses = Vec::new();

    if let Some(message) = value.get("message") {
        if let Some(content) = message.get("content").and_then(Value::as_array) {
            for block in content {
                if block.get("type").and_then(Value::as_str) == Some("tool_use") {
                    if let (Some(id), Some(name)) = (
                        block.get("id").and_then(Value::as_str),
                        block.get("name").and_then(Value::as_str),
                    ) {
                        tool_uses.push(ToolUse {
                            id: id.to_string(),
                            name: name.to_string(),
                            input: block.get("input").cloned().unwrap_or(Value::Null),
                        });
                    }
                }
            }
        }
    }

    tool_uses
}

/// Extract token usage from message.usage.
fn extract_usage(value: &Value) -> Option<TokenUsage> {
    let usage = value.get("message")?.get("usage")?;

    Some(TokenUsage {
        input_tokens: usage
            .get("input_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        output_tokens: usage
            .get("output_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        cache_read_tokens: usage
            .get("cache_read_input_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        cache_creation_tokens: usage
            .get("cache_creation_input_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0),
    })
}

/// Extract thinking content from message.content blocks.
fn extract_thinking(value: &Value) -> Option<String> {
    if let Some(message) = value.get("message") {
        if let Some(content) = message.get("content").and_then(Value::as_array) {
            let mut thinking_blocks = Vec::new();

            for block in content {
                if block.get("type").and_then(Value::as_str) == Some("thinking") {
                    if let Some(text) = block.get("thinking").and_then(Value::as_str) {
                        if !text.is_empty() {
                            thinking_blocks.push(text.to_string());
                        }
                    }
                }
            }

            if !thinking_blocks.is_empty() {
                return Some(thinking_blocks.join("\n\n"));
            }
        }
    }
    None
}

/// Extract tool_result blocks from user messages.
fn extract_tool_results(value: &Value) -> Vec<ToolResult> {
    let mut results = Vec::new();

    if let Some(message) = value.get("message") {
        if let Some(content) = message.get("content").and_then(Value::as_array) {
            for block in content {
                if block.get("type").and_then(Value::as_str) == Some("tool_result") {
                    if let Some(tool_use_id) = block.get("tool_use_id").and_then(Value::as_str) {
                        let is_error = block
                            .get("is_error")
                            .and_then(Value::as_bool)
                            .unwrap_or(false);

                        // Get a brief summary of the content
                        let content_summary = match block.get("content") {
                            Some(Value::String(s)) => {
                                let cleaned = strip_noise(s);
                                if cleaned.len() > 200 {
                                    Some(format!("{}...", &cleaned[..200]))
                                } else if !cleaned.is_empty() {
                                    Some(cleaned)
                                } else {
                                    None
                                }
                            }
                            Some(Value::Array(arr)) => {
                                // Take first text block as summary
                                arr.iter()
                                    .filter_map(|item| item.get("text").and_then(Value::as_str))
                                    .next()
                                    .map(|s| {
                                        let cleaned = strip_noise(s);
                                        if cleaned.len() > 200 {
                                            format!("{}...", &cleaned[..200])
                                        } else {
                                            cleaned
                                        }
                                    })
                            }
                            _ => None,
                        };

                        results.push(ToolResult {
                            tool_use_id: tool_use_id.to_string(),
                            content_summary,
                            is_error,
                        });
                    }
                }
            }
        }
    }

    results
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_system_init() {
        let json = r#"{"type":"system","subtype":"init","uuid":"abc-123","session_id":"sess-1"}"#;
        let event = ClaudeEvent::parse(json).unwrap();
        assert_eq!(event.event_type, Some(EventType::System));
        assert_eq!(event.subtype.as_deref(), Some("init"));
        assert_eq!(event.uuid.as_deref(), Some("abc-123"));
        assert!(event.is_init());
    }

    #[test]
    fn parse_assistant_with_parent() {
        let json = r#"{"type":"assistant","uuid":"def-456","session_id":"sess-1","parent_tool_use_id":"tool-789"}"#;
        let event = ClaudeEvent::parse(json).unwrap();
        assert!(event.is_assistant());
        assert!(event.is_nested());
        assert_eq!(event.parent_tool_use_id.as_deref(), Some("tool-789"));
    }

    #[test]
    fn parse_user_tool_result() {
        let json = r#"{"type":"user","uuid":"user-123","message":{"role":"user","content":[{"type":"tool_result","content":"file contents here","tool_use_id":"toolu_123"}]}}"#;
        let event = ClaudeEvent::parse(json).unwrap();
        assert_eq!(event.event_type, Some(EventType::User));
        assert_eq!(event.role.as_deref(), Some("user"));
        assert_eq!(event.message.as_deref(), Some("file contents here"));
    }

    #[test]
    fn parse_assistant_text() {
        let json = r#"{"type":"assistant","uuid":"asst-123","message":{"role":"assistant","content":[{"type":"text","text":"Hello world"}]}}"#;
        let event = ClaudeEvent::parse(json).unwrap();
        assert_eq!(event.event_type, Some(EventType::Assistant));
        assert_eq!(event.message.as_deref(), Some("Hello world"));
    }

    #[test]
    fn parse_result() {
        let json = r#"{"type":"result","uuid":"res-1","session_id":"sess-1","total_cost_usd":0.05,"duration_ms":1234,"num_turns":5}"#;
        let event = ClaudeEvent::parse(json).unwrap();
        assert!(event.is_result());
        assert_eq!(event.total_cost_usd, Some(0.05));
        assert_eq!(event.duration_ms, Some(1234));
        assert_eq!(event.num_turns, Some(5));
    }
}
