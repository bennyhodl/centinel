#!/usr/bin/env bash
# watch_lock.sh — advisory flock wrapper for the Watch Runner cron.
#
# Prevents overlapping runs: if a previous 4h tick is still active when the
# next one fires, this exits clean rather than piling up.
#
# Usage:
#   watch_lock.sh <command> [args...]
#
# Example:
#   watch_lock.sh python3 -m civic_watch_runner --wiki ~/wiki/Tampa
#
# Exit codes:
#   0  — wrapped command ran (success or its own non-zero passes through)
#   1  — could not acquire lock (previous run still active); message logged

set -euo pipefail

LOCKFILE="${WATCH_RUNNER_LOCKFILE:-/tmp/watch-runner.lock}"
STATUS_FILE="${WATCH_RUNNER_STATUS_FILE:-}"

if [ "$#" -lt 1 ]; then
  echo "usage: watch_lock.sh <command> [args...]" >&2
  exit 2
fi

# -n: non-blocking. -E 1: exit 1 if lock not acquired.
if ! exec 9>"$LOCKFILE"; then
  echo "watch_lock.sh: could not open lockfile $LOCKFILE" >&2
  exit 2
fi

if ! flock -n 9; then
  msg="$(date -u +%FT%TZ) watch_lock.sh: previous run still active (lock $LOCKFILE held); skipping."
  echo "$msg" >&2
  if [ -n "$STATUS_FILE" ] && [ -d "$(dirname "$STATUS_FILE")" ]; then
    echo "$msg" >> "$STATUS_FILE"
  fi
  exit 1
fi

# Lock acquired. Run the command. Lock auto-releases on exit (fd 9 close).
"$@"
