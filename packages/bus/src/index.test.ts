//! packages/bus/src/index.test.ts
import { describe, it, expect, beforeEach } from 'vitest';
import { EventBus } from './index';
import type { SignalEvent } from '@anomedge/contracts';

describe('EventBus', () => {
  let bus: EventBus;

  beforeEach(() => {
    bus = new EventBus();
  });

  it('delivers payload to subscriber', () => {
    const received: unknown[] = [];
    bus.subscribe('signals.raw', (payload) => received.push(payload));
    const event = { ts: 1, asset_id: 'A' } as SignalEvent;
    bus.publish('signals.raw', event);
    expect(received).toHaveLength(1);
    expect(received[0]).toBe(event);
  });

  it('delivers to multiple subscribers on same topic', () => {
    const counts = [0, 0];
    bus.subscribe('decisions', () => counts[0]++);
    bus.subscribe('decisions', () => counts[1]++);
    bus.publish('decisions', {} as never);
    expect(counts).toEqual([1, 1]);
  });

  it('does not cross-deliver to different topics', () => {
    const raw: unknown[] = [];
    const feat: unknown[] = [];
    bus.subscribe('signals.raw', (p) => raw.push(p));
    bus.subscribe('signals.features', (p) => feat.push(p));
    bus.publish('signals.raw', {} as never);
    expect(raw).toHaveLength(1);
    expect(feat).toHaveLength(0);
  });

  it('unsubscribe stops delivery', () => {
    let count = 0;
    const unsub = bus.subscribe('decisions.gated', () => count++);
    bus.publish('decisions.gated', {} as never);
    unsub();
    bus.publish('decisions.gated', {} as never);
    expect(count).toBe(1);
  });

  it('envelope includes id, topic, seq, ts, payload', () => {
    let env: unknown;
    bus.subscribe('signals.raw', (_p, envelope) => { env = envelope; });
    const event = { ts: 42, asset_id: 'B' } as SignalEvent;
    bus.publish('signals.raw', event);
    expect(env).toMatchObject({
      topic:   'signals.raw',
      seq:     expect.any(Number),
      ts:      expect.any(Number),
      id:      expect.stringMatching(/^ae-/),
      payload: event,
    });
  });

  it('seq increments per topic independently', () => {
    const seqs: number[] = [];
    bus.subscribe('decisions', (_p, e) => seqs.push(e.seq));
    bus.publish('decisions', {} as never);
    bus.publish('decisions', {} as never);
    bus.publish('decisions', {} as never);
    expect(seqs[2]).toBeGreaterThan(seqs[0]);
  });

  it('subscriberCount returns correct count', () => {
    expect(bus.subscriberCount('signals.raw')).toBe(0);
    const unsub = bus.subscribe('signals.raw', () => {});
    expect(bus.subscriberCount('signals.raw')).toBe(1);
    unsub();
    expect(bus.subscriberCount('signals.raw')).toBe(0);
  });

  it('reset clears all subscribers', () => {
    let count = 0;
    bus.subscribe('signals.raw', () => count++);
    bus.reset();
    bus.publish('signals.raw', {} as never);
    expect(count).toBe(0);
  });
});
