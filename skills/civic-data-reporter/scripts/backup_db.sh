#!/usr/bin/env bash
# backup_db.sh — atomic SQLite backup using the .backup API + gzip + integrity check.
#
# Usage: backup_db.sh <db_path> <backup_dir>
#
# Output: <backup_dir>/tampa-<YYYY-MM-DD>.db.gz
# Appends a one-line entry to <backup_dir>/_log.md.
#
# Uses sqlite3's .backup command, which is safe while other readers/writers are
# active (it uses the SQLite online backup API). Do NOT replace with `cp`.

set -euo pipefail

if [[ $# -ne 2 ]]; then
  echo "usage: backup_db.sh <db_path> <backup_dir>" >&2
  exit 2
fi

DB_PATH="$1"
BACKUP_DIR="$2"

if [[ ! -f "$DB_PATH" ]]; then
  echo "error: db not found: $DB_PATH" >&2
  exit 3
fi

mkdir -p "$BACKUP_DIR"
DATE_STR="$(date -u +%Y-%m-%d)"
TS_STR="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
SNAPSHOT="$BACKUP_DIR/tampa-$DATE_STR.db"
GZ_PATH="$SNAPSHOT.gz"
LOG_PATH="$BACKUP_DIR/_log.md"

# 1. Atomic snapshot via .backup API.
sqlite3 "$DB_PATH" ".backup '$SNAPSHOT'"

# 2. Integrity check on snapshot before we gzip.
INTEGRITY="$(sqlite3 "$SNAPSHOT" "PRAGMA integrity_check;" | head -n1)"
if [[ "$INTEGRITY" != "ok" ]]; then
  echo "error: integrity check failed on snapshot: $INTEGRITY" >&2
  rm -f "$SNAPSHOT"
  exit 4
fi

# 3. gzip in place.
gzip -9 -f "$SNAPSHOT"

# 4. Verify gzip is non-empty.
if [[ ! -s "$GZ_PATH" ]]; then
  echo "error: gzip output empty: $GZ_PATH" >&2
  exit 5
fi

# 5. Log line.
SIZE_BYTES="$(stat -c%s "$GZ_PATH" 2>/dev/null || stat -f%z "$GZ_PATH")"
{
  if [[ ! -f "$LOG_PATH" ]]; then
    echo "# tampa.db backup log"
    echo
  fi
  echo "- $TS_STR  $(basename "$GZ_PATH")  ${SIZE_BYTES}B  integrity=ok"
} >> "$LOG_PATH"

echo "OK $GZ_PATH"
