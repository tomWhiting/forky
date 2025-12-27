//! Claude CLI process spawning.

use anyhow::{Context, Result};
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

use super::events::ClaudeEvent;

/// Options for spawning Claude.
#[derive(Debug, Clone, Default)]
pub struct ClaudeOptions {
    /// Session ID to resume (if any).
    pub session_id: Option<String>,
    /// Explicit session ID to use (`UUIDv7`, set upfront).
    pub explicit_session_id: Option<String>,
    /// Whether to fork the session.
    pub fork_session: bool,
    /// Model to use (if any).
    pub model: Option<String>,
    /// Message to send.
    pub message: String,
    /// Working directory.
    pub working_dir: Option<String>,
    /// Additional directories to add.
    pub add_dirs: Vec<String>,

    // === System Prompt Options ===
    /// Text to append to system prompt (if any).
    pub append_system_prompt: Option<String>,
    /// Replace the entire system prompt.
    pub system_prompt: Option<String>,

    // === Chrome Options ===
    /// Enable Chrome browser integration.
    pub chrome: bool,
    /// Disable Chrome browser integration.
    pub no_chrome: bool,

    // === Advanced Options ===
    /// Custom subagents as JSON.
    pub agents: Option<String>,
    /// MCP server configuration as JSON or path.
    pub mcp_config: Option<String>,
    /// Additional settings as JSON or path.
    pub settings: Option<String>,
    /// Maximum agentic turns.
    pub max_turns: Option<u32>,
    /// Restrict available tools.
    pub tools: Option<String>,
    /// Tools that don't require permission prompts.
    pub allowed_tools: Option<String>,
    /// Include partial streaming messages.
    pub include_partial_messages: bool,
}

/// Result from a Claude session.
#[derive(Debug)]
pub struct ClaudeResult {
    /// The session ID (may be new if forked).
    pub session_id: Option<String>,
    /// All assistant messages collected.
    pub messages: Vec<String>,
    /// Final result text.
    pub result: Option<String>,
    /// Whether the session completed successfully.
    pub success: bool,
    /// Total cost in USD.
    pub cost_usd: Option<f64>,
}

/// Spawn a Claude CLI process and stream events.
///
/// This runs the claude CLI with the following arguments:
/// `claude --dangerously-skip-permissions --output-format stream-json --verbose [options] -p <message>`
pub async fn spawn_claude(options: ClaudeOptions) -> Result<ClaudeResult> {
    let mut cmd = Command::new("claude");

    // Always use these flags
    cmd.arg("--dangerously-skip-permissions");
    cmd.arg("--output-format").arg("stream-json");
    cmd.arg("--verbose");

    // Explicit session ID (UUIDv7, set upfront)
    if let Some(ref session_id) = options.explicit_session_id {
        cmd.arg("--session-id").arg(session_id);
    }

    // Resume session if provided
    if let Some(ref session_id) = options.session_id {
        cmd.arg("-r").arg(session_id);
    }

    // Fork session if requested
    if options.fork_session {
        cmd.arg("--fork-session");
    }

    // Model if specified
    if let Some(ref model) = options.model {
        cmd.arg("--model").arg(model);
    }

    // === System Prompt Options ===
    if let Some(ref prompt) = options.system_prompt {
        cmd.arg("--system-prompt").arg(prompt);
    } else if let Some(ref prompt) = options.append_system_prompt {
        cmd.arg("--append-system-prompt").arg(prompt);
    }

    // === Chrome Options ===
    if options.chrome {
        cmd.arg("--chrome");
    } else if options.no_chrome {
        cmd.arg("--no-chrome");
    }

    // === Additional Directories ===
    for dir in &options.add_dirs {
        cmd.arg("--add-dir").arg(dir);
    }

    // === Advanced Options ===
    if let Some(ref agents) = options.agents {
        cmd.arg("--agents").arg(agents);
    }

    if let Some(ref mcp_config) = options.mcp_config {
        cmd.arg("--mcp-config").arg(mcp_config);
    }

    if let Some(ref settings) = options.settings {
        cmd.arg("--settings").arg(settings);
    }

    if let Some(max_turns) = options.max_turns {
        cmd.arg("--max-turns").arg(max_turns.to_string());
    }

    if let Some(ref tools) = options.tools {
        cmd.arg("--tools").arg(tools);
    }

    if let Some(ref allowed_tools) = options.allowed_tools {
        cmd.arg("--allowedTools").arg(allowed_tools);
    }

    if options.include_partial_messages {
        cmd.arg("--include-partial-messages");
    }

    // Working directory
    if let Some(ref dir) = options.working_dir {
        cmd.current_dir(dir);
    }

    // Message as print mode
    cmd.arg("-p").arg(&options.message);

    // Set up stdio
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    cmd.stdin(Stdio::null());

    // Spawn the process
    let mut child = cmd.spawn().context("Failed to spawn claude CLI")?;

    let stdout = child.stdout.take().context("Failed to capture stdout")?;
    let stderr = child.stderr.take().context("Failed to capture stderr")?;

    let mut stdout_reader = BufReader::new(stdout).lines();
    let mut stderr_reader = BufReader::new(stderr).lines();

    let mut result = ClaudeResult {
        session_id: None,
        messages: Vec::new(),
        result: None,
        success: false,
        cost_usd: None,
    };

    // Process stdout (NDJSON events)
    loop {
        tokio::select! {
            line = stdout_reader.next_line() => {
                match line {
                    Ok(Some(line)) => {
                        if let Some(event) = ClaudeEvent::parse(&line) {
                            // Capture session ID
                            if event.session_id.is_some() && result.session_id.is_none() {
                                result.session_id.clone_from(&event.session_id);
                            }

                            // Capture assistant messages
                            if event.is_assistant() {
                                if let Some(text) = event.get_text() {
                                    result.messages.push(text.to_string());
                                }
                            }

                            // Capture result
                            if event.is_result() {
                                result.success = true;
                                if let Some(text) = event.get_text() {
                                    result.result = Some(text.to_string());
                                }
                                if event.cost_usd.is_some() {
                                    result.cost_usd = event.cost_usd;
                                }
                            }
                        }
                    }
                    Ok(None) => break,
                    Err(e) => {
                        eprintln!("Error reading stdout: {e}");
                        break;
                    }
                }
            }
            line = stderr_reader.next_line() => {
                match line {
                    Ok(Some(line)) => {
                        // Log stderr but don't fail
                        eprintln!("[claude stderr] {line}");
                    }
                    Ok(None) => {}
                    Err(e) => {
                        eprintln!("Error reading stderr: {e}");
                    }
                }
            }
        }
    }

    // Wait for the process to finish
    let status = child
        .wait()
        .await
        .context("Failed to wait for claude CLI")?;
    result.success = status.success() && result.success;

    Ok(result)
}
