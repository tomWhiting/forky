//! Database query implementations.

use anyhow::{Context, Result};
use chrono::{DateTime, NaiveDateTime, Utc};
use rusqlite::{params, Connection};

use crate::models::{Fork, ForkStatus, Job, JobStatus, Message, MessageRole, Session};

/// Parse a timestamp string flexibly from various formats.
fn parse_timestamp(s: &str) -> Result<DateTime<Utc>> {
    // Try RFC3339 first
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Ok(dt.with_timezone(&Utc));
    }

    // Try common SQLite datetime format: "YYYY-MM-DD HH:MM:SS"
    if let Ok(naive) = NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S") {
        return Ok(naive.and_utc());
    }

    // Try with fractional seconds: "YYYY-MM-DD HH:MM:SS.SSS"
    if let Ok(naive) = NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S%.f") {
        return Ok(naive.and_utc());
    }

    anyhow::bail!("Invalid timestamp format: {s}")
}

/// Queries for forks table.
pub struct ForkQueries;

impl ForkQueries {
    /// Insert a new fork.
    pub fn insert(conn: &Connection, fork: &Fork) -> Result<()> {
        conn.execute(
            r"INSERT INTO forks (id, parent_session_id, fork_session_id, ai_provider, name, status, read, created_at, completed_at)
              VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                fork.id,
                fork.parent_session_id,
                fork.fork_session_id,
                fork.ai_provider,
                fork.name,
                fork.status.as_str(),
                fork.read,
                fork.created_at.to_rfc3339(),
                fork.completed_at.map(|dt| dt.to_rfc3339()),
            ],
        )?;
        Ok(())
    }

    /// Get a fork by ID.
    #[allow(dead_code)]
    pub fn get_by_id(conn: &Connection, id: &str) -> Result<Option<Fork>> {
        let mut stmt = conn.prepare(
            r"SELECT id, parent_session_id, fork_session_id, ai_provider, name, status, read, created_at, completed_at
              FROM forks WHERE id = ?1",
        )?;

        let result = stmt.query_row(params![id], |row| Ok(Self::row_to_fork(row)));

        match result {
            Ok(fork) => Ok(Some(fork?)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Get the most recent fork.
    pub fn get_latest(conn: &Connection) -> Result<Option<Fork>> {
        let mut stmt = conn.prepare(
            r"SELECT id, parent_session_id, fork_session_id, ai_provider, name, status, read, created_at, completed_at
              FROM forks ORDER BY created_at DESC LIMIT 1",
        )?;

        let result = stmt.query_row([], |row| Ok(Self::row_to_fork(row)));

        match result {
            Ok(fork) => Ok(Some(fork?)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// List all forks, optionally filtered by status.
    pub fn list(conn: &Connection, status: Option<ForkStatus>) -> Result<Vec<Fork>> {
        let mut forks = Vec::new();

        if let Some(s) = status {
            let mut stmt = conn.prepare(
                r"SELECT id, parent_session_id, fork_session_id, ai_provider, name, status, read, created_at, completed_at
                  FROM forks WHERE status = ?1 ORDER BY created_at DESC",
            )?;
            let rows = stmt.query_map(params![s.as_str()], |row| Ok(Self::row_to_fork(row)))?;
            for row in rows {
                forks.push(row??);
            }
        } else {
            let mut stmt = conn.prepare(
                r"SELECT id, parent_session_id, fork_session_id, ai_provider, name, status, read, created_at, completed_at
                  FROM forks ORDER BY created_at DESC",
            )?;
            let rows = stmt.query_map([], |row| Ok(Self::row_to_fork(row)))?;
            for row in rows {
                forks.push(row??);
            }
        }

        Ok(forks)
    }

    /// Update fork session ID.
    pub fn update_session_id(conn: &Connection, fork_id: &str, session_id: &str) -> Result<()> {
        conn.execute(
            "UPDATE forks SET fork_session_id = ?1 WHERE id = ?2",
            params![session_id, fork_id],
        )?;
        Ok(())
    }

    /// Update fork status.
    pub fn update_status(
        conn: &Connection,
        fork_id: &str,
        status: ForkStatus,
        completed_at: Option<DateTime<Utc>>,
    ) -> Result<()> {
        conn.execute(
            "UPDATE forks SET status = ?1, completed_at = ?2 WHERE id = ?3",
            params![
                status.as_str(),
                completed_at.map(|dt| dt.to_rfc3339()),
                fork_id
            ],
        )?;
        Ok(())
    }

    /// Mark fork as read.
    pub fn mark_read(conn: &Connection, fork_id: &str) -> Result<()> {
        conn.execute("UPDATE forks SET read = 1 WHERE id = ?1", params![fork_id])?;
        Ok(())
    }

    /// Mark all forks as read.
    pub fn mark_all_read(conn: &Connection) -> Result<usize> {
        let count = conn.execute("UPDATE forks SET read = 1 WHERE read = 0", [])?;
        Ok(count)
    }

    /// Convert a row to a Fork.
    fn row_to_fork(row: &rusqlite::Row<'_>) -> Result<Fork> {
        let status_str: String = row.get(5)?;
        let status = ForkStatus::from_str(&status_str)
            .context(format!("Invalid fork status: {status_str}"))?;

        let created_at_str: String = row.get(7)?;
        let created_at = parse_timestamp(&created_at_str)?;

        let completed_at: Option<DateTime<Utc>> = row
            .get::<_, Option<String>>(8)?
            .map(|s| parse_timestamp(&s))
            .transpose()?;

        Ok(Fork {
            id: row.get(0)?,
            parent_session_id: row.get(1)?,
            fork_session_id: row.get(2)?,
            ai_provider: row.get(3)?,
            name: row.get(4)?,
            status,
            read: row.get(6)?,
            created_at,
            completed_at,
        })
    }
}

/// Queries for sessions table.
pub struct SessionQueries;

impl SessionQueries {
    /// Insert a new session.
    pub fn insert(conn: &Connection, session: &Session) -> Result<()> {
        conn.execute(
            "INSERT INTO sessions (id, fork_id, created_at) VALUES (?1, ?2, ?3)",
            params![session.id, session.fork_id, session.created_at.to_rfc3339(),],
        )?;
        Ok(())
    }

    /// Get a session by ID.
    #[allow(dead_code)]
    pub fn get_by_id(conn: &Connection, id: &str) -> Result<Option<Session>> {
        let mut stmt =
            conn.prepare("SELECT id, fork_id, created_at FROM sessions WHERE id = ?1")?;

        let result = stmt.query_row(params![id], |row| Ok(Self::row_to_session(row)));

        match result {
            Ok(session) => Ok(Some(session?)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// List all sessions.
    pub fn list(conn: &Connection) -> Result<Vec<Session>> {
        let mut stmt =
            conn.prepare("SELECT id, fork_id, created_at FROM sessions ORDER BY created_at DESC")?;
        let rows = stmt.query_map([], |row| Ok(Self::row_to_session(row)))?;

        let mut sessions = Vec::new();
        for row in rows {
            sessions.push(row??);
        }
        Ok(sessions)
    }

    /// Convert a row to a Session.
    fn row_to_session(row: &rusqlite::Row<'_>) -> Result<Session> {
        let created_at_str: String = row.get(2)?;
        let created_at = parse_timestamp(&created_at_str)?;

        Ok(Session {
            id: row.get(0)?,
            fork_id: row.get(1)?,
            created_at,
        })
    }
}

/// Queries for jobs table.
pub struct JobQueries;

impl JobQueries {
    /// Insert a new job.
    pub fn insert(conn: &Connection, job: &Job) -> Result<()> {
        conn.execute(
            r"INSERT INTO jobs (id, description, status, fork_id, session_id, output, created_at, completed_at)
              VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                job.id,
                job.description,
                job.status.as_str(),
                job.fork_id,
                job.session_id,
                job.output,
                job.created_at.to_rfc3339(),
                job.completed_at.map(|dt| dt.to_rfc3339()),
            ],
        )?;
        Ok(())
    }

    /// Update job status and output.
    pub fn update_status(
        conn: &Connection,
        job_id: &str,
        status: JobStatus,
        output: Option<&str>,
        completed_at: Option<DateTime<Utc>>,
    ) -> Result<()> {
        conn.execute(
            "UPDATE jobs SET status = ?1, output = ?2, completed_at = ?3 WHERE id = ?4",
            params![
                status.as_str(),
                output,
                completed_at.map(|dt| dt.to_rfc3339()),
                job_id
            ],
        )?;
        Ok(())
    }

    /// Update job session ID.
    pub fn update_session_id(conn: &Connection, job_id: &str, session_id: &str) -> Result<()> {
        conn.execute(
            "UPDATE jobs SET session_id = ?1 WHERE id = ?2",
            params![session_id, job_id],
        )?;
        Ok(())
    }

    /// List all jobs, optionally filtered by fork ID.
    pub fn list(conn: &Connection, fork_id: Option<&str>) -> Result<Vec<Job>> {
        let mut jobs = Vec::new();

        if let Some(fid) = fork_id {
            let mut stmt = conn.prepare(
                r"SELECT id, description, status, fork_id, session_id, output, created_at, completed_at
                  FROM jobs WHERE fork_id = ?1 ORDER BY created_at DESC",
            )?;
            let rows = stmt.query_map(params![fid], |row| Ok(Self::row_to_job(row)))?;
            for row in rows {
                jobs.push(row??);
            }
        } else {
            let mut stmt = conn.prepare(
                r"SELECT id, description, status, fork_id, session_id, output, created_at, completed_at
                  FROM jobs ORDER BY created_at DESC",
            )?;
            let rows = stmt.query_map([], |row| Ok(Self::row_to_job(row)))?;
            for row in rows {
                jobs.push(row??);
            }
        }

        Ok(jobs)
    }

    /// Convert a row to a Job.
    fn row_to_job(row: &rusqlite::Row<'_>) -> Result<Job> {
        let status_str: String = row.get(2)?;
        let status = JobStatus::from_str(&status_str)
            .context(format!("Invalid job status: {status_str}"))?;

        let created_at_str: String = row.get(6)?;
        let created_at = parse_timestamp(&created_at_str)?;

        let completed_at: Option<DateTime<Utc>> = row
            .get::<_, Option<String>>(7)?
            .map(|s| parse_timestamp(&s))
            .transpose()?;

        Ok(Job {
            id: row.get(0)?,
            description: row.get(1)?,
            status,
            fork_id: row.get(3)?,
            session_id: row.get(4)?,
            output: row.get(5)?,
            created_at,
            completed_at,
        })
    }
}

/// Queries for messages table.
pub struct MessageQueries;

impl MessageQueries {
    /// Insert a new message (id is auto-generated).
    pub fn insert(conn: &Connection, message: &Message) -> Result<i64> {
        conn.execute(
            r"INSERT INTO messages (fork_id, session_id, role, content, created_at)
              VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                message.fork_id,
                message.session_id,
                message.role.as_str(),
                message.content,
                message.created_at.to_rfc3339(),
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// List messages for a fork.
    pub fn list_for_fork(conn: &Connection, fork_id: &str) -> Result<Vec<Message>> {
        let mut stmt = conn.prepare(
            r"SELECT id, fork_id, session_id, role, content, created_at
              FROM messages WHERE fork_id = ?1 ORDER BY created_at ASC",
        )?;
        let rows = stmt.query_map(params![fork_id], |row| Ok(Self::row_to_message(row)))?;

        let mut messages = Vec::new();
        for row in rows {
            messages.push(row??);
        }
        Ok(messages)
    }

    /// Convert a row to a Message.
    fn row_to_message(row: &rusqlite::Row<'_>) -> Result<Message> {
        let role_str: String = row.get(3)?;
        let role = MessageRole::from_str(&role_str)
            .context(format!("Invalid message role: {role_str}"))?;

        let created_at_str: String = row.get(5)?;
        let created_at = parse_timestamp(&created_at_str)?;

        Ok(Message {
            id: row.get(0)?,
            fork_id: row.get(1)?,
            session_id: row.get(2)?,
            role,
            content: row.get(4)?,
            created_at,
        })
    }
}
