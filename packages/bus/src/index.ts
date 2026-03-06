//! packages/bus/src/index.ts
//! In-process synchronous EventBus — Phase 0.
//!
//! Phase 0: all subscribers run synchronously in publish() order.
//! Phase 1: replace with WebSocket broker (same API, drop-in swap).
//!
//! Topics and payload types are imported from @anomedge/contracts so
//! Person B and Person C always work from the same type source.

import type {
  BusTopic,
  Decision,
  EventEnvelope,
  FeatureWindow,
  SignalEvent,
} from '@anomedge/contracts';

// ─── Typed subscriber callback ────────────────────────────────────────────────

// eslint-disable-next-line @typescript-eslint/no-explicit-any
type Subscriber<T = any> = (payload: T, envelope: EventEnvelope<T>) => void;

// ─── Topic payload map (compile-time type safety) ────────────────────────────

export interface TopicPayloads {
  'signals.raw':      SignalEvent;
  'signals.features': FeatureWindow;
  'decisions':        Decision;
  'decisions.gated':  Decision;
  'actions':          unknown;
  'telemetry.sync':   unknown;
  'model.ota':        unknown;
  'system.heartbeat': unknown;
  'system.error':     unknown;
}

// ─── Monotonic sequence counters per topic ────────────────────────────────────

const topicSeq: Partial<Record<BusTopic, number>> = {};

function nextSeq(topic: BusTopic): number {
  topicSeq[topic] = (topicSeq[topic] ?? 0) + 1;
  return topicSeq[topic]!;
}

// Lightweight deterministic ID — no uuid dependency needed for Phase 0.
let _idCounter = 0;
function nextId(): string {
  return `ae-${Date.now()}-${++_idCounter}`;
}

// ─── EventBus ────────────────────────────────────────────────────────────────

export class EventBus {
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  private readonly _subs: Map<BusTopic, Set<Subscriber<any>>> = new Map();

  // ── Subscribe ──────────────────────────────────────────────────────────────

  subscribe<T extends BusTopic>(
    topic: T,
    cb: Subscriber<TopicPayloads[T]>,
  ): () => void {
    if (!this._subs.has(topic)) this._subs.set(topic, new Set());
    this._subs.get(topic)!.add(cb);
    // Return an unsubscribe function
    return () => this._subs.get(topic)?.delete(cb);
  }

  // ── Publish ────────────────────────────────────────────────────────────────

  publish<T extends BusTopic>(topic: T, payload: TopicPayloads[T]): void {
    const envelope: EventEnvelope<TopicPayloads[T]> = {
      id:      nextId(),
      topic,
      seq:     nextSeq(topic),
      ts:      Date.now(),
      payload,
    };

    const subs = this._subs.get(topic);
    if (!subs) return;

    for (const cb of subs) {
      cb(payload, envelope);
    }
  }

  // ── Helpers ────────────────────────────────────────────────────────────────

  /** Number of subscribers currently registered for a topic. */
  subscriberCount(topic: BusTopic): number {
    return this._subs.get(topic)?.size ?? 0;
  }

  /** Remove all subscribers (useful between tests). */
  reset(): void {
    this._subs.clear();
  }
}

// Export a singleton for convenience; callers may also construct their own.
export const bus = new EventBus();
