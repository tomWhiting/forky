# /forky - Spawn Parallel Claude Sessions

Spawn a forked Claude session to handle a task in parallel.

## Usage

```
/forky <task description>
```

## What This Does

1. Creates a new Claude session running in parallel
2. Injects callback instructions for completion notification
3. Returns immediately so you can continue working
4. Fork notifies you when complete

## Examples

```
/forky Review the authentication module for security issues
/forky Generate comprehensive test data for the user model
/forky Document the API endpoints in src/api/
```

## Options

After `/forky`, Claude will spawn the task. If you need special options, use the skill directly with flags:

- `--worktree` - Run in isolated git worktree (prevents file conflicts)
- `--dir <path>` - Run in specific directory

## Notes

- Opus is the default model. Ask the user before using alternatives.
- Forks run independently - don't wait for them unless explicitly asked
- Monitor forks via `forky serve` or `forky list forks`
