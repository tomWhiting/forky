//! Session ID detection from various sources.

use anyhow::Result;
use serde::Deserialize;
use std::path::{Path, PathBuf};

const FORKY_SESSION_FILE: &str = "/tmp/.forky-session";
const CLAUDE_SESSION_FILE: &str = ".claude/current-session.json";

/// Structure of the Claude current-session.json file.
#[derive(Debug, Deserialize)]
struct ClaudeSessionFile {
    #[serde(rename = "sessionId")]
    session_id: Option<String>,
}

/// Detect the current Claude session ID.
///
/// Priority order:
/// 1. Read from `/tmp/.forky-session` (hook-injected)
/// 2. Walk up directories looking for `.claude/current-session.json`
pub fn detect_session_id() -> Result<Option<String>> {
    // Priority 1: Check /tmp/.forky-session
    if let Some(session_id) = read_forky_session_file()? {
        return Ok(Some(session_id));
    }

    // Priority 2: Walk up directories looking for .claude/current-session.json
    if let Some(session_id) = find_claude_session_file()? {
        return Ok(Some(session_id));
    }

    Ok(None)
}

/// Read session ID from /tmp/.forky-session.
fn read_forky_session_file() -> Result<Option<String>> {
    let path = Path::new(FORKY_SESSION_FILE);
    if !path.exists() {
        return Ok(None);
    }

    let content = std::fs::read_to_string(path)?;
    let session_id = content.trim().to_string();

    if session_id.is_empty() {
        return Ok(None);
    }

    Ok(Some(session_id))
}

/// Walk up directories looking for .claude/current-session.json.
fn find_claude_session_file() -> Result<Option<String>> {
    let cwd = std::env::current_dir()?;
    let mut current = cwd.as_path();

    loop {
        let session_path = current.join(CLAUDE_SESSION_FILE);
        if session_path.exists() {
            if let Some(session_id) = read_claude_session_file(&session_path)? {
                return Ok(Some(session_id));
            }
        }

        match current.parent() {
            Some(parent) => current = parent,
            None => break,
        }
    }

    // Also check home directory
    if let Some(home) = dirs::home_dir() {
        let session_path = home.join(CLAUDE_SESSION_FILE);
        if session_path.exists() {
            if let Some(session_id) = read_claude_session_file(&session_path)? {
                return Ok(Some(session_id));
            }
        }
    }

    Ok(None)
}

/// Read session ID from a Claude session file.
fn read_claude_session_file(path: &PathBuf) -> Result<Option<String>> {
    let content = std::fs::read_to_string(path)?;
    let session_file: ClaudeSessionFile = serde_json::from_str(&content)?;
    Ok(session_file.session_id)
}
