//! packages/core/src/pipeline.ts
//! Wires FeatureEngine → RuleEngine → TrustEngine via EventBus.
//!
//! Call order per event:
//!   signals.raw  →  FeatureEngine.ingest()  →  publish signals.features
//!                                           →  RuleEngine.evaluate()
//!                                           →  publish decisions (raw)
//!                                           →  TrustEngine.evaluateAll()
//!                                           →  publish decisions.gated  ← Person B handoff

import type { Decision, PolicyPack, SignalEvent } from '@anomedge/contracts';
import { EventBus }     from '@anomedge/bus';
import { FeatureEngine } from './FeatureEngine';
import { RuleEngine }    from './RuleEngine';
import { TrustEngine }   from './TrustEngine';

// ─── Pipeline ────────────────────────────────────────────────────────────────

export class Pipeline {
  private readonly featureEngine: FeatureEngine;
  private readonly ruleEngine:    RuleEngine;
  private readonly trustEngine:   TrustEngine;
  private readonly bus:           EventBus;

  constructor(policy: PolicyPack, bus: EventBus) {
    this.featureEngine = new FeatureEngine();
    this.ruleEngine    = new RuleEngine(policy);
    this.trustEngine   = new TrustEngine(policy);
    this.bus           = bus;

    // Subscribe to signals.raw and run the full chain
    this.bus.subscribe<SignalEvent>('signals.raw', (envelope) => {
      this._processEvent(envelope.payload);
    });
  }

  private _processEvent(event: SignalEvent): void {
    // Step 1: Feature computation
    const window = this.featureEngine.ingest(event);
    this.bus.publish('signals.features', window);

    // Step 2: Inference — Tier 3 (RuleEngine, always fires in Phase 0)
    const rawDecisions: Decision[] = this.ruleEngine.evaluate(window);

    for (const d of rawDecisions) {
      this.bus.publish('decisions', d);
    }

    // Step 3: Trust filter → gated decisions
    const gated = this.trustEngine.evaluateAll(rawDecisions);
    for (const d of gated) {
      this.bus.publish('decisions.gated', d);
    }
  }
}

// ─── Factory ─────────────────────────────────────────────────────────────────

/**
 * Create a fully wired Pipeline and wire it to the provided bus.
 * Returns the Pipeline instance (callers rarely need to hold it —
 * the bus subscription drives everything).
 */
export function createPipeline(policy: PolicyPack, bus: EventBus): Pipeline {
  return new Pipeline(policy, bus);
}
