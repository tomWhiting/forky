//! Claude CLI interaction module.

mod events;
mod spawn;

pub use events::ClaudeEvent;
pub use spawn::{spawn_claude, ClaudeOptions};
