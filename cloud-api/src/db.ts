/**
 * SQLite database setup for cloud-api.
 * Uses better-sqlite3 (synchronous) for performance.
 * WAL mode enabled for concurrent read throughput.
 *
 * Canadian data sovereignty: database file stays on CA servers only.
 */

import Database from 'better-sqlite3';
import type { Database as DBType } from 'better-sqlite3';

// ── Schema constants ──────────────────────────────────────────────────────────

const DDL_EVENTS = `
  CREATE TABLE IF NOT EXISTS events (
    id         TEXT PRIMARY KEY,
    topic      TEXT NOT NULL,
    seq        INTEGER NOT NULL,
    ts         INTEGER NOT NULL,
    payload    TEXT NOT NULL,
    asset_id   TEXT,
    created_at TEXT DEFAULT CURRENT_TIMESTAMP
  )
`;

const DDL_ASSETS = `
  CREATE TABLE IF NOT EXISTS assets (
    asset_id    TEXT PRIMARY KEY,
    last_seen   INTEGER,
    event_count INTEGER DEFAULT 0
  )
`;

const DDL_IDX_TOPIC    = `CREATE INDEX IF NOT EXISTS idx_events_topic    ON events(topic)`;
const DDL_IDX_ASSET_ID = `CREATE INDEX IF NOT EXISTS idx_events_asset_id ON events(asset_id)`;
const DDL_IDX_TS       = `CREATE INDEX IF NOT EXISTS idx_events_ts       ON events(ts)`;

// ── Prepared statement types ──────────────────────────────────────────────────

export interface EventRow {
  id:         string;
  topic:      string;
  seq:        number;
  ts:         number;
  payload:    string;  // JSON string
  asset_id:   string | null;
  created_at: string;
}

export interface AssetRow {
  asset_id:    string;
  last_seen:   number | null;
  event_count: number;
}

// ── DB factory ────────────────────────────────────────────────────────────────

export interface PreparedStatements {
  insertEvent:  ReturnType<DBType['prepare']>;
  upsertAsset:  ReturnType<DBType['prepare']>;
  selectEvents: ReturnType<DBType['prepare']>;
  countCheck:   ReturnType<DBType['prepare']>;
  selectAssets: ReturnType<DBType['prepare']>;
}

export interface CloudDB {
  db:   DBType;
  stmts: PreparedStatements;
}

export function createDB(dbPath: string): CloudDB {
  const db = new Database(dbPath);

  // WAL mode for concurrent reads without blocking writes
  db.pragma('journal_mode = WAL');
  db.pragma('synchronous = NORMAL');

  // Create tables + indexes
  db.exec(DDL_EVENTS);
  db.exec(DDL_ASSETS);
  db.exec(DDL_IDX_TOPIC);
  db.exec(DDL_IDX_ASSET_ID);
  db.exec(DDL_IDX_TS);

  // Prepared statements — reused per request for performance
  const stmts: PreparedStatements = {
    insertEvent: db.prepare(`
      INSERT OR IGNORE INTO events (id, topic, seq, ts, payload, asset_id)
      VALUES (@id, @topic, @seq, @ts, @payload, @asset_id)
    `),

    upsertAsset: db.prepare(`
      INSERT INTO assets (asset_id, last_seen, event_count)
      VALUES (@asset_id, @last_seen, 1)
      ON CONFLICT(asset_id) DO UPDATE SET
        last_seen   = excluded.last_seen,
        event_count = event_count + 1
    `),

    // Dynamic filtering is handled in the route; this is the base select.
    // For filtered queries we build SQL at query time (safe: params are validated).
    selectEvents: db.prepare(`
      SELECT id, topic, seq, ts, payload, asset_id, created_at
      FROM   events
      ORDER  BY ts DESC
      LIMIT  @limit
    `),

    countCheck: db.prepare(`SELECT COUNT(*) as cnt FROM events`),

    selectAssets: db.prepare(`
      SELECT asset_id, last_seen, event_count FROM assets ORDER BY last_seen DESC
    `),
  };

  return { db, stmts };
}

// ── Extract asset_id from payload for topics that carry it ───────────────────

const ASSET_ID_TOPICS = new Set(['decisions.gated', 'actions', 'decisions', 'signals.raw', 'signals.features']);

export function extractAssetId(topic: string, payload: unknown): string | null {
  if (!ASSET_ID_TOPICS.has(topic)) return null;
  if (payload !== null && typeof payload === 'object') {
    const p = payload as Record<string, unknown>;
    if (typeof p['asset_id'] === 'string') return p['asset_id'];
  }
  return null;
}
