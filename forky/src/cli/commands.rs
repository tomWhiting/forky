//! CLI command execution.

use anyhow::{bail, Context, Result};
use chrono::Utc;
use uuid::Uuid;

use crate::claude::{spawn_claude, ClaudeOptions};
use crate::db::{Database, ForkQueries, JobQueries, MessageQueries, SessionQueries};
use crate::models::{Fork, ForkStatus, Job, JobStatus, Message, MessageRole, Session};
use crate::session::detect_session_id;

use super::args::{Cli, Commands, ListEntity};

/// Generate a short random ID for forks/jobs.
fn generate_short_id() -> String {
    use rand::Rng;
    let mut rng = rand::rng();
    let chars: Vec<char> = "abcdefghijklmnopqrstuvwxyz0123456789".chars().collect();
    (0..8)
        .map(|_| chars[rng.random_range(0..chars.len())])
        .collect()
}

/// Generate a `UUIDv7` for session IDs (time-ordered, visually distinct).
fn generate_session_id() -> String {
    Uuid::now_v7().to_string()
}

/// CLI options that get passed through to Claude.
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
            model: cli.model.clone(),
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

/// Execute the CLI command.
pub async fn execute(cli: Cli) -> Result<()> {
    let db = Database::open().context("Failed to open database")?;
    let opts = ForkOptions::from(&cli);

    // Handle -l flag (message last fork)
    if cli.message_last {
        let message = cli.message.join(" ");
        if message.is_empty() {
            bail!("Message is required when using -l flag");
        }
        return message_last_fork(&db, &message, &opts).await;
    }

    // Handle subcommands
    match cli.command {
        Some(Commands::ForkMe { message }) => {
            let message = message.join(" ");
            if message.is_empty() {
                bail!("Message is required for fork-me command");
            }
            fork_current_session(&db, &message, &opts).await
        }
        Some(Commands::Fork { id, message }) => {
            let message = message.join(" ");
            if message.is_empty() {
                bail!("Message is required for fork command");
            }
            fork_specific_session(&db, &id, &message, &opts).await
        }
        Some(Commands::Resume { id, message }) => {
            let message = message.join(" ");
            if message.is_empty() {
                bail!("Message is required for resume command");
            }
            resume_session(&db, &id, &message, &opts).await
        }
        Some(Commands::List { entity }) => list_entities(&db, entity),
        Some(Commands::Messages { fork_id }) => list_messages(&db, &fork_id),
        Some(Commands::Read { id, all }) => {
            if all {
                mark_all_read(&db)
            } else if let Some(id) = id {
                mark_read(&db, &id)
            } else {
                bail!("Either --all or an ID is required for read command");
            }
        }
        Some(Commands::New { message }) => {
            let message = message.join(" ");
            if message.is_empty() {
                bail!("Message is required for new command");
            }
            start_new_session(&db, &message, &opts).await
        }
        Some(Commands::Done { fork_id, summary }) => {
            let summary = summary.join(" ");
            fork_done(&db, &fork_id, &summary)
        }
        Some(Commands::Serve { port, open }) => {
            serve_ui(port, open).await
        }
        None => {
            // Default behavior: fork current session with message
            let message = cli.message.join(" ");
            if message.is_empty() {
                // No message provided, show help
                println!("Forky - Fork Claude sessions to handle side tasks in parallel");
                println!();
                println!("Usage: forky [OPTIONS] [MESSAGE]...");
                println!("       forky <COMMAND>");
                println!();
                println!("Commands:");
                println!("  fork-me        Fork the current session");
                println!("  fork <ID>      Fork a specific session");
                println!("  resume <ID>    Resume a specific session");
                println!("  list <TYPE>    List forks, sessions, or jobs");
                println!("  messages <ID>  View messages for a fork");
                println!("  read <ID>      Mark a fork as read");
                println!("  new            Start a fresh Claude session");
                println!("  serve          Start the observability UI server");
                println!();
                println!("Options:");
                println!("  -l, --last       Message the last fork");
                println!("  -m, --model      Model to use for Claude");
                println!("  --worktree       Run in a git worktree");
                println!("  --dir <PATH>     Directory to run in");
                println!("  --chrome         Enable Chrome integration");
                println!("  --no-chrome      Disable Chrome integration");
                println!("  -h, --help       Print help");
                println!("  -V, --version    Print version");
                return Ok(());
            }
            fork_current_session(&db, &message, &opts).await
        }
    }
}

/// Fork the current session.
async fn fork_current_session(db: &Database, message: &str, opts: &ForkOptions) -> Result<()> {
    let parent_session_id = detect_session_id()?;

    if parent_session_id.is_none() {
        println!("Warning: Could not detect current session ID. Starting fresh session.");
    }

    run_fork(db, parent_session_id.as_deref(), message, opts, true).await
}

/// Fork a specific session.
async fn fork_specific_session(
    db: &Database,
    session_id: &str,
    message: &str,
    opts: &ForkOptions,
) -> Result<()> {
    run_fork(db, Some(session_id), message, opts, true).await
}

/// Message the last fork.
async fn message_last_fork(db: &Database, message: &str, opts: &ForkOptions) -> Result<()> {
    let fork = ForkQueries::get_latest(db.conn())?.context("No forks found")?;

    let session_id = fork
        .fork_session_id
        .as_deref()
        .or(fork.parent_session_id.as_deref())
        .context("Fork has no session ID")?;

    run_fork(db, Some(session_id), message, opts, false).await
}

/// Resume a specific session (no forking).
async fn resume_session(
    db: &Database,
    session_id: &str,
    message: &str,
    opts: &ForkOptions,
) -> Result<()> {
    run_fork(db, Some(session_id), message, opts, false).await
}

/// Start a fresh session (no parent, no forking).
async fn start_new_session(db: &Database, message: &str, opts: &ForkOptions) -> Result<()> {
    run_fork(db, None, message, opts, false).await
}

/// Start the observability UI server.
#[allow(clippy::unused_async)] // Will use async when WebSocket is implemented
async fn serve_ui(port: u16, open: bool) -> Result<()> {
    // TODO: Implement WebSocket server
    println!("Starting Forky UI server on port {port}...");
    if open {
        println!("Opening browser...");
        // TODO: Open browser
    }
    println!("(Server not yet implemented)");
    Ok(())
}

/// Run a fork/session with the given parameters.
async fn run_fork(
    db: &Database,
    parent_session_id: Option<&str>,
    message: &str,
    opts: &ForkOptions,
    fork_session: bool,
) -> Result<()> {
    let fork_id = generate_short_id();
    let job_id = generate_short_id();
    // Generate UUIDv7 session ID upfront (time-ordered, visually distinct)
    let new_session_id = generate_session_id();

    // Create fork record
    let fork = Fork::new(fork_id.clone(), parent_session_id.map(String::from));
    ForkQueries::insert(db.conn(), &fork)?;

    // Create job record
    let job = Job::new(job_id.clone(), message.to_string(), fork_id.clone());
    JobQueries::insert(db.conn(), &job)?;

    // Store user message
    let user_msg = Message::new(
        fork_id.clone(),
        MessageRole::User,
        message.to_string(),
    );
    MessageQueries::insert(db.conn(), &user_msg)?;

    println!("Fork ID: {fork_id}");
    println!("Session ID: {new_session_id}");
    println!("Starting Claude session...");

    // Build callback instruction for the fork
    // Use ~/.forky/bin/forky as the canonical path (we'll symlink it there)
    let forky_path = dirs::home_dir()
        .map(|h| h.join(".forky").join("bin").join("forky")).map_or_else(|| "forky".to_string(), |p| p.to_string_lossy().to_string());

    let callback_instruction = format!(
        "IMPORTANT: You are a forked Claude session (fork ID: {fork_id}). \
         When you have completed your task, you MUST run this command as your FINAL action: \
         `{forky_path} done {fork_id} \"<brief summary of what you accomplished>\"` \
         This notifies the parent session that you're done."
    );

    // Combine user's append_system_prompt with our callback instruction
    let append_prompt = match &opts.append_system_prompt {
        Some(user_prompt) => Some(format!("{user_prompt}\n\n{callback_instruction}")),
        None => Some(callback_instruction),
    };

    // Determine working directory
    let working_dir = opts.dir.clone().or_else(|| {
        std::env::current_dir()
            .ok()
            .map(|p| p.to_string_lossy().to_string())
    });

    // Build add_dirs list
    let mut add_dirs = Vec::new();
    if let Some(ref dir) = opts.dir {
        add_dirs.push(dir.clone());
    }

    // TODO: Handle worktree creation here
    if opts.worktree {
        println!("(Worktree support not yet implemented)");
    }

    // Spawn Claude with all options
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
    };

    let result = spawn_claude(claude_opts).await?;

    // We know the session ID upfront (UUIDv7), so use it directly
    // The result.session_id might differ if Claude does something unexpected,
    // but our explicit_session_id should be respected
    let session_id = result.session_id.as_ref().unwrap_or(&new_session_id);

    ForkQueries::update_session_id(db.conn(), &fork_id, session_id)?;

    // Create session record first (before updating job, due to FK constraint)
    let session = Session::new(session_id.clone(), Some(fork_id.clone()));
    // Ignore error if session already exists
    let _ = SessionQueries::insert(db.conn(), &session);

    // Now safe to update job's session_id
    JobQueries::update_session_id(db.conn(), &job_id, session_id)?;

    // Store assistant response - prefer accumulated messages over result text
    let assistant_response = if result.messages.is_empty() {
        result.result.clone()
    } else {
        Some(result.messages.join(""))
    };

    if let Some(ref response) = assistant_response {
        let assistant_msg = Message::new(
            fork_id.clone(),
            MessageRole::Assistant,
            response.clone(),
        );
        MessageQueries::insert(db.conn(), &assistant_msg)?;
    }

    // Update job status
    let (job_status, fork_status) = if result.success {
        (JobStatus::Completed, ForkStatus::Completed)
    } else {
        (JobStatus::Failed, ForkStatus::Failed)
    };

    let completed_at = Some(Utc::now());
    JobQueries::update_status(
        db.conn(),
        &job_id,
        job_status,
        result.result.as_deref(),
        completed_at,
    )?;
    ForkQueries::update_status(db.conn(), &fork_id, fork_status, completed_at)?;

    // Print result
    if result.success {
        println!("\nFork completed successfully.");
        if let Some(cost) = result.cost_usd {
            println!("Cost: ${cost:.4}");
        }
    } else {
        println!("\nFork failed.");
    }

    if let Some(ref response) = assistant_response {
        println!("\nResponse:\n{response}");
    }

    Ok(())
}

/// List entities (forks, sessions, or jobs).
fn list_entities(db: &Database, entity: ListEntity) -> Result<()> {
    match entity {
        ListEntity::Forks => {
            let forks = ForkQueries::list(db.conn(), None)?;
            if forks.is_empty() {
                println!("No forks found.");
                return Ok(());
            }

            println!(
                "{:<10} {:<12} {:<10} {:<6} {:<20}",
                "ID", "STATUS", "READ", "MSGS", "CREATED"
            );
            println!("{}", "-".repeat(60));

            for fork in forks {
                let read_status = if fork.read { "yes" } else { "no" };
                let created = fork.created_at.format("%Y-%m-%d %H:%M");
                println!(
                    "{:<10} {:<12} {:<10} {:<6} {:<20}",
                    fork.id, fork.status, read_status, "-", created
                );
            }
        }
        ListEntity::Sessions => {
            let sessions = SessionQueries::list(db.conn())?;
            if sessions.is_empty() {
                println!("No sessions found.");
                return Ok(());
            }

            println!("{:<40} {:<10} {:<20}", "ID", "FORK", "CREATED");
            println!("{}", "-".repeat(72));

            for session in sessions {
                let fork_id = session.fork_id.as_deref().unwrap_or("-");
                let created = session.created_at.format("%Y-%m-%d %H:%M");
                println!("{:<40} {:<10} {:<20}", session.id, fork_id, created);
            }
        }
        ListEntity::Jobs => {
            let jobs = JobQueries::list(db.conn(), None)?;
            if jobs.is_empty() {
                println!("No jobs found.");
                return Ok(());
            }

            println!(
                "{:<10} {:<10} {:<12} {:<30}",
                "ID", "FORK", "STATUS", "DESCRIPTION"
            );
            println!("{}", "-".repeat(64));

            for job in jobs {
                let desc = if job.description.len() > 27 {
                    format!("{}...", &job.description[..27])
                } else {
                    job.description.clone()
                };
                println!(
                    "{:<10} {:<10} {:<12} {:<30}",
                    job.id, job.fork_id, job.status, desc
                );
            }
        }
    }
    Ok(())
}

/// List messages for a fork.
fn list_messages(db: &Database, fork_id: &str) -> Result<()> {
    let messages = MessageQueries::list_for_fork(db.conn(), fork_id)?;

    if messages.is_empty() {
        println!("No messages found for fork {fork_id}.");
        return Ok(());
    }

    for message in messages {
        let role = match message.role {
            MessageRole::User => "USER",
            MessageRole::Assistant => "ASSISTANT",
            MessageRole::System => "SYSTEM",
        };
        let time = message.created_at.format("%H:%M:%S");
        println!("[{time}] {role}:");
        println!("{}", message.content);
        println!();
    }

    Ok(())
}

/// Mark a fork as read.
fn mark_read(db: &Database, fork_id: &str) -> Result<()> {
    ForkQueries::mark_read(db.conn(), fork_id)?;
    println!("Marked fork {fork_id} as read.");
    Ok(())
}

/// Mark all forks as read.
fn mark_all_read(db: &Database) -> Result<()> {
    let count = ForkQueries::mark_all_read(db.conn())?;
    println!("Marked {count} fork(s) as read.");
    Ok(())
}

/// Signal that a fork has completed and notify the parent.
fn fork_done(_db: &Database, fork_id: &str, summary: &str) -> Result<()> {
    use std::fs::OpenOptions;
    use std::io::Write;

    // Create notifications directory
    let notif_dir = dirs::home_dir()
        .context("Could not find home directory")?
        .join(".forky")
        .join("notifications");
    std::fs::create_dir_all(&notif_dir)?;

    // Write to global pending notifications file
    let notif_file = notif_dir.join("pending.txt");
    let notification = format!(
        "{}|{}|{}\n",
        fork_id,
        Utc::now().format("%Y-%m-%d %H:%M:%S"),
        if summary.is_empty() { "Fork completed" } else { summary }
    );

    // Append to file (multiple forks might complete)
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
