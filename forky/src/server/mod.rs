//! Forky server - single global server managing multiple project databases.
//!
//! Architecture:
//! - One global server runs at ~/.forky (manages PID/port files)
//! - Each project gets its own ManifoldDB at <project>/.claude/mod-claude/forky.redb
//! - All DB access goes through the server to avoid lock contention
//! - CLI is a thin client that talks to the server via HTTP
//!
//! Endpoints:
//! - POST /api/events - Store events (requires project_path)
//! - GET /api/events - Query events
//! - POST /api/forks - Create a fork
//! - GET /api/forks - List forks
//! - PATCH /api/forks/:id - Update fork status
//! - WS /ws - WebSocket for real-time updates
//! - GET / - Dashboard UI

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use axum::{
    extract::{Path, Query, State, WebSocketUpgrade},
    http::StatusCode,
    response::{Html, IntoResponse},
    routing::{get, patch, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, RwLock};

use crate::claude::ClaudeEvent;
use crate::db::GraphDatabase;

/// Server configuration file paths.
const SERVER_DIR: &str = ".forky";
const PID_FILE: &str = "server.pid";
const PORT_FILE: &str = "server.port";

/// Database manager - handles multiple project databases.
pub struct DatabaseManager {
    /// Map of project_path -> GraphDatabase
    databases: HashMap<PathBuf, GraphDatabase>,
}

impl DatabaseManager {
    pub fn new() -> Self {
        Self {
            databases: HashMap::new(),
        }
    }

    /// Get or create database for a project path.
    pub fn get_or_create(&mut self, project_path: &PathBuf) -> Result<&mut GraphDatabase> {
        if !self.databases.contains_key(project_path) {
            let db_path = project_path
                .join(".claude")
                .join("mod-claude")
                .join("forky.redb");
            std::fs::create_dir_all(db_path.parent().unwrap())?;
            let db = GraphDatabase::open_at(&db_path)
                .with_context(|| format!("Failed to open database at {}", db_path.display()))?;
            self.databases.insert(project_path.clone(), db);
        }
        Ok(self.databases.get_mut(project_path).unwrap())
    }

    /// Get database for a project (if exists).
    pub fn get(&self, project_path: &PathBuf) -> Option<&GraphDatabase> {
        self.databases.get(project_path)
    }

    /// List all active projects.
    pub fn list_projects(&self) -> Vec<PathBuf> {
        self.databases.keys().cloned().collect()
    }
}

/// Shared server state.
pub struct ServerState {
    /// Database manager (handles multiple project DBs).
    db_manager: RwLock<DatabaseManager>,
    /// Broadcast channel for real-time updates.
    tx: broadcast::Sender<EventBroadcast>,
}

/// Event broadcast message.
#[derive(Clone, Debug, Serialize)]
pub struct EventBroadcast {
    /// Project path.
    pub project_path: String,
    /// The event that was stored.
    pub event: StoredEvent,
    /// Fork ID if known.
    pub fork_id: Option<String>,
}

/// Stored event (full data for API).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StoredEvent {
    pub fork_id: Option<String>,
    pub uuid: Option<String>,
    pub session_id: Option<String>,
    pub parent_tool_use_id: Option<String>,
    pub event_type: String,
    pub subtype: Option<String>,
    pub message: Option<String>,
    pub thinking: Option<String>,
    pub result: Option<String>,
    pub model: Option<String>,
    pub message_id: Option<String>,
    pub role: Option<String>,
    pub tool_uses: Option<serde_json::Value>,
    pub tool_results: Option<serde_json::Value>,
    pub cost_usd: Option<f64>,
    pub total_cost_usd: Option<f64>,
    pub duration_ms: Option<u64>,
    pub num_turns: Option<u32>,
    pub raw: Option<serde_json::Value>,
}

impl StoredEvent {
    pub fn from_event(e: &ClaudeEvent, fork_id: Option<&str>) -> Self {
        Self {
            fork_id: fork_id.map(String::from),
            uuid: e.uuid.clone(),
            session_id: e.session_id.clone(),
            parent_tool_use_id: e.parent_tool_use_id.clone(),
            event_type: e.type_label().to_string(),
            subtype: e.subtype.clone(),
            message: e.message.clone(),
            thinking: e.thinking.clone(),
            result: e.result.clone(),
            model: e.model.clone(),
            message_id: e.message_id.clone(),
            role: e.role.clone(),
            tool_uses: if e.tool_uses.is_empty() {
                None
            } else {
                serde_json::to_value(&e.tool_uses).ok()
            },
            tool_results: if e.tool_results.is_empty() {
                None
            } else {
                serde_json::to_value(&e.tool_results).ok()
            },
            cost_usd: e.cost_usd,
            total_cost_usd: e.total_cost_usd,
            duration_ms: e.duration_ms,
            num_turns: e.num_turns,
            raw: Some(e.raw.clone()),
        }
    }
}

// === Request/Response Types ===

/// Request to ingest events (now requires project_path).
#[derive(Debug, Deserialize)]
pub struct IngestRequest {
    /// Project path (required for routing to correct DB).
    pub project_path: String,
    /// Fork ID these events belong to.
    pub fork_id: Option<String>,
    /// Events to store.
    pub events: Vec<serde_json::Value>,
}

#[derive(Debug, Serialize)]
pub struct IngestResponse {
    pub stored: usize,
    pub errors: usize,
}

/// Request to create a fork.
#[derive(Debug, Deserialize)]
pub struct CreateForkRequest {
    pub project_path: String,
    pub fork_id: String,
    pub parent_session_id: Option<String>,
    pub job_description: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct CreateForkResponse {
    pub fork_id: String,
    pub success: bool,
}

/// Request to update fork status.
#[derive(Debug, Deserialize)]
pub struct UpdateForkRequest {
    pub project_path: String,
    pub status: String,
    pub session_id: Option<String>,
}

/// Fork summary for listing.
#[derive(Debug, Serialize)]
pub struct ForkSummary {
    pub project_path: String,
    pub fork_id: String,
    pub session_id: Option<String>,
    pub parent_session_id: Option<String>,
    pub status: String,
    pub event_count: usize,
    pub created_at: Option<String>,
}

/// Query parameters for events/forks.
#[derive(Debug, Deserialize)]
pub struct QueryParams {
    pub project_path: Option<String>,
    pub session: Option<String>,
    pub fork_id: Option<String>,
    pub limit: Option<usize>,
}

// === Server Lifecycle ===

/// Start the server.
pub async fn start_server(port: u16, open_browser: bool) -> Result<()> {
    let server_dir = get_server_dir()?;
    std::fs::create_dir_all(&server_dir)?;

    let pid = std::process::id();
    std::fs::write(server_dir.join(PID_FILE), pid.to_string())?;
    std::fs::write(server_dir.join(PORT_FILE), port.to_string())?;

    let (tx, _rx) = broadcast::channel(1000);

    let state = Arc::new(ServerState {
        db_manager: RwLock::new(DatabaseManager::new()),
        tx,
    });

    let app = Router::new()
        .route("/", get(index_handler))
        .route("/api/events", post(ingest_events))
        .route("/api/events", get(query_events))
        .route("/api/forks", post(create_fork))
        .route("/api/forks", get(list_forks))
        .route("/api/forks/{fork_id}", patch(update_fork))
        .route("/api/forks/{fork_id}", get(get_fork))
        .route("/api/projects", get(list_projects))
        .route("/ws", get(websocket_handler))
        .with_state(state);

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    println!("Forky server starting on http://{addr}");
    println!("Managing databases for all projects");

    if open_browser {
        let _ = open::that(format!("http://{addr}"));
    }

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await.context("Server error")?;

    let _ = std::fs::remove_file(server_dir.join(PID_FILE));
    let _ = std::fs::remove_file(server_dir.join(PORT_FILE));

    Ok(())
}

fn get_server_dir() -> Result<PathBuf> {
    let home = dirs::home_dir().context("Could not find home directory")?;
    Ok(home.join(SERVER_DIR))
}

pub fn get_server_port() -> Option<u16> {
    let server_dir = get_server_dir().ok()?;
    let pid_file = server_dir.join(PID_FILE);
    let port_file = server_dir.join(PORT_FILE);

    if let Ok(pid_str) = std::fs::read_to_string(&pid_file) {
        if let Ok(pid) = pid_str.trim().parse::<u32>() {
            #[cfg(unix)]
            {
                use std::process::Command;
                let output = Command::new("kill").args(["-0", &pid.to_string()]).output();
                if output.map(|o| o.status.success()).unwrap_or(false) {
                    if let Ok(port_str) = std::fs::read_to_string(&port_file) {
                        return port_str.trim().parse().ok();
                    }
                }
            }
            #[cfg(not(unix))]
            {
                if let Ok(port_str) = std::fs::read_to_string(&port_file) {
                    return port_str.trim().parse().ok();
                }
            }
        }
    }
    None
}

pub fn spawn_server_daemon(port: u16) -> Result<()> {
    use std::process::{Command, Stdio};

    let exe = std::env::current_exe()?;

    #[cfg(unix)]
    {
        Command::new(&exe)
            .args(["serve", "--port", &port.to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .context("Failed to spawn server daemon")?;
    }

    #[cfg(not(unix))]
    {
        Command::new(&exe)
            .args(["serve", "--port", &port.to_string()])
            .spawn()
            .context("Failed to spawn server daemon")?;
    }

    std::thread::sleep(std::time::Duration::from_millis(500));
    Ok(())
}

pub fn ensure_server_running() -> Result<u16> {
    if let Some(port) = get_server_port() {
        return Ok(port);
    }

    let port = 58231;
    spawn_server_daemon(port)?;

    for _ in 0..20 {
        if let Some(p) = get_server_port() {
            return Ok(p);
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    anyhow::bail!("Server failed to start")
}

// === Handlers ===

async fn index_handler() -> Html<&'static str> {
    Html(include_str!("ui.html"))
}

async fn ingest_events(
    State(state): State<Arc<ServerState>>,
    Json(req): Json<IngestRequest>,
) -> Result<Json<IngestResponse>, StatusCode> {
    let project_path = PathBuf::from(&req.project_path);
    let mut db_manager = state.db_manager.write().await;

    let db = db_manager
        .get_or_create(&project_path)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut stored = 0;
    let mut errors = 0;
    let fork_id = req.fork_id.as_deref();

    for event_json in &req.events {
        let json_str = serde_json::to_string(event_json).unwrap_or_default();
        if let Some(event) = ClaudeEvent::parse(&json_str) {
            match db.store_event(&event, fork_id) {
                Ok(_) => {
                    stored += 1;
                    let _ = state.tx.send(EventBroadcast {
                        project_path: req.project_path.clone(),
                        event: StoredEvent::from_event(&event, fork_id),
                        fork_id: req.fork_id.clone(),
                    });
                }
                Err(_) => errors += 1,
            }
        } else {
            errors += 1;
        }
    }

    Ok(Json(IngestResponse { stored, errors }))
}

async fn create_fork(
    State(state): State<Arc<ServerState>>,
    Json(req): Json<CreateForkRequest>,
) -> Result<Json<CreateForkResponse>, StatusCode> {
    use manifoldb_core::Value;

    let project_path = PathBuf::from(&req.project_path);
    let mut db_manager = state.db_manager.write().await;

    let db = db_manager
        .get_or_create(&project_path)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Create fork entity in the graph
    db.create_fork(&req.fork_id, req.parent_session_id.as_deref(), "running")
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(CreateForkResponse {
        fork_id: req.fork_id,
        success: true,
    }))
}

async fn update_fork(
    State(state): State<Arc<ServerState>>,
    Path(fork_id): Path<String>,
    Json(req): Json<UpdateForkRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let project_path = PathBuf::from(&req.project_path);
    let db_manager = state.db_manager.read().await;

    let db = db_manager
        .get(&project_path)
        .ok_or(StatusCode::NOT_FOUND)?;

    db.update_fork_status(&fork_id, &req.status)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(serde_json::json!({"success": true})))
}

async fn get_fork(
    State(state): State<Arc<ServerState>>,
    Path(fork_id): Path<String>,
    Query(params): Query<QueryParams>,
) -> Result<Json<Option<ForkSummary>>, StatusCode> {
    use manifoldb_core::Value;

    let project_path = params
        .project_path
        .as_ref()
        .map(PathBuf::from)
        .ok_or(StatusCode::BAD_REQUEST)?;

    let db_manager = state.db_manager.read().await;
    let db = db_manager.get(&project_path).ok_or(StatusCode::NOT_FOUND)?;

    let fork = db.get_fork(&fork_id).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let summary = fork.map(|f| {
        let get_str = |key: &str| -> Option<String> {
            f.properties.get(key).and_then(|v| match v {
                Value::String(s) => Some(s.clone()),
                _ => None,
            })
        };

        ForkSummary {
            project_path: project_path.to_string_lossy().to_string(),
            fork_id: get_str("fork_id").unwrap_or_default(),
            session_id: get_str("session_id"),
            parent_session_id: get_str("parent_session_id"),
            status: get_str("status").unwrap_or_else(|| "unknown".to_string()),
            event_count: 0,
            created_at: get_str("created_at"),
        }
    });

    Ok(Json(summary))
}

async fn list_forks(
    State(state): State<Arc<ServerState>>,
    Query(params): Query<QueryParams>,
) -> Result<Json<Vec<ForkSummary>>, StatusCode> {
    use manifoldb_core::Value;
    use manifoldb_graph::store::NodeStore;
    use manifoldb_storage::StorageEngine;

    let db_manager = state.db_manager.read().await;
    let mut all_forks = Vec::new();

    // If project_path specified, only query that project
    let projects: Vec<PathBuf> = if let Some(ref p) = params.project_path {
        vec![PathBuf::from(p)]
    } else {
        db_manager.list_projects()
    };

    for project_path in projects {
        let Some(db) = db_manager.get(&project_path) else {
            continue;
        };

        let tx = db
            .engine()
            .begin_read()
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        // First, collect Fork entities
        let mut forks_map: HashMap<String, ForkSummary> = HashMap::new();

        NodeStore::for_each(&tx, |entity| {
            let is_fork = entity.labels.iter().any(|l| l.as_str() == "Fork");
            if is_fork {
                let get_str = |key: &str| -> Option<String> {
                    entity.properties.get(key).and_then(|v| match v {
                        Value::String(s) => Some(s.clone()),
                        _ => None,
                    })
                };

                if let Some(fork_id) = get_str("fork_id") {
                    forks_map.insert(
                        fork_id.clone(),
                        ForkSummary {
                            project_path: project_path.to_string_lossy().to_string(),
                            fork_id,
                            session_id: get_str("session_id"),
                            parent_session_id: get_str("parent_session_id"),
                            status: get_str("status").unwrap_or_else(|| "running".to_string()),
                            event_count: 0,
                            created_at: get_str("created_at"),
                        },
                    );
                }
            }
            true
        })
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        // Count events per fork
        NodeStore::for_each(&tx, |entity| {
            let is_event = entity.labels.iter().any(|l| l.as_str() == "Event");
            if is_event {
                if let Some(Value::String(fork_id)) = entity.properties.get("fork_id") {
                    if let Some(fork) = forks_map.get_mut(fork_id) {
                        fork.event_count += 1;
                    }
                }
            }
            true
        })
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        all_forks.extend(forks_map.into_values());
    }

    // Sort by created_at descending
    all_forks.sort_by(|a, b| {
        let a_time = a.created_at.as_deref().unwrap_or("");
        let b_time = b.created_at.as_deref().unwrap_or("");
        b_time.cmp(a_time)
    });

    Ok(Json(all_forks))
}

async fn query_events(
    State(state): State<Arc<ServerState>>,
    Query(params): Query<QueryParams>,
) -> Result<Json<Vec<StoredEvent>>, StatusCode> {
    use manifoldb_core::Value;
    use manifoldb_graph::store::NodeStore;
    use manifoldb_storage::StorageEngine;

    let project_path = params
        .project_path
        .as_ref()
        .map(PathBuf::from)
        .ok_or(StatusCode::BAD_REQUEST)?;

    let db_manager = state.db_manager.read().await;
    let db = db_manager.get(&project_path).ok_or(StatusCode::NOT_FOUND)?;

    let tx = db
        .engine()
        .begin_read()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let limit = params.limit.unwrap_or(100);
    let mut events = Vec::new();

    NodeStore::for_each(&tx, |entity| {
        let is_event = entity.labels.iter().any(|l| l.as_str() == "Event");
        if !is_event {
            return true;
        }

        // Apply fork_id filter
        if let Some(ref filter) = params.fork_id {
            let fork = entity.properties.get("fork_id").and_then(|v| match v {
                Value::String(s) => Some(s.as_str()),
                _ => None,
            });
            if fork != Some(filter.as_str()) {
                return true;
            }
        }

        // Apply session filter
        if let Some(ref filter) = params.session {
            let session = entity.properties.get("session_id").and_then(|v| match v {
                Value::String(s) => Some(s.as_str()),
                _ => None,
            });
            if session.map(|s| !s.starts_with(filter)).unwrap_or(true) {
                return true;
            }
        }

        let get_str = |key: &str| -> Option<String> {
            entity.properties.get(key).and_then(|v| match v {
                Value::String(s) => Some(s.clone()),
                _ => None,
            })
        };
        let get_int = |key: &str| -> Option<i64> {
            entity.properties.get(key).and_then(|v| match v {
                Value::Int(i) => Some(*i),
                _ => None,
            })
        };
        let get_float = |key: &str| -> Option<f64> {
            entity.properties.get(key).and_then(|v| match v {
                Value::Float(f) => Some(*f),
                _ => None,
            })
        };

        let event = StoredEvent {
            fork_id: get_str("fork_id"),
            uuid: get_str("uuid"),
            session_id: get_str("session_id"),
            parent_tool_use_id: get_str("parent_tool_use_id"),
            event_type: get_str("type").unwrap_or_else(|| "unknown".to_string()),
            subtype: get_str("subtype"),
            message: get_str("message"),
            thinking: get_str("thinking"),
            result: get_str("result"),
            model: get_str("model"),
            message_id: get_str("message_id"),
            role: get_str("role"),
            tool_uses: get_str("tool_uses").and_then(|s| serde_json::from_str(&s).ok()),
            tool_results: get_str("tool_results").and_then(|s| serde_json::from_str(&s).ok()),
            cost_usd: get_float("cost_usd"),
            total_cost_usd: get_float("total_cost_usd"),
            duration_ms: get_int("duration_ms").map(|i| i as u64),
            num_turns: get_int("num_turns").map(|i| i as u32),
            raw: get_str("raw").and_then(|s| serde_json::from_str(&s).ok()),
        };

        events.push(event);
        events.len() < limit
    })
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(events))
}

async fn list_projects(
    State(state): State<Arc<ServerState>>,
) -> Result<Json<Vec<String>>, StatusCode> {
    let db_manager = state.db_manager.read().await;
    let projects: Vec<String> = db_manager
        .list_projects()
        .into_iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect();
    Ok(Json(projects))
}

async fn websocket_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<ServerState>>,
) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_websocket(socket, state))
}

async fn handle_websocket(mut socket: axum::extract::ws::WebSocket, state: Arc<ServerState>) {
    use axum::extract::ws::Message;

    let mut rx = state.tx.subscribe();

    while let Ok(broadcast) = rx.recv().await {
        if let Ok(json) = serde_json::to_string(&broadcast) {
            if socket.send(Message::Text(json.into())).await.is_err() {
                break;
            }
        }
    }
}
