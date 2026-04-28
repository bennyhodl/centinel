#!/usr/bin/env python3
"""
run_query.py — run a readonly SELECT against tampa.db, log methodology row.

Opens TWO connections:
  1. Readonly (mode=ro) for the SELECT itself.
  2. Read/write for the methodology insert.

Refuses any non-SELECT statement. Prints JSON to stdout with the result rows
plus the methodology id so the caller can cite it as M-<id>.

Usage:
    python run_query.py --db <path> --label <handle> --asked-by <agent>
                        ( --sql-file <path> | --sql "<inline>" )
                        [--used-in <slug>] [--notes <text>]

Stdlib only.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import re
import sqlite3
import sys
from datetime import datetime, timezone


def _is_select_only(sql: str) -> bool:
    """Cheap static check; the real defense is the readonly connection."""
    stripped = re.sub(r"--.*?$|/\*.*?\*/", "", sql, flags=re.M | re.S).strip()
    if not stripped:
        return False
    # Strip a single leading WITH ... AS (...) chain.
    head = stripped.lstrip().lower()
    if head.startswith("with"):
        # Trust the readonly connection to reject mutations; allow.
        return True
    if head.startswith("select"):
        return True
    return False


def _readonly_authorizer(action, *_):
    # Allow only SELECT-equivalent operations.
    ALLOWED = {
        sqlite3.SQLITE_SELECT,
        sqlite3.SQLITE_READ,
        sqlite3.SQLITE_FUNCTION,
        sqlite3.SQLITE_RECURSIVE,
        sqlite3.SQLITE_TRANSACTION,
    }
    if action in ALLOWED:
        return sqlite3.SQLITE_OK
    return sqlite3.SQLITE_DENY


def canonicalize_rows_for_hash(cols, rows) -> bytes:
    payload = {
        "cols": list(cols),
        "rows": [list(r) for r in rows],
    }
    return json.dumps(payload, sort_keys=True, default=str, separators=(",", ":")).encode()


def main(argv) -> int:
    p = argparse.ArgumentParser()
    p.add_argument("--db", required=True)
    p.add_argument("--label", required=True)
    p.add_argument("--asked-by", required=True)
    p.add_argument("--used-in", default=None)
    p.add_argument("--notes", default=None)
    g = p.add_mutually_exclusive_group(required=True)
    g.add_argument("--sql-file")
    g.add_argument("--sql")
    args = p.parse_args(argv[1:])

    if args.sql_file:
        with open(args.sql_file, "r", encoding="utf-8") as f:
            sql = f.read()
    else:
        sql = args.sql

    if not _is_select_only(sql):
        print(json.dumps({"error": "non-SELECT statement refused"}), file=sys.stderr)
        return 2

    # 1. Readonly execution.
    ro_uri = f"file:{args.db}?mode=ro"
    ro = sqlite3.connect(ro_uri, uri=True)
    try:
        ro.set_authorizer(_readonly_authorizer)
        try:
            cur = ro.execute(sql)
        except sqlite3.DatabaseError as e:
            print(json.dumps({"error": f"sql error: {e}"}), file=sys.stderr)
            return 3
        cols = [c[0] for c in cur.description] if cur.description else []
        rows = cur.fetchall()
    finally:
        ro.close()

    result_hash = hashlib.sha256(canonicalize_rows_for_hash(cols, rows)).hexdigest()
    queried_at = datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%S.%fZ")

    # 2. Write methodology row.
    rw = sqlite3.connect(args.db)
    try:
        rw.execute("PRAGMA journal_mode=WAL")
        rw.execute("PRAGMA foreign_keys=ON")
        with rw:
            cur = rw.execute(
                """
                INSERT INTO methodology (run_date, query_label, sql, row_count, result_hash, asked_by, used_in, notes)
                VALUES (?, ?, ?, ?, ?, ?, ?, ?)
                """,
                (queried_at, args.label, sql, len(rows), result_hash, args.asked_by, args.used_in, args.notes),
            )
            methodology_id = cur.lastrowid
    finally:
        rw.close()

    out = {
        "methodology_id": methodology_id,
        "label": args.label,
        "queried_at": queried_at,
        "row_count": len(rows),
        "result_hash": result_hash,
        "cols": cols,
        "rows": [list(r) for r in rows],
    }
    print(json.dumps(out, default=str, indent=2))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
