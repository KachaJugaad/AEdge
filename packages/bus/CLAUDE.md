# packages/bus — EventBus

## This is the SECOND package merged after contracts (Day 1)
Everyone depends on it. Build it before any other Person C work.

## What to Build
Typed, in-process EventBus. All 9 topics. Zero external broker needed.

## API to Implement
class EventBus:
  publish<T>(topic: BusTopic, payload: T): void
    - Wraps payload in EventEnvelope (adds id=UUID, seq++, ts=Date.now())
    - Delivers to all subscribers synchronously
    - Records latency in metrics (start = Date.now() before delivery)

  subscribe<T>(topic: BusTopic, handler: (envelope: EventEnvelope<T>) => void): () => void
    - Returns unsubscribe function
    - Multiple subscribers per topic allowed

  once<T>(topic: BusTopic): Promise<EventEnvelope<T>>
    - Returns promise that resolves on next message on topic

  consume<T>(topic: BusTopic, count: number): Promise<EventEnvelope<T>[]>
    - Collects exactly `count` messages then resolves

  collect<T>(topic: BusTopic, durationMs: number): Promise<EventEnvelope<T>[]>
    - Collects all messages published in durationMs window

  getMetrics(): BusMetrics
    - Returns p50/p95/p99 latency per topic
    - Returns message count per topic

  reset(): void
    - Clears all subscribers, resets seq, resets metrics
    - MUST be called at start of every gate test

## All 9 BusTopics Must Work
signals.raw · signals.features · decisions · decisions.gated · actions
telemetry.sync · model.ota · system.heartbeat · system.error

## Tests Required (write these FIRST — TDD)
1. subscribe + publish round-trip: handler called with correct payload
2. Multiple subscribers on same topic: all receive the message
3. unsubscribe: handler no longer called after unsubscribe
4. once(): resolves with first message, does not resolve again
5. collect() with count: resolves after exactly n messages
6. getMetrics(): returns latency values after publish
7. seq is monotonically increasing across multiple publishes
8. reset() clears state: subscribers gone, seq back to 0, metrics cleared
9. Different topics are independent: publish on A does not trigger handler on B
