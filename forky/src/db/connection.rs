//! Database connection management.

use anyhow::{bail, Context, Result};
use rusqlite::Connection;
use std::path::PathBuf;

/// Database wrapper for forky.
pub struct Database {
    conn: Connection,
}

impl Database {
    /// Open or create the database at the project-local location (.claude/mod-claude/forky.db).
    pub fn open() -> Result<Self> {
        let db_path = Self::default_path()?;
        Self::open_at(&db_path)
    }

    /// Get the default database path (project-local).
    /// Searches for .claude directory walking up from current dir.
    pub fn default_path() -> Result<PathBuf> {
        let project_root = Self::find_project_root()?;
        let mod_claude_dir = project_root.join(".claude").join("mod-claude");
        std::fs::create_dir_all(&mod_claude_dir)
            .with_context(|| format!("Failed to create directory: {}", mod_claude_dir.display()))?;
        Ok(mod_claude_dir.join("forky.db"))
    }

    /// Find the project root by looking for .claude directory.
    fn find_project_root() -> Result<PathBuf> {
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

    /// Open or create the database at a specific path.
    pub fn open_at(path: &PathBuf) -> Result<Self> {
        let conn = Connection::open(path)
            .with_context(|| format!("Failed to open database at {}", path.display()))?;

        let db = Self { conn };
        db.initialize()?;
        Ok(db)
    }

    /// Initialize the database schema.
    fn initialize(&self) -> Result<()> {
        self.conn.execute_batch(
            r"
            CREATE TABLE IF NOT EXISTS forks (
                id TEXT PRIMARY KEY,
                parent_session_id TEXT,
                fork_session_id TEXT,
                ai_provider TEXT NOT NULL DEFAULT 'claude',
                name TEXT,
                status TEXT NOT NULL DEFAULT 'running',
                read INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL,
                completed_at TEXT
            );

            CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY,
                fork_id TEXT,
                created_at TEXT NOT NULL,
                FOREIGN KEY (fork_id) REFERENCES forks(id)
            );

            CREATE TABLE IF NOT EXISTS jobs (
                id TEXT PRIMARY KEY,
                description TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'running',
                fork_id TEXT NOT NULL,
                session_id TEXT,
                output TEXT,
                created_at TEXT NOT NULL,
                completed_at TEXT,
                FOREIGN KEY (fork_id) REFERENCES forks(id),
                FOREIGN KEY (session_id) REFERENCES sessions(id)
            );

            CREATE TABLE IF NOT EXISTS messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                fork_id TEXT NOT NULL,
                session_id TEXT,
                role TEXT NOT NULL,
                content TEXT NOT NULL,
                created_at TEXT NOT NULL,
                FOREIGN KEY (fork_id) REFERENCES forks(id),
                FOREIGN KEY (session_id) REFERENCES sessions(id)
            );

            CREATE INDEX IF NOT EXISTS idx_forks_status ON forks(status);
            CREATE INDEX IF NOT EXISTS idx_forks_read ON forks(read);
            CREATE INDEX IF NOT EXISTS idx_sessions_fork_id ON sessions(fork_id);
            CREATE INDEX IF NOT EXISTS idx_jobs_fork_id ON jobs(fork_id);
            CREATE INDEX IF NOT EXISTS idx_messages_fork_id ON messages(fork_id);
            ",
        )?;
        Ok(())
    }

    /// Get a reference to the connection.
    pub const fn conn(&self) -> &Connection {
        &self.conn
    }
}
