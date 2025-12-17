---
allowed-tools: Bash(forky:*)
argument-hint: [-m haiku|sonnet|opus] <message>
description: Fork current session to handle a side task in the background
---

# Fork Current Session

Fork the current Claude session into a background process to handle a side task.

**Usage:** Arguments are passed directly to forky CLI.

```
/forky "Your task message here"
/forky -m haiku "Simple task for lighter model"
/forky fork-me "Explicit fork command"
```

## Execute Fork

!`${CLAUDE_PLUGIN_ROOT}/bin/forky $ARGUMENTS`

The fork runs detached and completes independently while you continue here.

**Options:**
- `-m haiku|sonnet|opus` - Model selection (must come before message)
- `-j "description"` - Track as a job
- `-l` - Message the last fork instead of creating new

**Management:**
- `forky list forks` - Show all forks and their status
- `forky messages <id>` - View fork results
- `forky read <id>` - Mark fork as read
