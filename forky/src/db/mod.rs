//! Database module for Forky storage.
//!
//! Uses ManifoldDB graph database for all storage.
//! The server manages database access to avoid lock contention.

mod graph;

pub use graph::GraphDatabase;
