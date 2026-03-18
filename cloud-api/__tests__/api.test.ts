/**
 * cloud-api integration tests — TDD first.
 * All data stays in Canadian infrastructure; no foreign API calls.
 *
 * 7 tests covering: sync batch insert, validation errors, health, events query.
 */

import { describe, it, expect, beforeAll, afterAll } from 'vitest';
import { buildApp } from '../src/server';
import type { FastifyInstance } from 'fastify';
import type { EventEnvelope } from '@anomedge/contracts';

// Use a temp in-memory (or temp file) DB for tests — never the production DB.
const TEST_DB = ':memory:';

let app: FastifyInstance;

beforeAll(async () => {
  app = await buildApp({ dbPath: TEST_DB });
  await app.ready();
});

afterAll(async () => {
  await app.close();
});

// ── Helpers ────────────────────────────────────────────────────────────────────

function makeEnvelope(overrides: Partial<EventEnvelope> = {}): EventEnvelope {
  return {
    id:      crypto.randomUUID(),
    topic:   'decisions.gated',
    seq:     1,
    ts:      Date.now(),
    payload: { asset_id: 'TRUCK-001', severity: 'WARN', rule_id: 'coolant_overheat' },
    ...overrides,
  };
}

// ── Test 1: POST /api/v1/sync — valid batch of 3 ──────────────────────────────

describe('POST /api/v1/sync', () => {
  it('accepts a valid batch of 3 envelopes and returns 3 confirmed IDs', async () => {
    const batch: EventEnvelope[] = [
      makeEnvelope({ seq: 1 }),
      makeEnvelope({ seq: 2, topic: 'actions',   payload: { asset_id: 'TRUCK-001', title: 'Coolant Alert' } }),
      makeEnvelope({ seq: 3, topic: 'telemetry.sync', payload: {} }),
    ];

    const res = await app.inject({
      method: 'POST',
      url:    '/api/v1/sync',
      payload: batch,
    });

    expect(res.statusCode).toBe(200);
    const body = JSON.parse(res.body) as { confirmed: string[] };
    expect(body.confirmed).toHaveLength(3);
    // every ID must be a UUID v4
    batch.forEach((env) => {
      expect(body.confirmed).toContain(env.id);
    });
  });

  // ── Test 2: invalid schema (missing id) → 400 ──────────────────────────────

  it('rejects an envelope with missing id and returns 400 with field error', async () => {
    const bad = [{ topic: 'decisions', seq: 1, ts: Date.now(), payload: {} }]; // no id

    const res = await app.inject({
      method: 'POST',
      url:    '/api/v1/sync',
      payload: bad,
    });

    expect(res.statusCode).toBe(400);
    const body = JSON.parse(res.body) as { error: string; details?: unknown };
    // Error message must reference the failing field
    const raw = JSON.stringify(body).toLowerCase();
    expect(raw).toMatch(/id/);
  });

  // ── Test 3: empty array → 200, confirmed: [] ──────────────────────────────

  it('accepts an empty array and returns confirmed: []', async () => {
    const res = await app.inject({
      method: 'POST',
      url:    '/api/v1/sync',
      payload: [],
    });

    expect(res.statusCode).toBe(200);
    const body = JSON.parse(res.body) as { confirmed: string[] };
    expect(body.confirmed).toEqual([]);
  });
});

// ── Test 4: GET /api/v1/health ────────────────────────────────────────────────

describe('GET /api/v1/health', () => {
  it('returns 200 with status ok and db connected', async () => {
    const res = await app.inject({ method: 'GET', url: '/api/v1/health' });

    expect(res.statusCode).toBe(200);
    const body = JSON.parse(res.body) as { status: string; uptime: number; db: string };
    expect(body.status).toBe('ok');
    expect(body.db).toBe('connected');
    expect(typeof body.uptime).toBe('number');
  });
});

// ── Tests 5–7: GET /api/v1/events ────────────────────────────────────────────

describe('GET /api/v1/events', () => {
  // Seed a known batch before querying
  beforeAll(async () => {
    const seed: EventEnvelope[] = [
      makeEnvelope({ id: 'aaa00000-0000-4000-a000-000000000001', topic: 'decisions.gated', seq: 10, payload: { asset_id: 'TRUCK-001' } }),
      makeEnvelope({ id: 'aaa00000-0000-4000-a000-000000000002', topic: 'decisions.gated', seq: 11, payload: { asset_id: 'TRUCK-002' } }),
      makeEnvelope({ id: 'aaa00000-0000-4000-a000-000000000003', topic: 'actions',         seq: 12, payload: { asset_id: 'TRUCK-001' } }),
    ];
    await app.inject({ method: 'POST', url: '/api/v1/sync', payload: seed });
  });

  // ── Test 5: GET after sync returns stored events ─────────────────────────

  it('returns stored events after a sync', async () => {
    const res = await app.inject({ method: 'GET', url: '/api/v1/events' });

    expect(res.statusCode).toBe(200);
    const body = JSON.parse(res.body) as EventEnvelope[];
    expect(Array.isArray(body)).toBe(true);
    expect(body.length).toBeGreaterThanOrEqual(1);
  });

  // ── Test 6: ?topic= filter ────────────────────────────────────────────────

  it('filters events by topic=decisions.gated', async () => {
    const res = await app.inject({
      method: 'GET',
      url:    '/api/v1/events?topic=decisions.gated',
    });

    expect(res.statusCode).toBe(200);
    const body = JSON.parse(res.body) as Array<{ topic: string }>;
    expect(body.length).toBeGreaterThanOrEqual(1);
    body.forEach((e) => expect(e.topic).toBe('decisions.gated'));
  });

  // ── Test 7: ?asset_id= filter ─────────────────────────────────────────────

  it('filters events by asset_id=TRUCK-001', async () => {
    const res = await app.inject({
      method: 'GET',
      url:    '/api/v1/events?asset_id=TRUCK-001',
    });

    expect(res.statusCode).toBe(200);
    const body = JSON.parse(res.body) as Array<{ asset_id: string }>;
    expect(body.length).toBeGreaterThanOrEqual(1);
    body.forEach((e) => expect(e.asset_id).toBe('TRUCK-001'));
  });
});
