#!/usr/bin/env bash
# init_public_views.sh — load <wiki>/_data/public-views.sql into tampa.db.
#
# Implementation choice: we apply CREATE VIEW IF NOT EXISTS statements to the
# live DB rather than maintaining a separate tampa-public.db. Datasette is
# pointed at tampa.db in readonly mode and its --metadata whitelists the
# v_*_public views (see references/public-views.md).
#
# Usage: init_public_views.sh <db_path> <public_views_sql_path>
#
# Idempotent: safe to run on every schema migration.

set -euo pipefail

if [[ $# -ne 2 ]]; then
  echo "usage: init_public_views.sh <db_path> <public_views_sql_path>" >&2
  exit 2
fi

DB_PATH="$1"
SQL_PATH="$2"

if [[ ! -f "$DB_PATH" ]]; then
  echo "error: db not found: $DB_PATH" >&2
  exit 3
fi
if [[ ! -f "$SQL_PATH" ]]; then
  echo "error: sql not found: $SQL_PATH" >&2
  exit 4
fi

sqlite3 "$DB_PATH" < "$SQL_PATH"

# List installed views for confirmation.
COUNT="$(sqlite3 "$DB_PATH" "SELECT COUNT(*) FROM sqlite_master WHERE type='view' AND name LIKE 'v_%_public';")"
echo "OK installed views: $COUNT"
sqlite3 "$DB_PATH" "SELECT name FROM sqlite_master WHERE type='view' AND name LIKE 'v_%_public' ORDER BY name;"
