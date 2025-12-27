//! Forky - Fork Claude sessions to handle side tasks in parallel.
//!
//! This CLI tool allows you to spawn and manage parallel Claude sessions,
//! tracking forks, sessions, jobs, and messages in a local `SQLite` database.

// CLI application doesn't need Send futures - rusqlite::Connection is not Sync
#![allow(clippy::future_not_send)]

mod claude;
mod cli;
mod db;
mod models;
mod session;

use anyhow::Result;
use clap::Parser;

use cli::{execute, Cli};

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    execute(cli).await
}
