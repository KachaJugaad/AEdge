/**
 * Route handlers for cloud-api sync endpoints.
 *
 * POST /api/v1/sync   — ingest EventEnvelope batch from edge SyncAgent
 * GET  /api/v1/health — liveness probe with DB connectivity check
 * GET  /api/v1/events — query stored envelopes with optional filters
 *
 * Canadian data sovereignty: no outbound network calls; all storage is local SQLite.
 */

import type { FastifyInstance, FastifyRequest, FastifyReply } from 'fastify';
import { SyncBatchSchema, EventsQuerySchema } from '../schemas';
import { extractAssetId } from '../db';
import type { CloudDB, EventRow } from '../db';
import type { EventEnvelope } from '@anomedge/contracts';

// ── Route registration ────────────────────────────────────────────────────────

export function registerSyncRoutes(app: FastifyInstance, cloudDb: CloudDB): void {
  const { db, stmts } = cloudDb;

  // ── POST /api/v1/sync ─────────────────────────────────────────────────────

  app.post(
    '/api/v1/sync',
    async (req: FastifyRequest, reply: FastifyReply) => {
      const parsed = SyncBatchSchema.safeParse(req.body);

      if (!parsed.success) {
        return reply.status(400).send({
          error:   'Validation failed',
          details: parsed.error.flatten(),
        });
      }

      const batch = parsed.data;

      if (batch.length === 0) {
        return reply.status(200).send({ confirmed: [] });
      }

      // Derive asset_id from the first envelope that has one for logging
      let logAssetId = 'unknown';
      for (const env of batch) {
        const aid = extractAssetId(env.topic, env.payload);
        if (aid) { logAssetId = aid; break; }
      }

      // Single transaction — batch insert + asset upsert
      const insertBatch = db.transaction(() => {
        const confirmed: string[] = [];

        for (const env of batch) {
          const assetId = extractAssetId(env.topic, env.payload);
          const payloadJson = JSON.stringify(env.payload);

          stmts.insertEvent.run({
            id:       env.id,
            topic:    env.topic,
            seq:      env.seq,
            ts:       env.ts,
            payload:  payloadJson,
            asset_id: assetId,
          });

          // Upsert asset record when we have an asset_id
          if (assetId) {
            stmts.upsertAsset.run({
              asset_id:  assetId,
              last_seen: env.ts,
            });
          }

          confirmed.push(env.id);
        }

        return confirmed;
      });

      const confirmed = insertBatch() as string[];

      app.log.info(
        `Sync received: ${batch.length} events from ${logAssetId} at ${new Date().toISOString()}`
      );

      return reply.status(200).send({ confirmed });
    }
  );

  // ── GET /api/v1/health ───────────────────────────────────────────────────

  app.get(
    '/api/v1/health',
    async (_req: FastifyRequest, reply: FastifyReply) => {
      try {
        // Verify DB is accessible with a lightweight count query
        (stmts.countCheck.get as Function)();

        return reply.status(200).send({
          status: 'ok',
          uptime: process.uptime(),
          db:     'connected',
        });
      } catch (err) {
        app.log.error({ err }, 'Health check DB failure');
        return reply.status(503).send({
          status: 'error',
          uptime: process.uptime(),
          db:     'disconnected',
        });
      }
    }
  );

  // ── GET /api/v1/events ───────────────────────────────────────────────────

  app.get(
    '/api/v1/events',
    async (req: FastifyRequest, reply: FastifyReply) => {
      const qp = EventsQuerySchema.safeParse(req.query);

      if (!qp.success) {
        return reply.status(400).send({
          error:   'Invalid query parameters',
          details: qp.error.flatten(),
        });
      }

      const { topic, asset_id, limit } = qp.data;

      // Build dynamic WHERE clause — all values come through Zod so are safe
      const conditions: string[] = [];
      const bindings: Record<string, unknown> = { limit };

      if (topic) {
        conditions.push('topic = @topic');
        bindings['topic'] = topic;
      }

      if (asset_id) {
        conditions.push('asset_id = @asset_id');
        bindings['asset_id'] = asset_id;
      }

      const where  = conditions.length > 0 ? `WHERE ${conditions.join(' AND ')}` : '';
      const sql    = `
        SELECT id, topic, seq, ts, payload, asset_id, created_at
        FROM   events
        ${where}
        ORDER  BY ts DESC
        LIMIT  @limit
      `;

      const rows = db.prepare(sql).all(bindings) as EventRow[];

      // Deserialise payload back to object before returning
      const envelopes: EventEnvelope[] = rows.map((row) => ({
        id:      row.id,
        topic:   row.topic as EventEnvelope['topic'],
        seq:     row.seq,
        ts:      row.ts,
        payload: JSON.parse(row.payload) as unknown,
        // asset_id is surfaced at the top level for filter convenience
        ...(row.asset_id ? { asset_id: row.asset_id } : {}),
      }));

      return reply.status(200).send(envelopes);
    }
  );
}
