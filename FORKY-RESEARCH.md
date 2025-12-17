# Forky Plugin Research

## Overview

Forky is a Claude Code plugin that enables spawning parallel Claude sessions. It allows users to offload tasks to background Claude instances while continuing to work in their main session. The plugin maintains state in a SQLite database and uses Claude Code's hook system to inject session context.

## Installation

```bash
/plugin marketplace add tomWhiting/mod-claude
/plugin install forky@mod-claude
```

## Architecture

### Components

1. **Binary CLI** (`bin/forky`) - A compiled Bun/TypeScript application (~3.7MB) that handles all forky operations
2. **Slash Command** (`commands/forky.md`) - Defines the `/forky` command interface for Claude Code
3. **Hook** (`hooks/inject-session.sh`) - Injects the current session ID before forky commands execute
4. **SQLite Database** (`~/.forky/forky.db`) - Stores forks, sessions, jobs, and messages

### Data Flow

1. User invokes `/forky "task message"`
2. PreToolUse hook writes current session ID to `/tmp/.forky-session`
3. Forky CLI reads the session ID and spawns a detached Claude process
4. The fork runs independently in the background
5. Results are stored in the SQLite database when complete
6. User can query status and retrieve results via CLI commands

## Commands Reference

### Primary Command

```bash
/forky [OPTIONS] <message>
```

**Options:**
- `-m, --model <MODEL>` - Select model (haiku, sonnet, opus)
- `-l, --last` - Message the last fork instead of creating a new one
- `-j "description"` - Track as a job (for categorization)

**Examples:**
```bash
/forky "Run the tests and fix any failures"
/forky -m haiku "Summarize what this module does"
/forky -l "Continue with the next step"
```

### Subcommands

| Command | Description |
|---------|-------------|
| `forky fork-me <message>` | Explicit fork of current session |
| `forky fork <id> <message>` | Fork a specific session by ID |
| `forky resume <id> <message>` | Resume a specific session |
| `forky new <message>` | Start a fresh session (no forking) |
| `forky list forks` | List all forks with status |
| `forky list sessions` | List all sessions |
| `forky list jobs` | List all tracked jobs |
| `forky messages <fork_id>` | View messages for a fork |
| `forky read <fork_id>` | Mark a fork as read |
| `forky read --all` | Mark all forks as read |

## Database Schema

### Tables

**forks** - Tracks spawned fork processes
- `id` (TEXT PK) - Short unique fork ID
- `parent_session_id` (TEXT) - Session that spawned this fork
- `fork_session_id` (TEXT) - The forked Claude session ID
- `ai_provider` (TEXT) - AI provider (default: 'claude')
- `name` (TEXT) - Optional fork name
- `status` (TEXT) - Status: 'active', 'running', 'completed'
- `created_at` (TEXT) - Timestamp
- `completed_at` (TEXT) - Completion timestamp
- `read` (INTEGER) - Read status flag

**sessions** - Tracks Claude sessions
- `id` (TEXT PK) - Session UUID
- `fork_id` (TEXT FK) - Associated fork
- `created_at` (TEXT) - Timestamp

**jobs** - Tracks categorized tasks
- `id` (TEXT PK) - Short unique job ID
- `description` (TEXT) - Job description
- `status` (TEXT) - Status: 'pending', 'running', 'completed'
- `fork_id` (TEXT FK) - Associated fork
- `session_id` (TEXT FK) - Associated session
- `output` (TEXT) - Job output/result
- `created_at` / `completed_at` (TEXT) - Timestamps

**messages** - Stores conversation history
- `id` (INTEGER PK) - Auto-increment ID
- `fork_id` (TEXT FK) - Associated fork
- `session_id` (TEXT) - Session reference
- `role` (TEXT) - 'USER' or 'ASSISTANT'
- `content` (TEXT) - Message content
- `created_at` (TEXT) - Timestamp

## Hook System

### PreToolUse Hook

Located at `hooks/inject-session.sh`, this hook:

1. Triggers before any Bash tool execution
2. Checks if the command contains "forky"
3. Writes `$CLAUDE_SESSION_ID` to `/tmp/.forky-session`
4. Always exits 0 to allow the tool to proceed

This mechanism provides forky with context about which session is spawning the fork.

### Configuration (`hooks/hooks.json`)

```json
{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "Bash",
        "hooks": [
          {
            "type": "command",
            "command": "${CLAUDE_PLUGIN_ROOT}/hooks/inject-session.sh"
          }
        ]
      }
    ]
  }
}
```

## Slash Command Definition

The `/forky` command is defined in `commands/forky.md`:

- **Allowed tools:** `Bash(forky:*)` - Restricts to forky-prefixed bash commands
- **Argument hint:** `[-m haiku|sonnet|opus] <message>`
- **Execution:** `${CLAUDE_PLUGIN_ROOT}/bin/forky $ARGUMENTS`

## Usage Patterns

### Quick Background Task
```bash
/forky "Check for type errors in the project"
```

### Cost-Conscious Simple Tasks
```bash
/forky -m haiku "Generate a summary of this file"
```

### Iterative Work with a Fork
```bash
/forky "Start implementing the auth module"
# ... later ...
/forky -l "Add tests for the auth module"
```

### Monitor Progress
```bash
forky list forks      # See status of all forks
forky messages <id>   # View what a fork has been doing
forky read <id>       # Mark as read when done reviewing
```

## Best Practices

1. **Use lighter models for simple tasks** - Haiku is sufficient for summaries, explanations, and simple queries
2. **Check fork status periodically** - Use `forky list forks` to monitor background work
3. **Review results** - Use `forky messages <id>` to see what forks accomplished
4. **Mark completed forks as read** - Keeps your fork list clean with `forky read <id>`
5. **Use the `-l` flag for iterative work** - Continue conversations with the same fork instead of spawning new ones

## Technical Notes

- Data persists in `~/.forky/forky.db` (SQLite)
- The binary is compiled from TypeScript using Bun
- Forks run as detached Claude CLI processes
- Session context is passed via the `/tmp/.forky-session` temp file
- Build command: `bun build --compile --minify ./src/cli.ts --outfile bin/forky`

## Limitations

- Forks are fire-and-forget; no real-time streaming of output
- Session ID injection relies on the hook system being properly configured
- Results must be explicitly queried via CLI commands
