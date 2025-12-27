//! CLI argument definitions.

use clap::{Parser, Subcommand, ValueEnum};

/// Forky - Fork Claude sessions to handle side tasks in parallel
#[derive(Parser, Debug)]
#[command(name = "forky")]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    /// Message the last fork instead of creating a new one
    #[arg(short = 'l', long = "last")]
    pub message_last: bool,

    /// Model to use for Claude (e.g., sonnet, opus, haiku)
    #[arg(short, long)]
    pub model: Option<String>,

    // === Directory / Worktree Options ===
    /// Run in a git worktree (creates branch forky/<fork-id>)
    #[arg(long)]
    pub worktree: bool,

    /// Directory to run the fork in (auto-adds as working directory)
    #[arg(long)]
    pub dir: Option<String>,

    // === Chrome Browser Options ===
    /// Enable Chrome browser integration
    #[arg(long)]
    pub chrome: bool,

    /// Disable Chrome browser integration
    #[arg(long, conflicts_with = "chrome")]
    pub no_chrome: bool,

    // === System Prompt Options ===
    /// Append text to the system prompt
    #[arg(long)]
    pub append_system_prompt: Option<String>,

    /// Replace the entire system prompt
    #[arg(long, conflicts_with = "append_system_prompt")]
    pub system_prompt: Option<String>,

    // === Advanced Options (wired up but not prominently exposed) ===
    /// Custom subagents as JSON
    #[arg(long, hide = true)]
    pub agents: Option<String>,

    /// MCP server configuration as JSON or path
    #[arg(long, hide = true)]
    pub mcp_config: Option<String>,

    /// Additional settings as JSON or path
    #[arg(long, hide = true)]
    pub settings: Option<String>,

    /// Maximum agentic turns (use sparingly)
    #[arg(long, hide = true)]
    pub max_turns: Option<u32>,

    /// Restrict available tools (comma-separated)
    #[arg(long, hide = true)]
    pub tools: Option<String>,

    /// Tools that don't require permission prompts
    #[arg(long, hide = true)]
    pub allowed_tools: Option<String>,

    /// Include partial streaming messages in output
    #[arg(long, hide = true)]
    pub include_partial_messages: bool,

    /// Message to send (used with default fork behavior)
    #[arg(trailing_var_arg = true)]
    pub message: Vec<String>,

    /// Subcommand to execute
    #[command(subcommand)]
    pub command: Option<Commands>,
}

/// Available subcommands
#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Fork the current session (explicit command)
    ForkMe {
        /// Message to send to the fork
        #[arg(trailing_var_arg = true)]
        message: Vec<String>,
    },

    /// Fork a specific session
    Fork {
        /// Session ID to fork
        id: String,

        /// Message to send to the fork
        #[arg(trailing_var_arg = true)]
        message: Vec<String>,
    },

    /// Resume a specific session
    Resume {
        /// Session ID to resume
        id: String,

        /// Message to send
        #[arg(trailing_var_arg = true)]
        message: Vec<String>,
    },

    /// List forks, sessions, or jobs
    List {
        /// Entity type to list
        #[arg(value_enum)]
        entity: ListEntity,
    },

    /// View messages for a fork
    Messages {
        /// Fork ID to view messages for
        fork_id: String,
    },

    /// Mark a fork as read
    Read {
        /// Fork ID to mark as read (or --all for all forks)
        #[arg(required_unless_present = "all")]
        id: Option<String>,

        /// Mark all forks as read
        #[arg(long)]
        all: bool,
    },

    /// Start a fresh Claude session (no forking)
    New {
        /// Message to send to the new session
        #[arg(trailing_var_arg = true)]
        message: Vec<String>,
    },

    /// Signal that a fork has completed (called by forked agents)
    Done {
        /// Fork ID that completed
        fork_id: String,

        /// Summary of what was accomplished
        #[arg(trailing_var_arg = true)]
        summary: Vec<String>,
    },

    /// Start the streaming server for fork observability
    Serve {
        /// Port to listen on
        #[arg(short, long, default_value = "58231")]
        port: u16,

        /// Open browser automatically
        #[arg(long)]
        open: bool,
    },

    /// Debug: show events stored in the graph database
    Events {
        /// Session ID to filter by (optional)
        #[arg(short, long)]
        session: Option<String>,

        /// Maximum number of events to show
        #[arg(short, long, default_value = "20")]
        limit: usize,
    },
}

/// Entity types that can be listed
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum ListEntity {
    /// List forks
    Forks,
    /// List sessions
    Sessions,
    /// List jobs
    Jobs,
}
