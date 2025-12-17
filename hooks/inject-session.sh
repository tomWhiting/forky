#!/bin/bash
#
# PreToolUse hook for forky
# Injects the current Claude session ID before forky commands run
#

# Only act on Bash commands that contain "forky"
if [ "$TOOL_NAME" = "Bash" ]; then
  # Check if command contains forky (read from stdin which has the tool input JSON)
  INPUT=$(cat)
  if echo "$INPUT" | grep -q "forky"; then
    # Write session ID to temp file that forky will read
    echo "$CLAUDE_SESSION_ID" > /tmp/.forky-session
  fi
fi

# Always allow the tool to proceed
exit 0
