//! Generic process spawning with streaming output.
//!
//! Provides async process spawning with:
//! - Configurable stdio handling
//! - Real-time stdout/stderr streaming via channels
//! - Timeout support
//! - Environment variable management
//! - Working directory configuration

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::ExitStatus;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;
use tokio::time::timeout;

/// Output line from a spawned process.
#[derive(Debug, Clone)]
pub enum ProcessOutput {
    /// Line from stdout.
    Stdout(String),
    /// Line from stderr.
    Stderr(String),
    /// Process has exited.
    Exit(ExitStatus),
}

/// Configuration options for spawning a process.
#[derive(Debug, Clone, Default)]
pub struct ProcessOptions {
    /// The program to execute.
    pub program: String,

    /// Arguments to pass to the program.
    pub args: Vec<String>,

    /// Working directory for the process.
    pub working_dir: Option<PathBuf>,

    /// Environment variables to set (merged with current env).
    pub env: HashMap<String, String>,

    /// Environment variables to remove.
    pub env_remove: Vec<String>,

    /// Whether to clear the environment before adding env vars.
    pub env_clear: bool,

    /// Timeout for the entire process execution.
    pub timeout: Option<Duration>,

    /// Whether to capture stdout (default: true).
    pub capture_stdout: bool,

    /// Whether to capture stderr (default: true).
    pub capture_stderr: bool,

    /// Whether to pipe stdin (default: false).
    pub pipe_stdin: bool,

    /// Buffer size for output channels (default: 1000).
    pub buffer_size: usize,
}

impl ProcessOptions {
    /// Create new options for the given program.
    pub fn new(program: impl Into<String>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            working_dir: None,
            env: HashMap::new(),
            env_remove: Vec::new(),
            env_clear: false,
            timeout: None,
            capture_stdout: true,
            capture_stderr: true,
            pipe_stdin: false,
            buffer_size: 1000,
        }
    }

    /// Add an argument.
    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.args.push(arg.into());
        self
    }

    /// Add multiple arguments.
    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    /// Set the working directory.
    pub fn working_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.working_dir = Some(dir.into());
        self
    }

    /// Set an environment variable.
    pub fn env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.insert(key.into(), value.into());
        self
    }

    /// Set multiple environment variables.
    pub fn envs<I, K, V>(mut self, vars: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        for (k, v) in vars {
            self.env.insert(k.into(), v.into());
        }
        self
    }

    /// Remove an environment variable.
    pub fn env_remove(mut self, key: impl Into<String>) -> Self {
        self.env_remove.push(key.into());
        self
    }

    /// Clear the environment before setting variables.
    pub fn env_clear(mut self) -> Self {
        self.env_clear = true;
        self
    }

    /// Set a timeout for the process.
    pub fn timeout(mut self, duration: Duration) -> Self {
        self.timeout = Some(duration);
        self
    }

    /// Enable stdin piping.
    pub fn pipe_stdin(mut self) -> Self {
        self.pipe_stdin = true;
        self
    }

    /// Set the output buffer size.
    pub fn buffer_size(mut self, size: usize) -> Self {
        self.buffer_size = size;
        self
    }
}

/// Result from a completed process.
#[derive(Debug)]
pub struct ProcessResult {
    /// Exit status of the process.
    pub status: ExitStatus,

    /// All stdout lines collected.
    pub stdout: Vec<String>,

    /// All stderr lines collected.
    pub stderr: Vec<String>,

    /// Whether the process was killed due to timeout.
    pub timed_out: bool,
}

impl ProcessResult {
    /// Check if the process exited successfully.
    pub fn success(&self) -> bool {
        self.status.success() && !self.timed_out
    }

    /// Get stdout as a single string.
    pub fn stdout_string(&self) -> String {
        self.stdout.join("\n")
    }

    /// Get stderr as a single string.
    pub fn stderr_string(&self) -> String {
        self.stderr.join("\n")
    }

    /// Get the exit code, if available.
    pub fn code(&self) -> Option<i32> {
        self.status.code()
    }
}

/// Spawn a process and collect all output.
///
/// This is a convenience function that spawns a process and waits for it
/// to complete, collecting all stdout and stderr output.
///
/// # Example
///
/// ```rust,no_run
/// use forky::process::{ProcessOptions, spawn_process};
///
/// #[tokio::main]
/// async fn main() -> anyhow::Result<()> {
///     let result = spawn_process(
///         ProcessOptions::new("ls")
///             .arg("-la")
///             .working_dir("/tmp")
///     ).await?;
///
///     println!("Exit code: {:?}", result.code());
///     println!("Output:\n{}", result.stdout_string());
///     Ok(())
/// }
/// ```
pub async fn spawn_process(options: ProcessOptions) -> Result<ProcessResult> {
    let mut cmd = Command::new(&options.program);

    // Add arguments
    cmd.args(&options.args);

    // Set working directory
    if let Some(ref dir) = options.working_dir {
        cmd.current_dir(dir);
    }

    // Handle environment
    if options.env_clear {
        cmd.env_clear();
    }

    for key in &options.env_remove {
        cmd.env_remove(key);
    }

    for (key, value) in &options.env {
        cmd.env(key, value);
    }

    // Configure stdio
    use std::process::Stdio;

    if options.capture_stdout {
        cmd.stdout(Stdio::piped());
    } else {
        cmd.stdout(Stdio::null());
    }

    if options.capture_stderr {
        cmd.stderr(Stdio::piped());
    } else {
        cmd.stderr(Stdio::null());
    }

    if options.pipe_stdin {
        cmd.stdin(Stdio::piped());
    } else {
        cmd.stdin(Stdio::null());
    }

    // Spawn the process
    let mut child = cmd
        .spawn()
        .with_context(|| format!("Failed to spawn process: {}", options.program))?;

    // Set up readers
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    // Create channel for output
    let (tx, mut rx) = mpsc::channel::<ProcessOutput>(options.buffer_size);

    // Spawn stdout reader task
    if let Some(stdout) = stdout {
        let tx = tx.clone();
        tokio::spawn(async move {
            let mut reader = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                if tx.send(ProcessOutput::Stdout(line)).await.is_err() {
                    break;
                }
            }
        });
    }

    // Spawn stderr reader task
    if let Some(stderr) = stderr {
        let tx = tx.clone();
        tokio::spawn(async move {
            let mut reader = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                if tx.send(ProcessOutput::Stderr(line)).await.is_err() {
                    break;
                }
            }
        });
    }

    // Drop the original sender so the channel closes when readers finish
    drop(tx);

    // Collect output with optional timeout
    let mut stdout_lines = Vec::new();
    let mut stderr_lines = Vec::new();
    let mut timed_out = false;

    let collect_future = async {
        while let Some(output) = rx.recv().await {
            match output {
                ProcessOutput::Stdout(line) => stdout_lines.push(line),
                ProcessOutput::Stderr(line) => stderr_lines.push(line),
                ProcessOutput::Exit(_) => break,
            }
        }
    };

    if let Some(duration) = options.timeout {
        if timeout(duration, collect_future).await.is_err() {
            timed_out = true;
            // Kill the process on timeout
            let _ = child.kill().await;
        }
    } else {
        collect_future.await;
    }

    // Wait for process to exit
    let status = child
        .wait()
        .await
        .context("Failed to wait for process to exit")?;

    Ok(ProcessResult {
        status,
        stdout: stdout_lines,
        stderr: stderr_lines,
        timed_out,
    })
}

/// Spawn a process with streaming output via a channel.
///
/// Returns a receiver that yields output lines as they arrive,
/// along with a handle to wait for process completion.
///
/// # Example
///
/// ```rust,no_run
/// use forky::process::{ProcessOptions, spawn_process_streaming};
///
/// #[tokio::main]
/// async fn main() -> anyhow::Result<()> {
///     let (mut rx, handle) = spawn_process_streaming(
///         ProcessOptions::new("tail")
///             .arg("-f")
///             .arg("/var/log/system.log")
///     ).await?;
///
///     // Process output as it arrives
///     while let Some(output) = rx.recv().await {
///         match output {
///             ProcessOutput::Stdout(line) => println!("OUT: {}", line),
///             ProcessOutput::Stderr(line) => eprintln!("ERR: {}", line),
///             ProcessOutput::Exit(status) => {
///                 println!("Process exited with: {:?}", status.code());
///                 break;
///             }
///         }
///     }
///
///     Ok(())
/// }
/// ```
pub async fn spawn_process_streaming(
    options: ProcessOptions,
) -> Result<(mpsc::Receiver<ProcessOutput>, tokio::task::JoinHandle<Result<ExitStatus>>)> {
    let mut cmd = Command::new(&options.program);

    // Add arguments
    cmd.args(&options.args);

    // Set working directory
    if let Some(ref dir) = options.working_dir {
        cmd.current_dir(dir);
    }

    // Handle environment
    if options.env_clear {
        cmd.env_clear();
    }

    for key in &options.env_remove {
        cmd.env_remove(key);
    }

    for (key, value) in &options.env {
        cmd.env(key, value);
    }

    // Configure stdio
    use std::process::Stdio;

    if options.capture_stdout {
        cmd.stdout(Stdio::piped());
    } else {
        cmd.stdout(Stdio::null());
    }

    if options.capture_stderr {
        cmd.stderr(Stdio::piped());
    } else {
        cmd.stderr(Stdio::null());
    }

    if options.pipe_stdin {
        cmd.stdin(Stdio::piped());
    } else {
        cmd.stdin(Stdio::null());
    }

    // Spawn the process
    let mut child = cmd
        .spawn()
        .with_context(|| format!("Failed to spawn process: {}", options.program))?;

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    let (tx, rx) = mpsc::channel::<ProcessOutput>(options.buffer_size);

    // Spawn stdout reader
    if let Some(stdout) = stdout {
        let tx = tx.clone();
        tokio::spawn(async move {
            let mut reader = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                if tx.send(ProcessOutput::Stdout(line)).await.is_err() {
                    break;
                }
            }
        });
    }

    // Spawn stderr reader
    if let Some(stderr) = stderr {
        let tx = tx.clone();
        tokio::spawn(async move {
            let mut reader = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                if tx.send(ProcessOutput::Stderr(line)).await.is_err() {
                    break;
                }
            }
        });
    }

    // Spawn task to wait for process and send exit status
    let handle = tokio::spawn(async move {
        let status = child
            .wait()
            .await
            .context("Failed to wait for process to exit")?;
        let _ = tx.send(ProcessOutput::Exit(status)).await;
        Ok(status)
    });

    Ok((rx, handle))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_spawn_echo() {
        let result = spawn_process(ProcessOptions::new("echo").arg("hello world"))
            .await
            .unwrap();

        assert!(result.success());
        assert_eq!(result.stdout, vec!["hello world"]);
        assert!(result.stderr.is_empty());
    }

    #[tokio::test]
    async fn test_spawn_with_env() {
        let result = spawn_process(
            ProcessOptions::new("sh")
                .arg("-c")
                .arg("echo $MY_VAR")
                .env("MY_VAR", "test_value"),
        )
        .await
        .unwrap();

        assert!(result.success());
        assert_eq!(result.stdout, vec!["test_value"]);
    }

    #[tokio::test]
    async fn test_spawn_with_working_dir() {
        let result = spawn_process(ProcessOptions::new("pwd").working_dir("/tmp"))
            .await
            .unwrap();

        assert!(result.success());
        // On macOS, /tmp is a symlink to /private/tmp
        assert!(result.stdout[0].contains("tmp"));
    }

    #[tokio::test]
    async fn test_spawn_nonexistent() {
        let result = spawn_process(ProcessOptions::new("nonexistent_command_12345")).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_spawn_with_timeout() {
        let result = spawn_process(
            ProcessOptions::new("sleep")
                .arg("10")
                .timeout(Duration::from_millis(100)),
        )
        .await
        .unwrap();

        assert!(result.timed_out);
        assert!(!result.success());
    }

    #[tokio::test]
    async fn test_spawn_stderr() {
        let result = spawn_process(
            ProcessOptions::new("sh")
                .arg("-c")
                .arg("echo error >&2"),
        )
        .await
        .unwrap();

        assert!(result.success());
        assert!(result.stdout.is_empty());
        assert_eq!(result.stderr, vec!["error"]);
    }

    #[tokio::test]
    async fn test_exit_code() {
        let result = spawn_process(ProcessOptions::new("sh").arg("-c").arg("exit 42"))
            .await
            .unwrap();

        assert!(!result.success());
        assert_eq!(result.code(), Some(42));
    }
}
