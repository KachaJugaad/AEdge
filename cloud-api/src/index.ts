/**
 * Production entry point — starts Fastify on 0.0.0.0:3000.
 * Canadian data sovereignty: database lives at ./anomedge.sqlite (CA servers only).
 */

import { buildApp } from './server';

const PORT = 3000;
const HOST = '0.0.0.0';

async function main(): Promise<void> {
  const app = await buildApp();

  try {
    await app.listen({ port: PORT, host: HOST });
    app.log.info(`[cloud-api] Listening on http://${HOST}:${PORT}`);
  } catch (err) {
    app.log.error(err);
    process.exit(1);
  }
}

main();
