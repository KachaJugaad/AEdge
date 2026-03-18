/**
 * Fastify application factory for cloud-api.
 * Exported as buildApp() so tests can inject an in-memory SQLite path.
 *
 * Canadian data sovereignty: all data is retained in Canadian infrastructure.
 * This server makes zero outbound network calls.
 */

import Fastify from 'fastify';
import type { FastifyInstance } from 'fastify';
import { createDB } from './db';
import { registerSyncRoutes } from './routes/sync';

// ── Options ───────────────────────────────────────────────────────────────────

export interface AppOptions {
  /** SQLite file path. Pass ':memory:' for tests. Defaults to anomedge.sqlite */
  dbPath?: string;
}

const DEFAULT_DB_PATH = 'anomedge.sqlite';

// ── App factory ───────────────────────────────────────────────────────────────

export async function buildApp(opts: AppOptions = {}): Promise<FastifyInstance> {
  const dbPath = opts.dbPath ?? DEFAULT_DB_PATH;

  const app = Fastify({ logger: true });

  // Initialise database
  const cloudDb = createDB(dbPath);

  app.log.info(`[cloud-api] Database: ${dbPath}`);

  // Register all route handlers
  registerSyncRoutes(app, cloudDb);

  // Graceful shutdown — close DB on process exit
  app.addHook('onClose', async () => {
    cloudDb.db.close();
  });

  return app;
}
