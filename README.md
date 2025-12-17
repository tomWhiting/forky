# forky

> *"I'm not a real fork. I was made from trash."*

A Claude Code plugin for spawning parallel sessions. Forky is held together with pipe cleaners and determination—a hastily assembled tool that somehow works, letting you offload tasks to background Claude instances while you keep moving.

Born from the need to do two things at once without losing your train of thought.

## Installation

```bash
/plugin marketplace add tomWhiting/mod-claude
/plugin install forky@mod-claude
```

## Usage

Throw a task over your shoulder and let a fork handle it:

```bash
/forky "Run the tests and fix any failures"
```

For the small stuff, use a lighter model:

```bash
/forky -m haiku "Summarize what this module does"
```

### Commands

| Command | What it does |
|---------|--------------|
| `/forky <message>` | Fork current session with a task |
| `/forky -m haiku\|sonnet\|opus <message>` | Pick your model |
| `/forky -l <message>` | Talk to the last fork you made |
| `/forky fork-me <message>` | Same thing, more explicit |

### Checking on your forks

They're out there, working. Or done. Or failed. Find out:

```bash
forky list forks      # What's happening
forky messages <id>   # What did it say
forky read <id>       # Mark it read so it stops haunting you
forky read --all      # Nuclear option
```

## How It Works

1. `/forky` spawns a detached Claude CLI process
2. The fork gets context that it's handling a side task
3. It runs independently—you don't wait for it
4. Results land in a local SQLite database when it's done

All data lives in `~/.forky/forky.db`.

## The Hook

Forky includes a `PreToolUse` hook that injects the current session ID before any forky command runs. This is how the fork knows where it came from. It writes to `/tmp/.forky-session` and forky reads from there.

## Building

If you're hacking on the source:

```bash
bun build --compile --minify ./src/cli.ts --outfile bin/forky
```

## License

MIT

---

*Made with googly eyes and existential uncertainty.*
