// packages/bus/src/index.ts
// In-process synchronous EventBus — Phase 0.
//
// Phase 0: all subscribers run synchronously in publish() order.
// Phase 1: replace with WebSocket broker (same API, drop-in swap).
//
// Topics and payload types are imported from @anomedge/contracts so
// Person B and Person C always work from the same type source.

import type {
  BusTopic,
  EventEnvelope,
} from '@anomedge/contracts';

// ── Metrics types ──────────────────────────────────────────────────────────

export type BusMetrics = {
  [topic: string]: { p50: number; p95: number; p99: number; count: number };
};

// ── Ring buffer for latency samples ────────────────────────────────────────

const RING_SIZE = 1000;

class LatencyRing {
  private readonly samples: number[] = [];
  private pos = 0;
  private full = false;

  push(value: number): void {
    if (this.samples.length < RING_SIZE) {
      this.samples.push(value);
    } else {
      this.samples[this.pos] = value;
      this.full = true;
    }
    this.pos = (this.pos + 1) % RING_SIZE;
  }

  getAll(): number[] {
    return this.samples.slice();
  }

  get length(): number {
    return this.samples.length;
  }
}

function percentile(sorted: number[], pct: number): number {
  const idx = Math.floor(sorted.length * pct);
  return sorted[Math.min(idx, sorted.length - 1)];
}

// ── Subscriber type ────────────────────────────────────────────────────────

// eslint-disable-next-line @typescript-eslint/no-explicit-any
type Handler<T = any> = (envelope: EventEnvelope<T>) => void;

// ── EventBus ──────────────────────────────────────────────────────────────

export class EventBus {
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  private _subs: Map<BusTopic, Set<Handler<any>>> = new Map();
  private _seq = 0;
  private _latencies: Map<string, LatencyRing> = new Map();
  private _counts: Map<string, number> = new Map();

  // ── Publish ──────────────────────────────────────────────────────────────

  publish<T>(topic: BusTopic, payload: T): void {
    const subs = this._subs.get(topic);
    if (!subs || subs.size === 0) return;

    this._seq++;

    const envelope: EventEnvelope<T> = {
      id: crypto.randomUUID(),
      topic,
      seq: this._seq,
      ts: Date.now(),
      payload,
    };

    // Track count
    this._counts.set(topic, (this._counts.get(topic) ?? 0) + 1);

    // Deliver to each subscriber, measuring latency
    const start = performance.now();
    for (const handler of subs) {
      handler(envelope);
    }
    const elapsed = performance.now() - start;

    // Record latency
    if (!this._latencies.has(topic)) {
      this._latencies.set(topic, new LatencyRing());
    }
    this._latencies.get(topic)!.push(elapsed);
  }

  // ── Subscribe ────────────────────────────────────────────────────────────

  subscribe<T>(topic: BusTopic, handler: (envelope: EventEnvelope<T>) => void): () => void {
    if (!this._subs.has(topic)) this._subs.set(topic, new Set());
    this._subs.get(topic)!.add(handler);
    return () => {
      this._subs.get(topic)?.delete(handler);
    };
  }

  // ── Once ─────────────────────────────────────────────────────────────────

  once<T>(topic: BusTopic): Promise<EventEnvelope<T>> {
    return new Promise((resolve) => {
      const unsub = this.subscribe<T>(topic, (envelope) => {
        unsub();
        resolve(envelope);
      });
    });
  }

  // ── Consume (count-based collection) ─────────────────────────────────────

  consume<T>(topic: BusTopic, handler: (envelope: EventEnvelope<T>) => void): () => void {
    return this.subscribe<T>(topic, handler);
  }

  // ── Collect (count-based, resolves after n messages) ─────────────────────

  collect<T>(topic: BusTopic, count: number): Promise<EventEnvelope<T>[]> {
    return new Promise((resolve) => {
      const collected: EventEnvelope<T>[] = [];
      const unsub = this.subscribe<T>(topic, (envelope) => {
        collected.push(envelope);
        if (collected.length >= count) {
          unsub();
          resolve(collected);
        }
      });
    });
  }

  // ── Metrics ──────────────────────────────────────────────────────────────

  getMetrics(): BusMetrics {
    const result: BusMetrics = {};
    for (const [topic, ring] of this._latencies) {
      const samples = ring.getAll();
      if (samples.length === 0) continue;
      const sorted = samples.slice().sort((a, b) => a - b);
      result[topic] = {
        p50: percentile(sorted, 0.5),
        p95: percentile(sorted, 0.95),
        p99: percentile(sorted, 0.99),
        count: this._counts.get(topic) ?? 0,
      };
    }
    return result;
  }

  // ── Reset ────────────────────────────────────────────────────────────────

  reset(): void {
    this._subs.clear();
    this._seq = 0;
    this._latencies.clear();
    this._counts.clear();
  }

  // ── Helpers ──────────────────────────────────────────────────────────────

  subscriberCount(topic: BusTopic): number {
    return this._subs.get(topic)?.size ?? 0;
  }
}

// ── Singleton and exports ─────────────────────────────────────────────────

export const bus = new EventBus();
export default EventBus;
