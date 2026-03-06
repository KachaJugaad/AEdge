//! packages/core/src/TrustEngine.ts
//! TypeScript mirror of crates/anomedge-core/src/trust.rs
//!
//! Cooldown + hysteresis filter for the decision stream.
//! Suppresses duplicate alerts that fire too soon or don't exceed the
//! hysteresis band. Maintains state per "asset_id::rule_id" key.

import type { Decision, PolicyPack } from '@anomedge/contracts';

// ─── Internal per-rule state ──────────────────────────────────────────────────

interface TrustEntry {
  lastFiredTs: number;  // Unix ms of last pass-through
}

// ─── TrustEngine ─────────────────────────────────────────────────────────────

export class TrustEngine {
  private readonly state: Map<string, TrustEntry> = new Map();

  constructor(private readonly policy: PolicyPack) {}

  /**
   * Filter one Decision through cooldown + hysteresis.
   *
   * Rules (applied in order):
   * 1. No prior state for this key → always pass through (first fire).
   * 2. elapsed_ms < cooldown_ms → suppress.
   * 3. raw_value < threshold + hysteresis → suppress (prevents oscillation).
   * 4. Pass through → update state.
   *
   * Unknown rule_id (e.g. AI class) → no constraint, always passes.
   */
  evaluate(decision: Decision): Decision | null {
    const rule = this.policy.rules.find(r => r.id === decision.rule_id);
    const cooldownMs  = rule?.cooldown_ms  ?? 0;
    const hysteresis  = rule?.hysteresis   ?? 0;
    const key         = stateKey(decision.asset_id, decision.rule_id);

    const entry = this.state.get(key);

    if (entry !== undefined) {
      // Rule 2: cooldown
      const elapsedMs = decision.ts - entry.lastFiredTs;
      if (elapsedMs < cooldownMs) return null;

      // Rule 3: hysteresis
      const required = decision.threshold + hysteresis;
      if (decision.raw_value < required) return null;
    }

    // Rule 1 / Rule 4: pass through, update state
    this.state.set(key, { lastFiredTs: decision.ts });
    return decision;
  }

  /** Filter a batch of decisions (all evaluated independently). */
  evaluateAll(decisions: Decision[]): Decision[] {
    return decisions.flatMap(d => {
      const result = this.evaluate(d);
      return result ? [result] : [];
    });
  }

  /** Number of unique rule+asset keys currently tracked. */
  get trackedKeys(): number {
    return this.state.size;
  }
}

function stateKey(assetId: string, ruleId: string): string {
  return `${assetId}::${ruleId}`;
}
