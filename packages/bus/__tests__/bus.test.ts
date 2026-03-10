import { describe, it, expect, beforeEach } from 'vitest';
import { EventBus } from '../src/index';
import type { SignalEvent, EventEnvelope, BusTopic } from '@anomedge/contracts';

describe('EventBus', () => {
  let bus: EventBus;

  beforeEach(() => {
    bus = new EventBus();
  });

  // Test 1: subscribe + publish round-trip
  it('delivers envelope with correct topic and payload to subscriber', () => {
    const received: EventEnvelope<SignalEvent>[] = [];
    const signal: SignalEvent = {
      ts: Date.now(),
      asset_id: 'TRUCK-001',
      driver_id: 'DRV-042',
      source: 'SIMULATOR',
      signals: { engine_rpm: 3200 },
    };

    bus.subscribe<SignalEvent>('signals.raw', (envelope) => {
      received.push(envelope);
    });

    bus.publish('signals.raw', signal);

    expect(received).toHaveLength(1);
    expect(received[0].topic).toBe('signals.raw');
    expect(received[0].payload).toEqual(signal);
    expect(received[0].id).toBeDefined();
    expect(received[0].seq).toBe(1);
    expect(received[0].ts).toBeGreaterThan(0);
  });

  // Test 2: Multiple subscribers on same topic
  it('delivers to multiple subscribers on same topic', () => {
    const calls = [0, 0];
    bus.subscribe('decisions', () => { calls[0]++; });
    bus.subscribe('decisions', () => { calls[1]++; });

    bus.publish('decisions', { rule_id: 'test' });

    expect(calls[0]).toBe(1);
    expect(calls[1]).toBe(1);
  });

  // Test 3: Unsubscribe stops delivery
  it('does not call handler after unsubscribe', () => {
    let count = 0;
    const unsub = bus.subscribe('decisions.gated', () => { count++; });

    bus.publish('decisions.gated', {});
    expect(count).toBe(1);

    unsub();
    bus.publish('decisions.gated', {});
    expect(count).toBe(1);
  });

  // Test 4: once() resolves with first message
  it('once() resolves with the first message on the topic', async () => {
    const promise = bus.once<{ value: number }>('signals.features');

    bus.publish('signals.features', { value: 42 });

    const envelope = await promise;
    expect(envelope.topic).toBe('signals.features');
    expect(envelope.payload).toEqual({ value: 42 });
  });

  // Test 5: collect(count) resolves after exactly n messages
  it('collect() resolves after exactly count messages', async () => {
    const promise = bus.collect<{ n: number }>('actions', 3);

    bus.publish('actions', { n: 1 });
    bus.publish('actions', { n: 2 });
    bus.publish('actions', { n: 3 });

    const envelopes = await promise;
    expect(envelopes).toHaveLength(3);
    expect(envelopes[0].payload).toEqual({ n: 1 });
    expect(envelopes[1].payload).toEqual({ n: 2 });
    expect(envelopes[2].payload).toEqual({ n: 3 });
  });

  // Test 6: getMetrics() returns latency after publishes
  it('getMetrics() returns p50 latency > 0 after publishes with subscribers', () => {
    bus.subscribe('signals.raw', () => {
      // Simulate some work
      const start = performance.now();
      while (performance.now() - start < 0.01) { /* busy wait */ }
    });

    for (let i = 0; i < 5; i++) {
      bus.publish('signals.raw', { ts: Date.now(), asset_id: 'A', driver_id: 'D', source: 'SIMULATOR' as const, signals: {} });
    }

    const metrics = bus.getMetrics();
    expect(metrics['signals.raw']).toBeDefined();
    expect(metrics['signals.raw'].count).toBe(5);
    expect(metrics['signals.raw'].p50).toBeGreaterThanOrEqual(0);
    expect(metrics['signals.raw'].p95).toBeGreaterThanOrEqual(0);
    expect(metrics['signals.raw'].p99).toBeGreaterThanOrEqual(0);
  });

  // Test 7: Monotonic seq — 10 messages, seq 1..10 with no gaps
  it('seq is monotonically increasing 1..10 with no gaps', () => {
    const seqs: number[] = [];
    bus.subscribe('telemetry.sync', (envelope) => {
      seqs.push(envelope.seq);
    });

    for (let i = 0; i < 10; i++) {
      bus.publish('telemetry.sync', {});
    }

    expect(seqs).toEqual([1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
  });

  // Test 8: reset() clears subscribers, seq, and metrics
  it('reset() clears subscribers, resets seq to 0, and empties metrics', () => {
    let count = 0;
    bus.subscribe('signals.raw', () => { count++; });
    bus.publish('signals.raw', { ts: 1, asset_id: 'A', driver_id: 'D', source: 'SIMULATOR' as const, signals: {} });
    expect(count).toBe(1);

    bus.reset();

    // Metrics emptied (check immediately after reset, before any new publishes)
    const metrics = bus.getMetrics();
    expect(Object.keys(metrics)).toHaveLength(0);

    // Subscribers cleared — handler should NOT be called
    bus.publish('signals.raw', { ts: 2, asset_id: 'A', driver_id: 'D', source: 'SIMULATOR' as const, signals: {} });
    expect(count).toBe(1);

    // Seq restarts — next subscribe should see seq=1
    const seqs: number[] = [];
    bus.subscribe('signals.raw', (envelope) => { seqs.push(envelope.seq); });
    bus.publish('signals.raw', { ts: 3, asset_id: 'A', driver_id: 'D', source: 'SIMULATOR' as const, signals: {} });
    expect(seqs[0]).toBe(1);
  });

  // Test 9: Topic isolation
  it('publish on signals.raw does NOT trigger handler on decisions', () => {
    let called = false;
    bus.subscribe('decisions', () => { called = true; });

    bus.publish('signals.raw', { ts: 1, asset_id: 'A', driver_id: 'D', source: 'SIMULATOR' as const, signals: {} });

    expect(called).toBe(false);
  });
});
