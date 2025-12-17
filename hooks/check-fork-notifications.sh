#!/bin/bash
# Check for fork completion notifications for the current session

NOTIF_FILE="$HOME/.forky/notifications/${CLAUDE_SESSION_ID}.txt"

if [ -f "$NOTIF_FILE" ]; then
    # Read notifications
    NOTIFICATIONS=$(cat "$NOTIF_FILE")

    if [ -n "$NOTIFICATIONS" ]; then
        # Clear the file after reading
        rm "$NOTIF_FILE"

        # Output as JSON with additional context for Claude
        # Format: fork_id|timestamp|summary
        CONTEXT=""
        while IFS='|' read -r fork_id timestamp summary; do
            if [ -n "$fork_id" ]; then
                CONTEXT="${CONTEXT}Fork ${fork_id} completed at ${timestamp}: ${summary}\n"
            fi
        done <<< "$NOTIFICATIONS"

        if [ -n "$CONTEXT" ]; then
            cat << EOF
{
  "decision": "block",
  "reason": "Your forked session(s) completed. Please acknowledge:\n${CONTEXT}"
}
EOF
        fi
    fi
fi

exit 0
