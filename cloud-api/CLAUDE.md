# cloud-api — REST API (Phase 1+)

## Stack: Fastify + better-sqlite3 + Zod + TypeScript

## Endpoints to Build
POST /api/v1/sync
  - Accepts: EventEnvelope[] body (batch from Person B's SyncAgent)
  - Validates: schema against @anomedge/contracts EventEnvelope shape
  - Writes: to SQLite (dev) / Postgres (prod)
  - Returns: { confirmed: string[] }  (array of confirmed envelope IDs)
  - On invalid: 400 with { error: string, field: string }

GET /api/v1/health
  - Returns: { status: "ok", uptime: number, db: "connected"|"error" }

GET /api/v1/assets
  - Returns list of known asset_ids with last-seen timestamp

## Canadian Data Sovereignty
All data stays in Canadian cloud infrastructure.
No foreign API calls from this service.
Log every sync batch: asset_id, count, timestamp.

## Tests (write first)
- POST /sync with valid batch → 200, returns confirmed IDs
- POST /sync with invalid schema → 400 with error detail
- POST /sync empty array → 200, confirmed: []
- GET /health → 200, db: "connected"
- Persisted events are queryable after sync
