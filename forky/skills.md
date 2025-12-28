---
name: forky
description: Fork Claude sessions to handle side tasks in parallel. Use when the user wants to spawn a parallel Claude session, delegate a task to a background agent, monitor forked sessions, or when they mention "forky", "fork", "spawn", or "parallel task".
---

# Forky - Parallel Claude Session Manager

Forky lets you spawn parallel Claude sessions to handle side tasks without interrupting your main workflow. Forked sessions run independently and notify you when complete.

## Core Commands

### Spawning a Fork

The primary command to create a new parallel session:

```bash
forky spawn "your task description here"
```

Or simply (spawn is the default):

```bash
forky "your task description here"
```

**Common options:**

| Flag | Description |
|------|-------------|
| `-m, --model` | Model: opus (default). Always use opus unless user explicitly requests otherwise |
| `--worktree` | Run in isolated git worktree (branch: forky/<id>) |
| `--dir <PATH>` | Directory to run the fork in |

**Examples:**

```bash
# Spawn a code review task
forky spawn "Review the authentication module for security issues"

# Model selection (opus is default - check with user before using alternatives)
# forky -m sonnet "task"  # Ask user first!

# Spawn in a worktree to avoid file conflicts
forky --worktree "Refactor the logging system"
```

### Monitoring Forks

Start the observability UI to monitor all forked sessions:

```bash
forky serve
```

Options:
- `-p, --port <PORT>`: Port to listen on (default: 58231)
- `--open`: Open browser automatically

List active forks:

```bash
forky list forks
```

View messages from a specific fork:

```bash
forky messages <fork_id>
```

### Done Callback

Forked sessions automatically call the done callback when complete. This is injected into the fork's system prompt.

**For forked agents:** When you complete your task, run:

```bash
forky done <fork_id> "brief summary of what was accomplished"
```

This:
1. Updates the fork status to "completed"
2. Writes a notification for the parent session
3. Logs completion to ~/.forky/notifications/pending.txt

## Session Management

| Command | Description |
|---------|-------------|
| `forky spawn <msg>` | Spawn new fork from current session |
| `forky fork <id> <msg>` | Fork a specific session |
| `forky resume <id> <msg>` | Resume an existing session |
| `forky new <msg>` | Start fresh session (no parent) |
| `forky -l <msg>` | Message the most recent fork |

## Architecture

- **CLI**: Thin client that communicates with the forky server via HTTP
- **Server**: Manages per-project ManifoldDB graph databases
- **Sessions**: Each fork gets a UUIDv7 session ID for ordering/uniqueness
- **Events**: All Claude events are streamed to the server for observability

## When to Use Forky

**Good use cases:**
- Code review while you continue development
- Running tests in the background
- Researching a topic while working on implementation
- Generating documentation for code you just wrote
- Any task that can run independently

**Avoid forking for:**
- Tasks that need your immediate input
- Work that modifies the same files you're editing (unless using `--worktree`)
- Very short tasks (overhead > benefit)

## Cascade Prevention

Forky includes safeguards against infinite session creation. Messages that look like forky commands are rejected to prevent cascade bugs where a forked session accidentally re-executes the spawn command.
