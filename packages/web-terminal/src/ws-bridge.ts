// packages/web-terminal/src/ws-bridge.ts
// WebSocket bridge — forwards EventBus messages to browser clients in real-time.
//
// startBridge(bus, port?) subscribes to ALL bus topics and broadcasts each
// EventEnvelope as JSON to every connected WS client.
// stopBridge() closes the server and all active connections.

import { WebSocketServer, WebSocket } from 'ws';
import type { EventBus } from '@anomedge/bus';
import type { BusTopic } from '@anomedge/contracts';

// ── Constants ─────────────────────────────────────────────────────────────────

const ALL_TOPICS: BusTopic[] = [
  'signals.raw',
  'signals.features',
  'decisions',
  'decisions.gated',
  'actions',
  'system.heartbeat',
  'system.error',
  'telemetry.sync',
  'model.ota',
];

// ── Module state ──────────────────────────────────────────────────────────────

let _wss: WebSocketServer | null = null;
let _unsubscribers: Array<() => void> = [];
const _clients: Set<WebSocket> = new Set();

// ── Helpers ───────────────────────────────────────────────────────────────────

function broadcast(data: string): void {
  for (const client of _clients) {
    if (client.readyState === WebSocket.OPEN) {
      client.send(data);
    }
  }
}

function logClientCount(): void {
  console.log(`[ws-bridge] connected clients: ${_clients.size}`);
}

// ── Public API ────────────────────────────────────────────────────────────────

/**
 * Start the WebSocket bridge server.
 *
 * Subscribes to all bus topics and forwards each EventEnvelope as JSON to
 * every connected browser client. Safe to call multiple times — calling while
 * already running is a no-op that returns the existing port.
 *
 * @param bus   The EventBus instance to subscribe to.
 * @param port  TCP port for the WebSocket server. Defaults to 4200.
 * @returns     Promise that resolves once the server is listening.
 */
export function startBridge(bus: EventBus, port = 4200): Promise<void> {
  if (_wss) {
    console.warn('[ws-bridge] already running — ignoring startBridge() call');
    return Promise.resolve();
  }

  return new Promise((resolve, reject) => {
    _wss = new WebSocketServer({ port });

    _wss.on('error', (err) => {
      console.error(`[ws-bridge] server error: ${err.message}`);
      reject(err);
    });

    _wss.on('listening', () => {
      console.log(`[ws-bridge] listening on ws://localhost:${port}`);

      // Subscribe to every bus topic and broadcast envelopes to clients
      for (const topic of ALL_TOPICS) {
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        const unsub = bus.subscribe<any>(topic, (envelope) => {
          if (_clients.size === 0) return; // nothing to send
          try {
            const json = JSON.stringify(envelope);
            broadcast(json);
          } catch (err) {
            console.error(`[ws-bridge] serialization error on topic ${topic}:`, err);
          }
        });
        _unsubscribers.push(unsub);
      }

      resolve();
    });

    _wss.on('connection', (ws, req) => {
      const remoteAddr = req.socket.remoteAddress ?? 'unknown';
      _clients.add(ws);
      console.log(`[ws-bridge] client connected from ${remoteAddr}`);
      logClientCount();

      ws.on('close', () => {
        _clients.delete(ws);
        console.log(`[ws-bridge] client disconnected from ${remoteAddr}`);
        logClientCount();
      });

      ws.on('error', (err) => {
        console.error(`[ws-bridge] client error (${remoteAddr}): ${err.message}`);
        _clients.delete(ws);
      });
    });
  });
}

/**
 * Stop the WebSocket bridge server.
 *
 * Unsubscribes from all bus topics, closes all client connections, then closes
 * the server. Returns a Promise that resolves once fully shut down.
 */
export function stopBridge(): Promise<void> {
  // Unsubscribe from bus
  for (const unsub of _unsubscribers) {
    unsub();
  }
  _unsubscribers = [];

  // Terminate all connected clients
  for (const client of _clients) {
    client.terminate();
  }
  _clients.clear();

  if (!_wss) return Promise.resolve();

  return new Promise((resolve, reject) => {
    _wss!.close((err) => {
      _wss = null;
      if (err) {
        reject(err);
      } else {
        console.log('[ws-bridge] server stopped');
        resolve();
      }
    });
  });
}

/** Current number of connected WebSocket clients (for tests / health checks). */
export function clientCount(): number {
  return _clients.size;
}
