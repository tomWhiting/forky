//! Process pool for managing multiple concurrent processes.
//!
//! Provides a pool abstraction for spawning and managing multiple processes
//! with concurrency limits and resource management.

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::process::ExitStatus;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot, Semaphore};

use super::spawn::{ProcessOptions, ProcessOutput, ProcessResult};

/// A unique identifier for a pooled process.
pub type ProcessId = usize;

/// Handle to a running process in the pool.
#[derive(Debug)]
pub struct PooledProcess {
    /// Unique ID for this process.
    pub id: ProcessId,

    /// Receiver for streaming output.
    pub output: mpsc::Receiver<ProcessOutput>,

    /// Channel to receive the final result.
    result_rx: oneshot::Receiver<ProcessResult>,
}

impl PooledProcess {
    /// Wait for the process to complete and get the result.
    pub async fn wait(self) -> Result<ProcessResult> {
        self.result_rx
            .await
            .context("Process task was dropped before completion")
    }
}

/// Event emitted by the process pool.
#[derive(Debug, Clone)]
pub enum PoolEvent {
    /// A process has started.
    Started { id: ProcessId },

    /// Output from a process.
    Output { id: ProcessId, output: ProcessOutput },

    /// A process has completed.
    Completed { id: ProcessId, success: bool },
}

/// A pool for managing multiple concurrent processes.
///
/// The pool limits concurrency and provides a unified interface for
/// spawning and monitoring multiple processes.
///
/// # Example
///
/// ```rust,no_run
/// use forky::process::{ProcessPool, ProcessOptions};
///
/// #[tokio::main]
/// async fn main() -> anyhow::Result<()> {
///     // Create a pool with max 4 concurrent processes
///     let pool = ProcessPool::new(4);
///
///     // Spawn multiple processes
///     let proc1 = pool.spawn(ProcessOptions::new("sleep").arg("1")).await?;
///     let proc2 = pool.spawn(ProcessOptions::new("sleep").arg("2")).await?;
///
///     // Wait for all to complete
///     let result1 = proc1.wait().await?;
///     let result2 = proc2.wait().await?;
///
///     println!("Process 1 success: {}", result1.success());
///     println!("Process 2 success: {}", result2.success());
///
///     Ok(())
/// }
/// ```
pub struct ProcessPool {
    /// Semaphore for limiting concurrency.
    semaphore: Arc<Semaphore>,

    /// Counter for generating unique process IDs.
    next_id: AtomicUsize,

    /// Optional channel for pool-wide events.
    event_tx: Option<mpsc::Sender<PoolEvent>>,
}

impl ProcessPool {
    /// Create a new process pool with the given concurrency limit.
    pub fn new(max_concurrent: usize) -> Self {
        Self {
            semaphore: Arc::new(Semaphore::new(max_concurrent)),
            next_id: AtomicUsize::new(0),
            event_tx: None,
        }
    }

    /// Create a pool with an event channel for monitoring all processes.
    pub fn with_events(max_concurrent: usize) -> (Self, mpsc::Receiver<PoolEvent>) {
        let (tx, rx) = mpsc::channel(1000);
        let pool = Self {
            semaphore: Arc::new(Semaphore::new(max_concurrent)),
            next_id: AtomicUsize::new(0),
            event_tx: Some(tx),
        };
        (pool, rx)
    }

    /// Spawn a process in the pool.
    ///
    /// This will block if the pool is at capacity until a slot becomes available.
    pub async fn spawn(&self, options: ProcessOptions) -> Result<PooledProcess> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let semaphore = self.semaphore.clone();
        let event_tx = self.event_tx.clone();
        let buffer_size = options.buffer_size;

        // Create channels
        let (output_tx, output_rx) = mpsc::channel(buffer_size);
        let (result_tx, result_rx) = oneshot::channel();

        // Spawn task to run the process
        tokio::spawn(async move {
            // Acquire semaphore permit (waits if at capacity)
            let _permit = semaphore.acquire().await;

            // Notify started
            if let Some(ref tx) = event_tx {
                let _ = tx.send(PoolEvent::Started { id }).await;
            }

            // Run the actual process
            match run_process_internal(options, id, output_tx.clone(), event_tx.clone()).await {
                Ok(result) => {
                    let success = result.success();

                    // Notify completed
                    if let Some(ref tx) = event_tx {
                        let _ = tx.send(PoolEvent::Completed { id, success }).await;
                    }

                    let _ = result_tx.send(result);
                }
                Err(e) => {
                    // Notify completed with failure
                    if let Some(ref tx) = event_tx {
                        let _ = tx
                            .send(PoolEvent::Completed {
                                id,
                                success: false,
                            })
                            .await;
                    }

                    // Send a failure result
                    let _ = result_tx.send(ProcessResult {
                        status: <ExitStatus as ExitStatusExt>::default(),
                        stdout: Vec::new(),
                        stderr: vec![e.to_string()],
                        timed_out: false,
                    });
                }
            }
        });

        Ok(PooledProcess {
            id,
            output: output_rx,
            result_rx,
        })
    }

    /// Spawn multiple processes and wait for all to complete.
    pub async fn spawn_all(
        &self,
        options_list: Vec<ProcessOptions>,
    ) -> Result<HashMap<ProcessId, ProcessResult>> {
        let mut handles = Vec::new();

        for options in options_list {
            let proc = self.spawn(options).await?;
            handles.push(proc);
        }

        let mut results = HashMap::new();
        for proc in handles {
            let id = proc.id;
            let result = proc.wait().await?;
            results.insert(id, result);
        }

        Ok(results)
    }

    /// Get the number of available slots in the pool.
    pub fn available_permits(&self) -> usize {
        self.semaphore.available_permits()
    }
}

/// Internal function to run a process and stream output.
async fn run_process_internal(
    options: ProcessOptions,
    id: ProcessId,
    output_tx: mpsc::Sender<ProcessOutput>,
    event_tx: Option<mpsc::Sender<PoolEvent>>,
) -> Result<ProcessResult> {
    use std::process::Stdio;
    use tokio::io::{AsyncBufReadExt, BufReader};
    use tokio::process::Command;

    let mut cmd = Command::new(&options.program);
    cmd.args(&options.args);

    if let Some(ref dir) = options.working_dir {
        cmd.current_dir(dir);
    }

    if options.env_clear {
        cmd.env_clear();
    }

    for key in &options.env_remove {
        cmd.env_remove(key);
    }

    for (key, value) in &options.env {
        cmd.env(key, value);
    }

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

    cmd.stdin(Stdio::null());

    let mut child = cmd
        .spawn()
        .with_context(|| format!("Failed to spawn process: {}", options.program))?;

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    let mut stdout_lines = Vec::new();
    let mut stderr_lines = Vec::new();

    // Spawn stdout reader
    let stdout_handle = if let Some(stdout) = stdout {
        let output_tx = output_tx.clone();
        let event_tx = event_tx.clone();
        Some(tokio::spawn(async move {
            let mut lines = Vec::new();
            let mut reader = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                lines.push(line.clone());
                let _ = output_tx.send(ProcessOutput::Stdout(line.clone())).await;
                if let Some(ref tx) = event_tx {
                    let _ = tx
                        .send(PoolEvent::Output {
                            id,
                            output: ProcessOutput::Stdout(line),
                        })
                        .await;
                }
            }
            lines
        }))
    } else {
        None
    };

    // Spawn stderr reader
    let stderr_handle = if let Some(stderr) = stderr {
        let output_tx = output_tx.clone();
        let event_tx = event_tx.clone();
        Some(tokio::spawn(async move {
            let mut lines = Vec::new();
            let mut reader = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                lines.push(line.clone());
                let _ = output_tx.send(ProcessOutput::Stderr(line.clone())).await;
                if let Some(ref tx) = event_tx {
                    let _ = tx
                        .send(PoolEvent::Output {
                            id,
                            output: ProcessOutput::Stderr(line),
                        })
                        .await;
                }
            }
            lines
        }))
    } else {
        None
    };

    // Wait for readers to complete
    if let Some(handle) = stdout_handle {
        if let Ok(lines) = handle.await {
            stdout_lines = lines;
        }
    }

    if let Some(handle) = stderr_handle {
        if let Ok(lines) = handle.await {
            stderr_lines = lines;
        }
    }

    // Wait for process
    let status = child
        .wait()
        .await
        .context("Failed to wait for process to exit")?;

    // Send exit status
    let _ = output_tx.send(ProcessOutput::Exit(status)).await;

    Ok(ProcessResult {
        status,
        stdout: stdout_lines,
        stderr: stderr_lines,
        timed_out: false,
    })
}

/// Trait extension to create a default ExitStatus (for error cases).
trait ExitStatusExt {
    fn default() -> ExitStatus;
}

impl ExitStatusExt for ExitStatus {
    #[cfg(unix)]
    fn default() -> ExitStatus {
        use std::os::unix::process::ExitStatusExt;
        ExitStatus::from_raw(1)
    }

    #[cfg(windows)]
    fn default() -> ExitStatus {
        use std::os::windows::process::ExitStatusExt;
        ExitStatus::from_raw(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_pool_basic() {
        let pool = ProcessPool::new(2);

        let proc = pool
            .spawn(ProcessOptions::new("echo").arg("hello"))
            .await
            .unwrap();

        let result = proc.wait().await.unwrap();
        assert!(result.success());
        assert_eq!(result.stdout, vec!["hello"]);
    }

    #[tokio::test]
    async fn test_pool_spawn_all() {
        let pool = ProcessPool::new(4);

        let options = vec![
            ProcessOptions::new("echo").arg("one"),
            ProcessOptions::new("echo").arg("two"),
            ProcessOptions::new("echo").arg("three"),
        ];

        let results = pool.spawn_all(options).await.unwrap();
        assert_eq!(results.len(), 3);

        for (_, result) in results {
            assert!(result.success());
        }
    }

    #[tokio::test]
    async fn test_pool_with_events() {
        let (pool, mut events) = ProcessPool::with_events(2);

        let proc = pool
            .spawn(ProcessOptions::new("echo").arg("test"))
            .await
            .unwrap();

        // Should receive started event
        if let Some(PoolEvent::Started { id }) = events.recv().await {
            assert_eq!(id, proc.id);
        }

        // Should receive output event
        let mut got_output = false;
        while let Some(event) = events.recv().await {
            match event {
                PoolEvent::Output { id, output } => {
                    assert_eq!(id, proc.id);
                    if let ProcessOutput::Stdout(line) = output {
                        assert_eq!(line, "test");
                        got_output = true;
                    }
                }
                PoolEvent::Completed { id, success } => {
                    assert_eq!(id, proc.id);
                    assert!(success);
                    break;
                }
                _ => {}
            }
        }
        assert!(got_output);
    }

    #[tokio::test]
    async fn test_pool_concurrency_limit() {
        use std::time::{Duration, Instant};

        let pool = ProcessPool::new(1); // Only 1 concurrent process

        let start = Instant::now();

        // Spawn 2 processes that each sleep for 100ms
        let proc1 = pool
            .spawn(ProcessOptions::new("sleep").arg("0.1"))
            .await
            .unwrap();
        let proc2 = pool
            .spawn(ProcessOptions::new("sleep").arg("0.1"))
            .await
            .unwrap();

        // Wait for both
        let _ = proc1.wait().await.unwrap();
        let _ = proc2.wait().await.unwrap();

        let elapsed = start.elapsed();

        // With concurrency limit of 1, should take at least 200ms
        assert!(elapsed >= Duration::from_millis(180)); // Allow some slack
    }
}
