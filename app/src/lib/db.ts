import "server-only";
import Database from "better-sqlite3";
import { dbPath } from "./config";

let _db: Database.Database | null = null;

/**
 * Lazy, read-only connection to <wiki>/_data/tampa.db.
 * Opens with readonly + fileMustExist=false so the first call when DB is
 * absent throws a clear error rather than creating an empty file.
 */
export function getDb(): Database.Database {
  if (_db) return _db;
  const p = dbPath();
  _db = new Database(p, { readonly: true, fileMustExist: true });
  _db.pragma("journal_mode = WAL");
  _db.pragma("query_only = true");
  return _db;
}

export function closeDb(): void {
  if (_db) {
    _db.close();
    _db = null;
  }
}
