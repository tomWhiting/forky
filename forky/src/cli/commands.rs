//! CLI command execution.
//!
//! This is a thin client - all database operations go through the server.

use std::path::PathBuf;
use std::process::Command;

use anyhow::{bail, Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::claude::{spawn_claude, ClaudeEvent, ClaudeOptions};
use crate::server;
use crate::session::detect_session_id;

use super::args::{Cli, Commands, ListEntity};

/// Generate a UUIDv7 (time-ordered, globally unique).
fn generate_uuid() -> String {
    Uuid::now_v7().to_string()
}

/// Patterns that indicate a message is likely a forky command being re-executed.
/// This prevents cascade bugs where forked sessions re-run forky commands.
/// NOTE: All patterns must be lowercase since we compare against lowercased input.
const FORKY_COMMAND_PATTERNS: &[&str] = &[
    "spawn ",
    "spawn\t",
    "fork ",
    "fork\t",
    "forky ",
    "forky\t",
    ".forky/bin/forky",  // Matches both /Users/xxx/.forky/bin/forky and ~/.forky/bin/forky
    "fork-me ",
    "fork-me\t",
];

/// Validate that a message doesn't look like a forky command.
/// This prevents cascade bugs where a forked session receives a message like
/// "spawn --model haiku -m hello" and re-executes it, creating infinite sessions.
fn validate_message_not_forky_command(message: &str) -> Result<()> {
    let msg_lower = message.to_lowercase();
    let msg_trimmed = msg_lower.trim();

    // Check for patterns that indicate this is a forky command
    for pattern in FORKY_COMMAND_PATTERNS {
        if msg_trimmed.starts_with(pattern) {
            bail!(
                "CASCADE PREVENTION: Message looks like a forky command: '{}'\n\
                 This would cause infinite session creation.\n\
                 If you meant to send this as a task, wrap it differently.\n\
                 If this is a legitimate message, please rephrase it.",
                &message[..message.len().min(50)]
            );
        }
    }

    // Check for forky binary path anywhere in the message (catches full paths)
    if msg_lower.contains(".forky/bin/forky") || msg_lower.contains(".forky\\bin\\forky") {
        bail!(
            "CASCADE PREVENTION: Message contains forky binary path: '{}'\n\
             This would cause infinite session creation.\n\
             If you meant to send this as a task, wrap it differently.",
            &message[..message.len().min(50)]
        );
    }

    // Also check if message starts with common forky subcommands without the "forky" prefix
    // These could be captured as messages if typed wrong
    let dangerous_starts = ["spawn", "fork", "resume", "new", "fork-me"];
    for cmd in dangerous_starts {
        if msg_trimmed == cmd || msg_trimmed.starts_with(&format!("{cmd} ")) {
            // Check if this looks like a command with flags (has --)
            if message.contains("--") || message.contains(" -m ") || message.contains(" -l") {
                bail!(
                    "CASCADE PREVENTION: Message looks like a forky command: '{}'\n\
                     Did you mean to run: forky {} ?\n\
                     This safeguard prevents infinite session creation.",
                    &message[..message.len().min(50)],
                    message
                );
            }
        }
    }

    Ok(())
}

/// Get the current project path.
fn get_project_path() -> Result<PathBuf> {
    let mut current = std::env::current_dir().context("Failed to get current directory")?;

    loop {
        if current.join(".claude").is_dir() {
            return Ok(current);
        }

        if !current.pop() {
            bail!("Could not find .claude directory. Are you in a Claude Code project?");
        }
    }
}

/// Result of worktree setup.
struct WorktreeInfo {
    path: PathBuf,
    branch: String,
}

/// Set up a git worktree for the fork.
fn setup_worktree(fork_id: &str) -> Result<WorktreeInfo> {
    let output = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .context("Failed to run git rev-parse")?;

    if !output.status.success() {
        bail!(
            "Not in a git repository: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let repo_root = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());

    let worktrees_dir = dirs::home_dir()
        .context("Could not find home directory")?
        .join(".forky")
        .join("worktrees");
    std::fs::create_dir_all(&worktrees_dir)
        .with_context(|| format!("Failed to create {}", worktrees_dir.display()))?;

    let short_id = &fork_id[..8.min(fork_id.len())];
    let branch_name = format!("forky/{short_id}");
    let worktree_path = worktrees_dir.join(short_id);

    if worktree_path.exists() {
        let _ = Command::new("git")
            .current_dir(&repo_root)
            .args(["worktree", "remove", "--force"])
            .arg(&worktree_path)
            .output();
        let _ = Command::new("git")
            .current_dir(&repo_root)
            .args(["branch", "-D", &branch_name])
            .output();
    }

    let output = Command::new("git")
        .current_dir(&repo_root)
        .args(["branch", &branch_name])
        .output()
        .context("Failed to create git branch")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !stderr.contains("already exists") {
            bail!("Failed to create branch {branch_name}: {stderr}");
        }
    }

    let output = Command::new("git")
        .current_dir(&repo_root)
        .args(["worktree", "add"])
        .arg(&worktree_path)
        .arg(&branch_name)
        .output()
        .context("Failed to create git worktree")?;

    if !output.status.success() {
        bail!(
            "Failed to create worktree at {}: {}",
            worktree_path.display(),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    Ok(WorktreeInfo {
        path: worktree_path,
        branch: branch_name,
    })
}

// === HTTP Client for Server Communication ===

/// Fork summary from server.
#[derive(Debug, Deserialize)]
struct ForkSummary {
    project_path: String,
    fork_id: String,
    fork_name: Option<String>,
    session_id: Option<String>,
    parent_session_id: Option<String>,
    status: String,
    event_count: usize,
    created_at: Option<String>,
}

/// Response from creating a fork.
#[derive(Debug, Deserialize)]
struct CreateForkResponse {
    fork_id: String,
    fork_name: String,
    success: bool,
}

/// Create a fork via the server. Returns the generated fork name.
async fn create_fork_on_server(
    port: u16,
    project_path: &str,
    fork_id: &str,
    parent_session_id: Option<&str>,
) -> Result<String> {
    let url = format!("http://127.0.0.1:{port}/api/forks");
    let body = serde_json::json!({
        "project_path": project_path,
        "fork_id": fork_id,
        "parent_session_id": parent_session_id,
    });

    let resp = reqwest::Client::new()
        .post(&url)
        .json(&body)
        .send()
        .await
        .context("Failed to create fork on server")?;

    if !resp.status().is_success() {
        bail!("Server returned {}", resp.status());
    }

    let response: CreateForkResponse = resp.json().await.context("Failed to parse response")?;
    Ok(response.fork_name)
}

/// Update fork status via the server.
async fn update_fork_status_on_server(
    port: u16,
    project_path: &str,
    fork_id: &str,
    status: &str,
    session_id: Option<&str>,
) -> Result<()> {
    let url = format!("http://127.0.0.1:{port}/api/forks/{fork_id}");
    let body = serde_json::json!({
        "project_path": project_path,
        "status": status,
        "session_id": session_id,
    });

    let resp = reqwest::Client::new()
        .patch(&url)
        .json(&body)
        .send()
        .await
        .context("Failed to update fork on server")?;

    if !resp.status().is_success() {
        bail!("Server returned {}", resp.status());
    }

    Ok(())
}

/// Get forks from the server.
async fn get_forks_from_server(port: u16, project_path: Option<&str>) -> Result<Vec<ForkSummary>> {
    let mut url = format!("http://127.0.0.1:{port}/api/forks");
    if let Some(p) = project_path {
        url = format!("{url}?project_path={}", urlencoding::encode(p));
    }

    let resp = reqwest::Client::new()
        .get(&url)
        .send()
        .await
        .context("Failed to get forks from server")?;

    if !resp.status().is_success() {
        bail!("Server returned {}", resp.status());
    }

    let forks: Vec<ForkSummary> = resp.json().await.context("Failed to parse forks")?;
    Ok(forks)
}

/// Send events to the server for storage.
async fn send_events_to_server(
    port: u16,
    project_path: &str,
    events: &[ClaudeEvent],
    fork_id: Option<&str>,
) -> Result<()> {
    if events.is_empty() {
        return Ok(());
    }

    let url = format!("http://127.0.0.1:{port}/api/events");
    let events_json: Vec<_> = events.iter().map(|e| &e.raw).collect();

    let body = serde_json::json!({
        "project_path": project_path,
        "fork_id": fork_id,
        "events": events_json,
    });

    let resp = reqwest::Client::new()
        .post(&url)
        .json(&body)
        .send()
        .await
        .context("Failed to send events to server")?;

    if !resp.status().is_success() {
        bail!("Server returned {}", resp.status());
    }

    Ok(())
}

/// Get events from the server.
#[derive(Debug, Deserialize)]
struct StoredEvent {
    fork_id: Option<String>,
    uuid: Option<String>,
    session_id: Option<String>,
    event_type: String,
    message: Option<String>,
    thinking: Option<String>,
    role: Option<String>,
}

async fn get_events_from_server(
    port: u16,
    project_path: &str,
    fork_id: Option<&str>,
    limit: usize,
) -> Result<Vec<StoredEvent>> {
    let mut url = format!(
        "http://127.0.0.1:{port}/api/events?project_path={}&limit={limit}",
        urlencoding::encode(project_path)
    );
    if let Some(fid) = fork_id {
        url = format!("{url}&fork_id={fid}");
    }

    let resp = reqwest::Client::new()
        .get(&url)
        .send()
        .await
        .context("Failed to get events from server")?;

    if !resp.status().is_success() {
        bail!("Server returned {}", resp.status());
    }

    let events: Vec<StoredEvent> = resp.json().await.context("Failed to parse events")?;
    Ok(events)
}

// === CLI Options ===

#[derive(Debug, Clone, Default)]
pub struct ForkOptions {
    pub model: Option<String>,
    pub worktree: bool,
    pub dir: Option<String>,
    pub chrome: bool,
    pub no_chrome: bool,
    pub append_system_prompt: Option<String>,
    pub system_prompt: Option<String>,
    pub agents: Option<String>,
    pub mcp_config: Option<String>,
    pub settings: Option<String>,
    pub max_turns: Option<u32>,
    pub tools: Option<String>,
    pub allowed_tools: Option<String>,
    pub include_partial_messages: bool,
}

impl From<&Cli> for ForkOptions {
    fn from(cli: &Cli) -> Self {
        Self {
            model: Some(cli.model.clone()),
            worktree: cli.worktree,
            dir: cli.dir.clone(),
            chrome: cli.chrome,
            no_chrome: cli.no_chrome,
            append_system_prompt: cli.append_system_prompt.clone(),
            system_prompt: cli.system_prompt.clone(),
            agents: cli.agents.clone(),
            mcp_config: cli.mcp_config.clone(),
            settings: cli.settings.clone(),
            max_turns: cli.max_turns,
            tools: cli.tools.clone(),
            allowed_tools: cli.allowed_tools.clone(),
            include_partial_messages: cli.include_partial_messages,
        }
    }
}

// === Command Execution ===

pub async fn execute(cli: Cli) -> Result<()> {
    let opts = ForkOptions::from(&cli);

    // Handle -l flag (message last fork)
    if cli.message_last {
        let message = cli.message.join(" ");
        if message.is_empty() {
            bail!("Message is required when using -l flag");
        }
        return message_last_fork(&message, &opts).await;
    }

    match cli.command {
        Some(Commands::Spawn { message }) => {
            let message = message.join(" ");
            if message.is_empty() {
                bail!("Message is required for spawn command");
            }
            validate_message_not_forky_command(&message)?;
            fork_current_session(&message, &opts).await
        }
        Some(Commands::ForkMe { message }) => {
            let message = message.join(" ");
            if message.is_empty() {
                bail!("Message is required for fork-me command");
            }
            validate_message_not_forky_command(&message)?;
            fork_current_session(&message, &opts).await
        }
        Some(Commands::Fork { id, message }) => {
            let message = message.join(" ");
            if message.is_empty() {
                bail!("Message is required for fork command");
            }
            validate_message_not_forky_command(&message)?;
            fork_specific_session(&id, &message, &opts).await
        }
        Some(Commands::Resume { id, message }) => {
            let message = message.join(" ");
            if message.is_empty() {
                bail!("Message is required for resume command");
            }
            validate_message_not_forky_command(&message)?;
            resume_session(&id, &message, &opts).await
        }
        Some(Commands::List { entity }) => list_entities(entity).await,
        Some(Commands::Messages { fork_id }) => list_messages(&fork_id).await,
        Some(Commands::Read { id, all }) => {
            if all {
                println!("Mark all read not yet implemented via server");
                Ok(())
            } else if let Some(id) = id {
                println!("Mark {id} read not yet implemented via server");
                Ok(())
            } else {
                bail!("Either --all or an ID is required for read command");
            }
        }
        Some(Commands::New { message }) => {
            let message = message.join(" ");
            if message.is_empty() {
                bail!("Message is required for new command");
            }
            validate_message_not_forky_command(&message)?;
            start_new_session(&message, &opts).await
        }
        Some(Commands::Done { fork_id, summary }) => {
            let summary = summary.join(" ");
            fork_done(&fork_id, &summary).await
        }
        Some(Commands::Serve { port, open }) => serve_ui(port, open).await,
        Some(Commands::Events { session, limit }) => list_events(session.as_deref(), limit).await,
        None => {
            let message = cli.message.join(" ");
            if message.is_empty() {
                println!("Forky - Fork Claude sessions to handle side tasks in parallel");
                println!();
                println!("Usage: forky [OPTIONS] [MESSAGE]...");
                println!("       forky <COMMAND>");
                println!();
                println!("Commands:");
                println!("  spawn          Spawn a new forked Claude session (recommended)");
                println!("  fork-me        Fork the current session");
                println!("  fork <ID>      Fork a specific session");
                println!("  resume <ID>    Resume a specific session");
                println!("  list <TYPE>    List forks, sessions, or jobs");
                println!("  messages <ID>  View messages for a fork");
                println!("  new            Start a fresh Claude session");
                println!("  serve          Start the observability UI server");
                println!();
                println!("Options:");
                println!("  -l, --last       Message the last fork");
                println!("  -m, --model      Model to use for Claude");
                println!("  --worktree       Run in a git worktree");
                println!("  --dir <PATH>     Directory to run in");
                println!("  -h, --help       Print help");
                return Ok(());
            }
            validate_message_not_forky_command(&message)?;
            fork_current_session(&message, &opts).await
        }
    }
}

async fn fork_current_session(message: &str, opts: &ForkOptions) -> Result<()> {
    let parent_session_id = detect_session_id()?;

    if parent_session_id.is_none() {
        println!("Warning: Could not detect current session ID. Starting fresh session.");
    }

    run_fork(parent_session_id.as_deref(), message, opts, true).await
}

async fn fork_specific_session(session_id: &str, message: &str, opts: &ForkOptions) -> Result<()> {
    run_fork(Some(session_id), message, opts, true).await
}

async fn message_last_fork(message: &str, opts: &ForkOptions) -> Result<()> {
    let port = server::ensure_server_running()?;
    let project_path = get_project_path()?;
    let project_str = project_path.to_string_lossy();

    let forks = get_forks_from_server(port, Some(&project_str)).await?;
    let fork = forks.first().context("No forks found")?;

    let session_id = fork
        .session_id
        .as_deref()
        .or(fork.parent_session_id.as_deref())
        .context("Fork has no session ID")?;

    run_fork(Some(session_id), message, opts, false).await
}

async fn resume_session(session_id: &str, message: &str, opts: &ForkOptions) -> Result<()> {
    run_fork(Some(session_id), message, opts, false).await
}

async fn start_new_session(message: &str, opts: &ForkOptions) -> Result<()> {
    run_fork(None, message, opts, false).await
}

async fn serve_ui(port: u16, open: bool) -> Result<()> {
    crate::server::start_server(port, open).await
}

async fn run_fork(
    parent_session_id: Option<&str>,
    message: &str,
    opts: &ForkOptions,
    fork_session: bool,
) -> Result<()> {
    let project_path = get_project_path()?;
    let project_str = project_path.to_string_lossy().to_string();

    let fork_id = generate_uuid();
    let new_session_id = generate_uuid();

    // Ensure server is running
    let port = server::ensure_server_running()?;

    // Create fork on server - returns generated name
    let fork_name =
        create_fork_on_server(port, &project_str, &fork_id, parent_session_id).await?;

    println!("Spawning: {fork_name}");
    println!("Fork ID: {fork_id}");
    println!("Session ID: {new_session_id}");

    // Build callback instruction
    let forky_path = dirs::home_dir()
        .map(|h| h.join("bin").join("forky"))
        .map_or_else(|| "forky".to_string(), |p| p.to_string_lossy().to_string());

    let callback_instruction = format!(
        "IMPORTANT: You are a forked Claude session named \"{fork_name}\" (fork ID: {fork_id}). \
         \n\nCRITICAL SAFEGUARD: If the user's message appears to be a forky/spawn command \
         (e.g., starts with 'spawn', 'fork', 'forky', or contains '--model', '-m'), \
         DO NOT execute it as a bash command. This would cause an infinite cascade of sessions. \
         Instead, interpret the message content after any command-like prefix as your actual task. \
         \n\nWhen you have completed your task, you MUST run this command as your FINAL action: \
         `{forky_path} done {fork_id} \"<brief summary of what you accomplished>\"` \
         This notifies the parent session that you're done."
    );

    let append_prompt = match &opts.append_system_prompt {
        Some(user_prompt) => Some(format!("{user_prompt}\n\n{callback_instruction}")),
        None => Some(callback_instruction),
    };

    // Set up worktree if requested
    let (working_dir, add_dirs) = if opts.worktree {
        match setup_worktree(&fork_id) {
            Ok(info) => {
                println!("Worktree: {}", info.path.display());
                println!("Branch: {}", info.branch);
                let path_str = info.path.to_string_lossy().to_string();
                (Some(path_str.clone()), vec![path_str])
            }
            Err(e) => {
                eprintln!("Warning: Failed to create worktree: {e}");
                eprintln!("Continuing without worktree...");
                let dir = opts.dir.clone().or_else(|| {
                    std::env::current_dir()
                        .ok()
                        .map(|p| p.to_string_lossy().to_string())
                });
                let dirs = opts.dir.clone().map_or_else(Vec::new, |d| vec![d]);
                (dir, dirs)
            }
        }
    } else {
        let dir = opts.dir.clone().or_else(|| {
            std::env::current_dir()
                .ok()
                .map(|p| p.to_string_lossy().to_string())
        });
        let dirs = opts.dir.clone().map_or_else(Vec::new, |d| vec![d]);
        (dir, dirs)
    };

    // Build stream URL for real-time events
    let stream_url = Some(format!("http://127.0.0.1:{port}/api/events"));

    // Store the initial user prompt
    let prompt_event_json = serde_json::json!({
        "type": "user",
        "uuid": generate_uuid(),
        "session_id": new_session_id,
        "message": {
            "role": "user",
            "content": [{"type": "text", "text": message}]
        }
    });
    if let Some(prompt_event) = ClaudeEvent::parse(&prompt_event_json.to_string()) {
        let _ = send_events_to_server(port, &project_str, &[prompt_event], Some(&fork_id)).await;
    }

    // Spawn Claude
    let claude_opts = ClaudeOptions {
        session_id: parent_session_id.map(String::from),
        explicit_session_id: Some(new_session_id.clone()),
        fork_session,
        model: opts.model.clone(),
        message: message.to_string(),
        working_dir,
        add_dirs,
        append_system_prompt: append_prompt,
        system_prompt: opts.system_prompt.clone(),
        chrome: opts.chrome,
        no_chrome: opts.no_chrome,
        agents: opts.agents.clone(),
        mcp_config: opts.mcp_config.clone(),
        settings: opts.settings.clone(),
        max_turns: opts.max_turns,
        tools: opts.tools.clone(),
        allowed_tools: opts.allowed_tools.clone(),
        include_partial_messages: opts.include_partial_messages,
        stream_url: stream_url.clone(),
        fork_id: Some(fork_id.clone()),
        project_path: Some(project_str.clone()),
    };

    let result = spawn_claude(claude_opts).await?;

    // Update fork status
    let status = if result.success { "completed" } else { "failed" };
    let session_id = result.session_id.as_ref().unwrap_or(&new_session_id);

    let _ = update_fork_status_on_server(port, &project_str, &fork_id, status, Some(session_id))
        .await;

    // Print result
    if result.success {
        println!("\nFork completed successfully.");
        if let Some(cost) = result.cost_usd {
            println!("Cost: ${cost:.4}");
        }
    } else {
        println!("\nFork failed.");
    }

    if !result.messages.is_empty() {
        let response = result.messages.join("");
        println!("\nResponse:\n{response}");
    } else if let Some(ref response) = result.result {
        println!("\nResponse:\n{response}");
    }

    Ok(())
}

async fn list_entities(entity: ListEntity) -> Result<()> {
    let port = server::ensure_server_running()?;
    let project_path = get_project_path()?;
    let project_str = project_path.to_string_lossy();

    match entity {
        ListEntity::Forks => {
            let forks = get_forks_from_server(port, Some(&project_str)).await?;
            if forks.is_empty() {
                println!("No forks found.");
                return Ok(());
            }

            println!(
                "{:<10} {:<28} {:<12} {:<8}",
                "ID", "NAME", "STATUS", "EVENTS"
            );
            println!("{}", "-".repeat(60));

            for fork in forks {
                let name = fork.fork_name.as_deref().unwrap_or("-");
                let name_short = if name.len() > 26 { &name[..26] } else { name };
                println!(
                    "{:<10} {:<28} {:<12} {:<8}",
                    &fork.fork_id[..8.min(fork.fork_id.len())],
                    name_short,
                    fork.status,
                    fork.event_count,
                );
            }
        }
        ListEntity::Sessions => {
            println!("Session listing via server not yet implemented.");
            println!("Use 'forky list forks' to see forks with their session IDs.");
        }
        ListEntity::Jobs => {
            println!("Job listing via server not yet implemented.");
            println!("Use 'forky list forks' to see forks.");
        }
    }
    Ok(())
}

async fn list_messages(fork_id: &str) -> Result<()> {
    let port = server::ensure_server_running()?;
    let project_path = get_project_path()?;
    let project_str = project_path.to_string_lossy();

    let events = get_events_from_server(port, &project_str, Some(fork_id), 100).await?;

    if events.is_empty() {
        println!("No messages found for fork {fork_id}.");
        return Ok(());
    }

    for event in events {
        let role = event.role.as_deref().unwrap_or(&event.event_type);
        let role_display = role.to_uppercase();

        if let Some(ref msg) = event.message {
            println!("[{role_display}]:");
            println!("{msg}");
            println!();
        }

        if let Some(ref thinking) = event.thinking {
            println!("[{role_display} THINKING]:");
            let preview = if thinking.len() > 200 {
                format!("{}...", &thinking[..200])
            } else {
                thinking.clone()
            };
            println!("{preview}");
            println!();
        }
    }

    Ok(())
}

async fn fork_done(fork_id: &str, summary: &str) -> Result<()> {
    use std::fs::OpenOptions;
    use std::io::Write;

    // Try to update fork status via server
    if let Ok(port) = server::get_server_port().ok_or(()).map_err(|_| ()) {
        if let Ok(project_path) = get_project_path() {
            let project_str = project_path.to_string_lossy();
            let _ =
                update_fork_status_on_server(port, &project_str, fork_id, "completed", None).await;
        }
    }

    // Write to notifications file
    let notif_dir = dirs::home_dir()
        .context("Could not find home directory")?
        .join(".forky")
        .join("notifications");
    std::fs::create_dir_all(&notif_dir)?;

    let notif_file = notif_dir.join("pending.txt");
    let notification = format!(
        "{}|{}|{}\n",
        fork_id,
        Utc::now().format("%Y-%m-%d %H:%M:%S"),
        if summary.is_empty() {
            "Fork completed"
        } else {
            summary
        }
    );

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&notif_file)?;
    file.write_all(notification.as_bytes())?;

    println!("✓ Fork {fork_id} done");
    if !summary.is_empty() {
        println!("  Summary: {summary}");
    }

    Ok(())
}

async fn list_events(session_filter: Option<&str>, limit: usize) -> Result<()> {
    let port = server::ensure_server_running()?;
    let project_path = get_project_path()?;
    let project_str = project_path.to_string_lossy();

    let events = get_events_from_server(port, &project_str, None, limit).await?;

    if events.is_empty() {
        println!("No events found.");
        return Ok(());
    }

    println!("Found {} events:\n", events.len());
    println!(
        "{:<8} {:<12} {:<10} {}",
        "UUID", "TYPE", "ROLE", "MESSAGE"
    );
    println!("{}", "-".repeat(70));

    for event in events {
        // Apply session filter if provided
        if let Some(filter) = session_filter {
            if let Some(ref sid) = event.session_id {
                if !sid.starts_with(filter) {
                    continue;
                }
            } else {
                continue;
            }
        }

        let uuid = event.uuid.as_deref().unwrap_or("-");
        let uuid_short = if uuid.len() > 8 { &uuid[..8] } else { uuid };

        let role = event.role.as_deref().unwrap_or("-");
        let msg = event.message.as_deref().unwrap_or("-");
        let msg_short = if msg.len() > 35 {
            format!("{}...", &msg[..32])
        } else {
            msg.to_string()
        };

        println!(
            "{:<8} {:<12} {:<10} {}",
            uuid_short, event.event_type, role, msg_short
        );
    }

    Ok(())
}
