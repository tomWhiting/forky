//! Generic process spawning utilities.
//!
//! This module provides a flexible, async-first approach to spawning
//! and managing child processes with streaming output.

mod spawn;
mod pool;

pub use spawn::{ProcessOptions, ProcessResult, ProcessOutput, spawn_process};
pub use pool::{ProcessPool, PooledProcess};
