//! packages/core/src/RuleEngine.ts
//! TypeScript mirror of crates/anomedge-core/src/rules.rs
//!
//! Evaluates a FeatureWindow against every rule in a PolicyPack.
//! Returns all triggered Decisions — TrustEngine handles cooldown/hysteresis.
//!
//! Performance target: evaluate() < 1ms per frame (O(r) where r ≈ 20 rules).

import type {
  Decision,
  DecisionSource,
  FeatureWindow,
  PolicyPack,
  PolicyRule,
  RuleOperator,
  SignalMap,
} from '@anomedge/contracts';

// ─── RuleEngine ───────────────────────────────────────────────────────────────

export class RuleEngine {
  constructor(private readonly policy: PolicyPack) {}

  /**
   * Evaluate all rules against the window.
   * Returns every triggered Decision (0..n). Multiple may fire simultaneously.
   * Caller (TrustEngine) applies cooldown and hysteresis.
   */
  evaluate(window: FeatureWindow): Decision[] {
    const decisions: Decision[] = [];

    for (const rule of this.policy.rules) {
      const rawValue = resolveSignal(window, rule.signal);
      if (rawValue === undefined) continue;          // signal absent — skip

      if (checkOperator(rawValue, rule.operator, rule.threshold)) {
        decisions.push(buildDecision(window, rule, rawValue));
      }
    }

    return decisions;
  }

  get pack(): PolicyPack {
    return this.policy;
  }
}

// ─── Signal resolution ────────────────────────────────────────────────────────

function resolveSignal(window: FeatureWindow, signal: string): number | undefined {
  // Direct top-level FeatureWindow fields
  const direct: Record<string, number> = {
    coolant_slope:     window.coolant_slope,
    brake_spike_count: window.brake_spike_count,
    speed_mean:        window.speed_mean,
    rpm_mean:          window.rpm_mean,
    engine_load_mean:  window.engine_load_mean,
    throttle_variance: window.throttle_variance,
    hydraulic_spike:   window.hydraulic_spike ? 1 : 0,
    transmission_heat: window.transmission_heat ? 1 : 0,
    dtc_new_count:     window.dtc_new.length,
  };

  if (signal in direct) return direct[signal];

  // Nested: signals_snapshot.<field>
  if (signal.startsWith('signals_snapshot.')) {
    const field = signal.slice('signals_snapshot.'.length);
    const snap  = window.signals_snapshot as SignalMap;
    const val   = snap[field];
    return typeof val === 'number' ? val : undefined;
  }

  return undefined;
}

// ─── Operator check ───────────────────────────────────────────────────────────

function checkOperator(value: number, op: RuleOperator, threshold: number): boolean {
  switch (op) {
    case 'gt':       return value > threshold;
    case 'lt':       return value < threshold;
    case 'gte':      return value >= threshold;
    case 'lte':      return value <= threshold;
    case 'eq':       return value === threshold;
    case 'contains': return value >= threshold; // numeric "contains" → gte
    default:         return false;
  }
}

// ─── Decision builder ─────────────────────────────────────────────────────────

function buildDecision(
  window:   FeatureWindow,
  rule:     PolicyRule,
  rawValue: number,
): Decision {
  return {
    ts:              window.ts,
    asset_id:        window.asset_id,
    severity:        rule.severity,
    rule_id:         rule.id,
    rule_group:      rule.group,
    confidence:      1.0,
    triggered_by:    [rule.signal],
    raw_value:       rawValue,
    threshold:       rule.threshold,
    decision_source: 'RULE_ENGINE' as DecisionSource,
    context:         window,
  };
}
