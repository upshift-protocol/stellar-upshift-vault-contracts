#!/usr/bin/env bash
# Open, or update, the single open alert issue for a label.
#
# Used by the scheduled workflows (mainnet-monitor, nightly). Only issues opened
# by, and comments written by, the github-actions bot are trusted: anyone can
# comment on a public repo, so an outsider must not be able to pre-post a dedup
# marker and mute an alert.
#
# Usage: upsert-issue.sh LABEL COLOR TITLE BODY [MARKER]
#   No open issue      -> create it (assigned to $ALERT_ASSIGNEES if set).
#   Open, no MARKER    -> comment BODY on it.
#   Open, MARKER new   -> comment BODY (+ re-assign): a new, distinct alert.
#   Open, MARKER seen  -> do nothing.
# Writes action=created|commented|seen and url=<issue url> to $GITHUB_OUTPUT.
# Requires GH_TOKEN and GH_REPO.
set -euo pipefail

label=$1 color=$2 title=$3 body=$4 marker=${5:-}
assignees=${ALERT_ASSIGNEES:-}
bot="github-actions"
out=${GITHUB_OUTPUT:-/dev/stdout}

[ -n "$marker" ] && body=$(printf '%s\n\n<!-- %s -->\n' "$body" "$marker")

gh label create "$label" --color "$color" 2>/dev/null || true

existing=$(gh issue list --label "$label" --state open --author "app/$bot" \
  --json number --jq '.[0].number // empty')

if [ -z "$existing" ]; then
  url=$(gh issue create --title "$title" --label "$label" \
    ${assignees:+--assignee "$assignees"} --body "$body")
  printf 'action=created\nurl=%s\n' "$url" >> "$out"
  exit 0
fi

url=$(gh issue view "$existing" --json url --jq .url)
if [ -n "$marker" ]; then
  seen=$(gh issue view "$existing" --json body,comments --jq \
    "[.body, (.comments[] | select(.author.login == \"$bot\") | .body)]
     | map(select(contains(\"<!-- $marker -->\"))) | length")
  if [ "$seen" -gt 0 ]; then
    printf 'action=seen\nurl=%s\n' "$url" >> "$out"
    exit 0
  fi
fi

gh issue comment "$existing" --body "$body" >/dev/null
[ -n "$assignees" ] && gh issue edit "$existing" --add-assignee "$assignees" >/dev/null
printf 'action=commented\nurl=%s\n' "$url" >> "$out"
