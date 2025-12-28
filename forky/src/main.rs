//! Forky - Fork Claude sessions to handle side tasks in parallel.
//!
//! This CLI tool allows you to spawn and manage parallel Claude sessions,
//! tracking forks and events in a ManifoldDB graph database.
//!
//! Architecture:
//! - CLI is a thin client that talks to the forky server via HTTP
//! - Server manages per-project ManifoldDB databases
//! - All database access goes through the server to avoid lock contention

mod claude;
mod cli;
mod db;
mod names;
mod process;
mod server;
mod session;

use anyhow::Result;
use clap::Parser;

use cli::{execute, Cli};

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    execute(cli).await
}
