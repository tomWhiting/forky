#!/bin/bash
# Check for fork completion notifications

NOTIF_FILE="$HOME/.forky/notifications/pending.txt"

if [ -f "$NOTIF_FILE" ]; then
    # Read notifications
    NOTIFICATIONS=$(cat "$NOTIF_FILE")

    if [ -n "$NOTIFICATIONS" ]; then
        # Clear the file after reading
        rm "$NOTIF_FILE"

        # Build context message
        CONTEXT="📬 Fork(s) completed:\\n"
        while IFS='|' read -r fork_id timestamp summary; do
            if [ -n "$fork_id" ]; then
                CONTEXT="${CONTEXT}  • ${fork_id}: ${summary}\\n"
            fi
        done <<< "$NOTIFICATIONS"

        # Output JSON that blocks stopping with notification
        cat << EOF
{
  "decision": "block",
  "reason": "${CONTEXT}Please acknowledge these fork completions."
}
EOF
    fi
fi

exit 0
