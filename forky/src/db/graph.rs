//! ManifoldDB-based graph storage for Forky.
//!
//! This module stores Claude events and fork data as a graph:
//! - Events are entities with label "Event"
//! - Events link to parents via "CHILD_OF" edges (parent_tool_use_id)
//! - Forks, Sessions, Jobs are also entities with their respective labels
//! - Relationships form a navigable graph of Claude conversations

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use manifoldb_core::{Edge, Entity, EntityId, Value};
use manifoldb_graph::store::{EdgeStore, IdGenerator, NodeStore};
use manifoldb_storage::backends::RedbEngine;
use manifoldb_storage::{StorageEngine, Transaction};

use crate::claude::ClaudeEvent;

/// Edge type for parent-child event relationships (sub-agent nesting).
pub const EDGE_CHILD_OF: &str = "CHILD_OF";

/// Edge type for tool_result → tool_use linking.
pub const EDGE_RESPONDS_TO: &str = "RESPONDS_TO";

/// Edge type for fork-to-session relationships.
pub const EDGE_HAS_SESSION: &str = "HAS_SESSION";

/// Edge type for fork-to-job relationships.
pub const EDGE_HAS_JOB: &str = "HAS_JOB";

/// Edge type for session-to-event relationships.
pub const EDGE_HAS_EVENT: &str = "HAS_EVENT";

/// Label for event entities.
pub const LABEL_EVENT: &str = "Event";

/// Label for fork entities.
pub const LABEL_FORK: &str = "Fork";

/// Label for session entities.
pub const LABEL_SESSION: &str = "Session";

/// Label for job entities.
pub const LABEL_JOB: &str = "Job";

/// Graph database for Forky using ManifoldDB.
pub struct GraphDatabase {
    engine: Arc<RedbEngine>,
    id_gen: IdGenerator,
    /// Index: tool_use_id -> EntityId of the event containing it
    tool_use_index: HashMap<String, EntityId>,
    /// Index: event uuid -> EntityId
    uuid_index: HashMap<String, EntityId>,
}

impl GraphDatabase {
    /// Open the graph database at the default project location.
    pub fn open() -> Result<Self> {
        let db_path = Self::default_path()?;
        Self::open_at(&db_path)
    }

    /// Get the default database path (project-local).
    pub fn default_path() -> Result<PathBuf> {
        let project_root = Self::find_project_root()?;
        let mod_claude_dir = project_root.join(".claude").join("mod-claude");
        std::fs::create_dir_all(&mod_claude_dir)
            .with_context(|| format!("Failed to create directory: {}", mod_claude_dir.display()))?;
        Ok(mod_claude_dir.join("forky.redb"))
    }

    /// Find the project root by looking for .claude directory.
    fn find_project_root() -> Result<PathBuf> {
        let mut current = std::env::current_dir().context("Failed to get current directory")?;

        loop {
            if current.join(".claude").is_dir() {
                return Ok(current);
            }

            if !current.pop() {
                anyhow::bail!(
                    "Could not find .claude directory. Are you in a Claude Code project?"
                );
            }
        }
    }

    /// Open the database at a specific path.
    pub fn open_at(path: &PathBuf) -> Result<Self> {
        let engine = RedbEngine::open(path)
            .with_context(|| format!("Failed to open database at {}", path.display()))?;

        let id_gen = IdGenerator::new();

        // Build indexes by scanning existing data
        let (tool_use_index, uuid_index) = Self::build_indexes(&engine, &id_gen)?;

        Ok(Self {
            engine: Arc::new(engine),
            id_gen,
            tool_use_index,
            uuid_index,
        })
    }

    /// Build indexes from existing data.
    fn build_indexes(
        engine: &RedbEngine,
        _id_gen: &IdGenerator,
    ) -> Result<(HashMap<String, EntityId>, HashMap<String, EntityId>)> {
        let mut tool_use_index = HashMap::new();
        let mut uuid_index = HashMap::new();

        // Scan all Event entities
        let tx = engine.begin_read()?;
        let event_ids = NodeStore::find_by_label(&tx, &LABEL_EVENT.into())?;

        for entity_id in event_ids {
            if let Some(entity) = NodeStore::get(&tx, entity_id)? {
                // Index by uuid
                if let Some(Value::String(uuid)) = entity.properties.get("uuid") {
                    uuid_index.insert(uuid.clone(), entity_id);
                }

                // Index by tool_use_ids
                if let Some(Value::String(tool_ids_json)) = entity.properties.get("tool_use_ids") {
                    if let Ok(ids) = serde_json::from_str::<Vec<String>>(tool_ids_json) {
                        for id in ids {
                            tool_use_index.insert(id, entity_id);
                        }
                    }
                }
            }
        }

        Ok((tool_use_index, uuid_index))
    }

    /// Store a Claude event as a graph entity.
    ///
    /// Creates an Event entity and links it to its parent if `parent_tool_use_id` is set.
    pub fn store_event(&mut self, event: &ClaudeEvent, fork_id: Option<&str>) -> Result<EntityId> {
        let mut tx = self.engine.begin_write()?;

        // Create the event entity
        let entity = NodeStore::create(&mut tx, &self.id_gen, |id| {
            let mut e = Entity::new(id).with_label(LABEL_EVENT);

            // Fork ID
            if let Some(fid) = fork_id {
                e = e.with_property("fork_id", Value::String(fid.to_string()));
            }

            // Core fields
            if let Some(ref uuid) = event.uuid {
                e = e.with_property("uuid", Value::String(uuid.clone()));
            }
            if let Some(ref session_id) = event.session_id {
                e = e.with_property("session_id", Value::String(session_id.clone()));
            }
            if let Some(ref parent_id) = event.parent_tool_use_id {
                e = e.with_property("parent_tool_use_id", Value::String(parent_id.clone()));
            }

            // Type info
            e = e.with_property("type", Value::String(event.type_label().to_string()));
            if let Some(ref subtype) = event.subtype {
                e = e.with_property("subtype", Value::String(subtype.clone()));
            }

            // Content (the useful stuff)
            if let Some(ref msg) = event.message {
                e = e.with_property("message", Value::String(msg.clone()));
            }
            if let Some(ref thinking) = event.thinking {
                e = e.with_property("thinking", Value::String(thinking.clone()));
            }
            if let Some(ref result) = event.result {
                e = e.with_property("result", Value::String(result.clone()));
            }

            // Message metadata
            if let Some(ref model) = event.model {
                e = e.with_property("model", Value::String(model.clone()));
            }
            if let Some(ref message_id) = event.message_id {
                e = e.with_property("message_id", Value::String(message_id.clone()));
            }
            if let Some(ref role) = event.role {
                e = e.with_property("role", Value::String(role.clone()));
            }

            // Tool uses (store as JSON array for queryability)
            if !event.tool_uses.is_empty() {
                let tool_uses_json = serde_json::to_string(&event.tool_uses).unwrap_or_default();
                e = e.with_property("tool_uses", Value::String(tool_uses_json));
            }

            // Tool results (links tool_result to its tool_use)
            if !event.tool_results.is_empty() {
                let tool_results_json =
                    serde_json::to_string(&event.tool_results).unwrap_or_default();
                e = e.with_property("tool_results", Value::String(tool_results_json));
            }

            // Metrics (cost is useful, token counts are not)
            if let Some(cost) = event.cost_usd {
                e = e.with_property("cost_usd", Value::Float(cost));
            }
            if let Some(total_cost) = event.total_cost_usd {
                e = e.with_property("total_cost_usd", Value::Float(total_cost));
            }
            if let Some(duration) = event.duration_ms {
                e = e.with_property("duration_ms", Value::Int(duration as i64));
            }
            if let Some(turns) = event.num_turns {
                e = e.with_property("num_turns", Value::Int(i64::from(turns)));
            }

            // Tool use IDs (for indexing children)
            if !event.tool_use_ids.is_empty() {
                let ids_json = serde_json::to_string(&event.tool_use_ids).unwrap_or_default();
                e = e.with_property("tool_use_ids", Value::String(ids_json));
            }

            // Store raw JSON
            let raw_json = serde_json::to_string(&event.raw).unwrap_or_default();
            e = e.with_property("raw", Value::String(raw_json));

            e
        })?;

        let entity_id = entity.id;

        // Update indexes
        if let Some(ref uuid) = event.uuid {
            self.uuid_index.insert(uuid.clone(), entity_id);
        }
        for tool_id in &event.tool_use_ids {
            self.tool_use_index.insert(tool_id.clone(), entity_id);
        }

        // Create edge to parent if parent_tool_use_id is set (sub-agent nesting)
        if let Some(ref parent_tool_id) = event.parent_tool_use_id {
            if let Some(&parent_entity_id) = self.tool_use_index.get(parent_tool_id) {
                // Create CHILD_OF edge from this event to parent
                EdgeStore::create(
                    &mut tx,
                    &self.id_gen,
                    entity_id,
                    parent_entity_id,
                    EDGE_CHILD_OF,
                    |id| Edge::new(id, entity_id, parent_entity_id, EDGE_CHILD_OF),
                )?;
            }
            // If parent not found yet, that's OK - we'll link later or leave unlinked
        }

        // Create RESPONDS_TO edges for tool_results (links to the tool_use events)
        for tool_result in &event.tool_results {
            if let Some(&target_entity_id) = self.tool_use_index.get(&tool_result.tool_use_id) {
                EdgeStore::create(
                    &mut tx,
                    &self.id_gen,
                    entity_id,
                    target_entity_id,
                    EDGE_RESPONDS_TO,
                    |id| Edge::new(id, entity_id, target_entity_id, EDGE_RESPONDS_TO),
                )?;
            }
        }

        tx.commit()?;
        Ok(entity_id)
    }

    /// Get an event by its UUID.
    pub fn get_event_by_uuid(&self, uuid: &str) -> Result<Option<Entity>> {
        if let Some(&entity_id) = self.uuid_index.get(uuid) {
            let tx = self.engine.begin_read()?;
            Ok(NodeStore::get(&tx, entity_id)?)
        } else {
            Ok(None)
        }
    }

    /// Get events for a session.
    pub fn get_events_for_session(&self, session_id: &str) -> Result<Vec<Entity>> {
        let tx = self.engine.begin_read()?;
        let event_ids = NodeStore::find_by_label(&tx, &LABEL_EVENT.into())?;

        let mut events = Vec::new();
        for entity_id in event_ids {
            if let Some(entity) = NodeStore::get(&tx, entity_id)? {
                if let Some(Value::String(sid)) = entity.properties.get("session_id") {
                    if sid == session_id {
                        events.push(entity);
                    }
                }
            }
        }

        Ok(events)
    }

    /// Get child events (events that have this event as parent via tool_use_id).
    pub fn get_child_events(&self, entity_id: EntityId) -> Result<Vec<Entity>> {
        let tx = self.engine.begin_read()?;

        // Get incoming CHILD_OF edges (children point to parent)
        let edges = EdgeStore::get_incoming(&tx, entity_id)?;

        let mut children = Vec::new();
        for edge in edges {
            if edge.edge_type == EDGE_CHILD_OF.into() {
                if let Some(child) = NodeStore::get(&tx, edge.source)? {
                    children.push(child);
                }
            }
        }

        Ok(children)
    }

    /// Create a Fork entity.
    pub fn create_fork(
        &mut self,
        fork_id: &str,
        parent_session_id: Option<&str>,
        status: &str,
        fork_name: Option<&str>,
    ) -> Result<EntityId> {
        let mut tx = self.engine.begin_write()?;

        let entity = NodeStore::create(&mut tx, &self.id_gen, |id| {
            let mut e = Entity::new(id)
                .with_label(LABEL_FORK)
                .with_property("fork_id", Value::String(fork_id.to_string()))
                .with_property("status", Value::String(status.to_string()))
                .with_property("read", Value::Bool(false))
                .with_property("created_at", Value::String(chrono::Utc::now().to_rfc3339()));

            if let Some(pid) = parent_session_id {
                e = e.with_property("parent_session_id", Value::String(pid.to_string()));
            }

            if let Some(name) = fork_name {
                e = e.with_property("fork_name", Value::String(name.to_string()));
            }

            e
        })?;

        tx.commit()?;
        Ok(entity.id)
    }

    /// Update fork status and optionally set session_id.
    pub fn update_fork_status(
        &self,
        fork_id: &str,
        status: &str,
        session_id: Option<&str>,
    ) -> Result<()> {
        let tx = self.engine.begin_read()?;
        let fork_ids = NodeStore::find_by_label(&tx, &LABEL_FORK.into())?;
        drop(tx);

        for entity_id in fork_ids {
            let tx = self.engine.begin_read()?;
            if let Some(entity) = NodeStore::get(&tx, entity_id)? {
                if let Some(Value::String(fid)) = entity.properties.get("fork_id") {
                    if fid == fork_id {
                        drop(tx);
                        let mut tx = self.engine.begin_write()?;
                        let mut updated = entity.clone();
                        updated
                            .properties
                            .insert("status".to_string(), Value::String(status.to_string()));
                        if let Some(sid) = session_id {
                            updated
                                .properties
                                .insert("session_id".to_string(), Value::String(sid.to_string()));
                        }
                        if status == "completed" || status == "failed" {
                            updated.properties.insert(
                                "completed_at".to_string(),
                                Value::String(chrono::Utc::now().to_rfc3339()),
                            );
                        }
                        NodeStore::update(&mut tx, &updated)?;
                        tx.commit()?;
                        return Ok(());
                    }
                }
            }
        }

        Ok(())
    }

    /// List all forks.
    pub fn list_forks(&self) -> Result<Vec<Entity>> {
        let tx = self.engine.begin_read()?;
        let fork_ids = NodeStore::find_by_label(&tx, &LABEL_FORK.into())?;

        let mut forks = Vec::new();
        for entity_id in fork_ids {
            if let Some(entity) = NodeStore::get(&tx, entity_id)? {
                forks.push(entity);
            }
        }

        // Sort by created_at descending
        forks.sort_by(|a, b| {
            let a_time = a
                .properties
                .get("created_at")
                .and_then(|v| match v {
                    Value::String(s) => Some(s.as_str()),
                    _ => None,
                })
                .unwrap_or("");
            let b_time = b
                .properties
                .get("created_at")
                .and_then(|v| match v {
                    Value::String(s) => Some(s.as_str()),
                    _ => None,
                })
                .unwrap_or("");
            b_time.cmp(a_time)
        });

        Ok(forks)
    }

    /// Get a fork by its fork_id.
    pub fn get_fork(&self, fork_id: &str) -> Result<Option<Entity>> {
        let tx = self.engine.begin_read()?;
        let fork_ids = NodeStore::find_by_label(&tx, &LABEL_FORK.into())?;

        for entity_id in fork_ids {
            if let Some(entity) = NodeStore::get(&tx, entity_id)? {
                if let Some(Value::String(fid)) = entity.properties.get("fork_id") {
                    if fid == fork_id {
                        return Ok(Some(entity));
                    }
                }
            }
        }

        Ok(None)
    }

    /// Get the most recent fork.
    pub fn get_latest_fork(&self) -> Result<Option<Entity>> {
        let forks = self.list_forks()?;
        Ok(forks.into_iter().next())
    }

    /// Mark a fork as read.
    pub fn mark_fork_read(&self, fork_id: &str) -> Result<()> {
        if let Some(entity) = self.get_fork(fork_id)? {
            let mut tx = self.engine.begin_write()?;
            let mut updated = entity;
            updated
                .properties
                .insert("read".to_string(), Value::Bool(true));
            NodeStore::update(&mut tx, &updated)?;
            tx.commit()?;
        }
        Ok(())
    }

    /// Mark all forks as read.
    pub fn mark_all_forks_read(&self) -> Result<usize> {
        let forks = self.list_forks()?;
        let mut count = 0;

        for entity in forks {
            if let Some(Value::Bool(false)) = entity.properties.get("read") {
                let mut tx = self.engine.begin_write()?;
                let mut updated = entity;
                updated
                    .properties
                    .insert("read".to_string(), Value::Bool(true));
                NodeStore::update(&mut tx, &updated)?;
                tx.commit()?;
                count += 1;
            }
        }

        Ok(count)
    }

    /// Create a Session entity linked to a fork.
    pub fn create_session(
        &mut self,
        session_id: &str,
        fork_entity_id: EntityId,
    ) -> Result<EntityId> {
        let mut tx = self.engine.begin_write()?;

        let entity = NodeStore::create(&mut tx, &self.id_gen, |id| {
            Entity::new(id)
                .with_label(LABEL_SESSION)
                .with_property("session_id", Value::String(session_id.to_string()))
                .with_property("created_at", Value::String(chrono::Utc::now().to_rfc3339()))
        })?;

        // Link fork to session
        EdgeStore::create(
            &mut tx,
            &self.id_gen,
            fork_entity_id,
            entity.id,
            EDGE_HAS_SESSION,
            |id| Edge::new(id, fork_entity_id, entity.id, EDGE_HAS_SESSION),
        )?;

        tx.commit()?;
        Ok(entity.id)
    }

    /// Create a Job entity linked to a fork.
    pub fn create_job(
        &mut self,
        job_id: &str,
        description: &str,
        fork_entity_id: EntityId,
    ) -> Result<EntityId> {
        let mut tx = self.engine.begin_write()?;

        let entity = NodeStore::create(&mut tx, &self.id_gen, |id| {
            Entity::new(id)
                .with_label(LABEL_JOB)
                .with_property("job_id", Value::String(job_id.to_string()))
                .with_property("description", Value::String(description.to_string()))
                .with_property("status", Value::String("running".to_string()))
                .with_property("created_at", Value::String(chrono::Utc::now().to_rfc3339()))
        })?;

        // Link fork to job
        EdgeStore::create(
            &mut tx,
            &self.id_gen,
            fork_entity_id,
            entity.id,
            EDGE_HAS_JOB,
            |id| Edge::new(id, fork_entity_id, entity.id, EDGE_HAS_JOB),
        )?;

        tx.commit()?;
        Ok(entity.id)
    }

    /// Get the underlying engine for advanced queries.
    pub fn engine(&self) -> &Arc<RedbEngine> {
        &self.engine
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn test_db() -> GraphDatabase {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.redb");
        GraphDatabase::open_at(&path).unwrap()
    }

    #[test]
    fn test_store_event() {
        let mut db = test_db();
        let event =
            ClaudeEvent::parse(r#"{"type":"assistant","uuid":"test-uuid","session_id":"sess-1"}"#)
                .unwrap();

        let entity_id = db.store_event(&event, Some("fork-123")).unwrap();
        assert!(entity_id.as_u64() > 0);

        let retrieved = db.get_event_by_uuid("test-uuid").unwrap();
        assert!(retrieved.is_some());
    }

    #[test]
    fn test_fork_lifecycle() {
        let mut db = test_db();

        let fork_id = db
            .create_fork("fork-1", Some("parent-sess"), "running")
            .unwrap();
        assert!(fork_id.as_u64() > 0);

        let fork = db.get_fork("fork-1").unwrap().unwrap();
        assert_eq!(
            fork.properties.get("status"),
            Some(&Value::String("running".to_string()))
        );

        db.update_fork_status("fork-1", "completed", None).unwrap();

        let fork = db.get_fork("fork-1").unwrap().unwrap();
        assert_eq!(
            fork.properties.get("status"),
            Some(&Value::String("completed".to_string()))
        );
    }
}
