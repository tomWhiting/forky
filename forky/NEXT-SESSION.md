# Forky - Next Session Notes

## What We Did This Session

### 1. ManifoldDB Integration (MAJOR)
Integrated ManifoldDB as the graph database backend for storing Claude events:

- **Added ManifoldDB dependencies** to `Cargo.toml`:
  - `manifoldb-core` - Entity, Edge, Value types
  - `manifoldb-storage` - RedbEngine backend
  - `manifoldb-graph` - NodeStore, EdgeStore, IdGenerator

- **Created `src/db/graph.rs`** - New graph database module:
  - `GraphDatabase` struct wraps ManifoldDB RedbEngine
  - Events stored as entities with label "Event"
  - Parent-child edges via `parent_tool_use_id` → CHILD_OF edge
  - Indexes: `uuid_index` and `tool_use_index` for fast lookups
  - Fork/Session/Job CRUD operations (ready for migration)

- **Enhanced `src/claude/events.rs`**:
  - Added `uuid` and `parent_tool_use_id` fields for event chaining
  - Added `tool_use_ids` extraction from content blocks
  - New fields: `total_cost_usd`, `duration_ms`, `num_turns`
  - Stored raw JSON for full fidelity

- **Updated `src/claude/spawn.rs`**:
  - `ClaudeResult` now includes `events: Vec<ClaudeEvent>`
  - All parsed events collected during streaming

- **Integrated in `src/cli/commands.rs`**:
  - After spawn completes, all events stored in GraphDatabase
  - Silent fallback if graph DB unavailable

### 2. Previous Session Work (still in place)
All Claude CLI flags wired up:
- `--chrome` / `--no-chrome`
- `--system-prompt` / `--append-system-prompt`
- `--worktree` (stubbed)
- `--dir`
- Hidden: `--agents`, `--mcp-config`, `--settings`, `--max-turns`, `--tools`, `--allowed-tools`

UUIDv7 session IDs for time-ordered, visually distinct sessions.

## Event Graph Structure

Events form chains via `parent_tool_use_id`:

```
[Assistant Message uuid:A] ──contains──> [tool_use id:"toolu_123"]
                                              │
                                              │ CHILD_OF
                                              ▼
                         [User Message uuid:B, parent_tool_use_id:"toolu_123"]
                                              │
                                              │ (next turn)
                                              ▼
                         [Assistant Message uuid:C, parent_tool_use_id:"toolu_123"]
```

The `tool_use_index` maps tool_use IDs to the entity containing them,
enabling fast parent lookups when creating CHILD_OF edges.

## TODO (Priority Order)

### 1. Complete SQLite → ManifoldDB Migration
The GraphDatabase has all the methods ready. Need to:
- Update `execute()` to use GraphDatabase instead of Database
- Update list commands to query graph entities
- Remove SQLite dependency once verified

### 2. Worktree Support
In `run_fork()` when `opts.worktree` is true:
1. Generate branch name: `forky/<fork-id>`
2. Create branch from HEAD: `git branch forky/<fork-id>`
3. Create worktree: `git worktree add ~/.forky/worktrees/<fork-id> forky/<fork-id>`
4. Add to `add_dirs` for Claude
5. Set `working_dir` to worktree path

### 3. WebSocket Streaming Server
Architecture decided:
- `forky serve` runs as daemon (check `~/.forky/server.pid`)
- Forks POST events to server
- Browser connects via WebSocket for real-time updates
- Server writes port to `~/.forky/server.port`
- Multiple forky instances share same server

### 4. Web UI
Simple HTML/JS page:
- Connect to WebSocket
- Show live feed of fork events
- Display fork status, messages, costs
- Navigate event chains

## Key Files
- `src/cli/args.rs` - CLI argument definitions
- `src/cli/commands.rs` - Command execution, ForkOptions, GraphDatabase integration
- `src/claude/spawn.rs` - ClaudeOptions, spawn logic, event collection
- `src/claude/events.rs` - ClaudeEvent with full schema support
- `src/db/graph.rs` - ManifoldDB GraphDatabase (NEW)
- `src/db/connection.rs` - SQLite (legacy, to be removed)
- `src/db/queries.rs` - SQLite queries (legacy, to be removed)

## Related Directories
- `/Users/tom/Developer/spaces/tooling/claude-plugins/forky` - Plugin repo with hooks/commands
- `/Users/tom/Developer/spaces/tooling/claude-plugins/mod-claude` - Published plugin
- `/Users/tom/Developer/spaces/manifoldb` - ManifoldDB codebase

## Build
```bash
cd /Users/tom/Developer/spaces/tooling/claude-plugins/workspace/rust
cargo build --release
./target/release/forky --help
```

## Database Location
- SQLite (legacy): `.claude/mod-claude/forky.db`
- ManifoldDB: `.claude/mod-claude/forky.redb`
