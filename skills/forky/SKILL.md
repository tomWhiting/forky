---
name: forky
description: Fork Claude sessions to handle side tasks in parallel without losing context
---

# Forky - Parallel Session Management

Forky lets you spawn parallel Claude sessions to handle side tasks while preserving your current context and focus.

## When to Use Forky

**Use forky when:**
- You need to research something but don't want to derail your current work
- Running tests, linting, or validation that takes time
- You're unsure about an approach and want to explore alternatives
- Documentation or housekeeping tasks come up mid-flow
- You need a second opinion without context-switching
- Multiple independent tasks could run in parallel

**Classic scenarios:**
```
/forky "Check the docs for X and summarize the relevant parts"
/forky "Run the full test suite and report failures"
/forky "Review this approach - am I overcomplicating it?"
/forky "Update the README to reflect these changes"
```

## Critical Rule: Never Wait for Forks

**YOU HAVE NO PERCEPTION OF TIME.** If you spawn a fork and then poll for its completion, you will check every second, burning tokens and context for nothing.

**WRONG:**
```
/forky "do something"
# checks status
# checks status again
# checks status again
# 50 more checks...
```

**RIGHT:**
```
/forky "do something"
# continues with other work
# fork notifies when done via Stop hook
```

Forks run detached. You'll be notified when they complete. Trust the system.

## How Forks Work

1. You call `/forky "task description"`
2. Fork gets a unique ID (e.g., `aotbz20v`)
3. Fork runs independently with your context
4. Fork calls `forky done <id> "summary"` when complete
5. Your Stop hook shows: "📬 Fork aotbz20v completed: summary"

## Multiple Forks (Multiple Tines)

Forks can have more than two prongs. Spawn as many as needed:

```
/forky "Research authentication options"
/forky "Check performance benchmarks"
/forky "Look for security vulnerabilities"
```

All three run in parallel. You continue working. Results come back as each completes.

## Using Forky When Stuck

When you're uncertain or facing a complex decision:

```
/forky "Explore approach A - implement a rough prototype"
/forky "Explore approach B - implement a rough prototype"
```

Let your forks do the exploration. Compare results when they return. This is cheaper than going down a wrong path yourself.

## Checking on Forks

```bash
forky list forks      # See all forks and their status
forky messages <id>   # View what a fork said/did
forky read <id>       # Mark as read when done reviewing
```

## Model Selection

Use lighter models for simple tasks:
```
/forky -m haiku "Summarize this file"
/forky -m haiku "Check for typos in the docs"
```

Reserve heavier models for complex work:
```
/forky -m opus "Design the authentication architecture"
```

## Continuing Conversations

Use `-l` to message the last fork instead of creating new:
```
/forky "Start implementing the auth module"
# later...
/forky -l "Now add the logout functionality"
```

## What Forks Are NOT For

- Tasks requiring back-and-forth clarification (they run unattended)
- Work that changes your current direction (you need to integrate it)
- Simple questions you could just answer
- Anything requiring real-time coordination

## The Fork Mindset

Think of forks as capable colleagues you can hand tasks to:
- Give clear instructions
- Let them work independently
- Check results when they're done
- Don't micromanage

You're preserving your context. They're doing the legwork. Everyone wins.
