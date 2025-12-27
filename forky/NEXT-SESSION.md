# Forky - Next Session Notes

## What We Did This Session

### 1. Added New CLI Flags
All Claude CLI flags now wired up in `src/cli/args.rs` and `src/claude/spawn.rs`:
- `--chrome` / `--no-chrome` - Browser integration
- `--system-prompt` / `--append-system-prompt` - Prompt control
- `--worktree` - Git worktree mode (stubbed, not implemented)
- `--dir` - Run in specific directory (auto-adds --add-dir)
- Hidden: `--agents`, `--mcp-config`, `--settings`, `--max-turns`, `--tools`, `--allowed-tools`, `--include-partial-messages`

### 2. UUIDv7 Session IDs
- Session IDs now generated upfront using UUIDv7 (time-ordered)
- Passed to Claude via `--session-id` flag
- No more hook-based session detection needed
- See `generate_session_id()` in `src/cli/commands.rs`

### 3. Added Dependencies
- `uuid` with v7 feature
- `axum` with websocket feature
- `tower-http` for CORS/static files
- `tokio-stream` for streaming

### 4. Serve Command Stubbed
- `forky serve --port 3847` command added
- Will start WebSocket server for observability
- Not yet implemented

## TODO (Priority Order)

### 1. ManifoldDB Integration (NEXT)
Replace SQLite with ManifoldDB in `src/db/`:
- `connection.rs` - Replace `rusqlite::Connection` with ManifoldDB
- `queries.rs` - Convert SQL queries to entity/edge operations
- Schema maps well to graph:
  - Fork → entity with edges to parent_session, child_session
  - Session → entity
  - Job → entity with edge to Fork
  - Message → entity with edge to Fork/Session

ManifoldDB path: `/Users/tom/Developer/spaces/manifoldb`

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

## Key Files
- `src/cli/args.rs` - CLI argument definitions
- `src/cli/commands.rs` - Command execution, `ForkOptions` struct
- `src/claude/spawn.rs` - `ClaudeOptions` struct, spawn logic
- `src/db/` - SQLite layer (to be replaced with ManifoldDB)

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
