//! trust.rs
//! TrustEngine — cooldown + hysteresis filter for the decision stream.
//!
//! Receives every `Decision` from `InferenceChain`, suppresses duplicates that
//! fire too often or don't exceed the hysteresis band, and emits to
//! `decisions.gated` — the handoff topic Person B subscribes to.
//!
//! State is per `"{asset_id}::{rule_id}"` key so each asset has independent
//! cooldown tracking.
//!
//! Performance target: evaluate() is O(1) — single HashMap lookup + insert.

use std::collections::HashMap;

use crate::types::{Decision, PolicyPack};

// ─── Internal state per rule+asset ───────────────────────────────────────────

#[derive(Debug, Clone)]
struct TrustEntry {
    /// Timestamp (Unix ms) when this rule last passed the trust filter.
    last_fired_ts: i64,
}

// ─── TrustEngine ─────────────────────────────────────────────────────────────

/// Stateful cooldown + hysteresis filter.
///
/// Initialised once with the same `PolicyPack` as `RuleEngine` so it can look
/// up `cooldown_ms` and `hysteresis` for any `rule_id` that fires.
///
/// Decisions from Edge AI or ML Statistical tiers that have no matching policy
/// rule (rule_id not in pack) are always passed through — no suppression.
pub struct TrustEngine {
    policy: PolicyPack,
    state:  HashMap<String, TrustEntry>,
}

impl TrustEngine {
    pub fn new(policy: PolicyPack) -> Self {
        Self { policy, state: HashMap::new() }
    }

    /// Filter a single decision through the cooldown + hysteresis gate.
    ///
    /// Returns `Some(decision)` if it should be published to `decisions.gated`,
    /// `None` if it is suppressed.
    ///
    /// Suppression rules (applied in order):
    /// 1. **First fire** — no prior state for this key → always pass through.
    /// 2. **Cooldown** — if `elapsed_ms < cooldown_ms` → suppress.
    /// 3. **Hysteresis** — if `raw_value < threshold + hysteresis` → suppress.
    ///    Prevents rapid re-fire as a value oscillates around the threshold.
    /// 4. **Pass** — update state and return the decision.
    pub fn evaluate(&mut self, decision: Decision) -> Option<Decision> {
        // Look up cooldown + hysteresis for this rule (0 defaults = always pass).
        let (cooldown_ms, hysteresis) = self
            .policy
            .rules
            .iter()
            .find(|r| r.id == decision.rule_id)
            .map(|r| (r.cooldown_ms as i64, r.hysteresis))
            .unwrap_or((0, 0.0));

        let key = state_key(&decision.asset_id, &decision.rule_id);

        if let Some(entry) = self.state.get(&key) {
            // ── Rule 2: cooldown ──────────────────────────────────────────────
            let elapsed_ms = decision.ts - entry.last_fired_ts;
            if elapsed_ms < cooldown_ms {
                log::debug!(
                    "TrustEngine: suppressed {} (cooldown: {}ms remaining)",
                    decision.rule_id,
                    cooldown_ms - elapsed_ms
                );
                return None;
            }

            // ── Rule 3: hysteresis ────────────────────────────────────────────
            // Value must exceed (threshold + hysteresis) to avoid re-firing on
            // a signal oscillating just above the trigger threshold.
            let required = decision.threshold + hysteresis;
            if decision.raw_value < required {
                log::debug!(
                    "TrustEngine: suppressed {} (hysteresis: value {:.2} < required {:.2})",
                    decision.rule_id,
                    decision.raw_value,
                    required
                );
                return None;
            }
        }

        // ── Rule 1 / Rule 4: pass through — update state ──────────────────────
        self.state.insert(key, TrustEntry { last_fired_ts: decision.ts });

        Some(decision)
    }

    /// Filter a batch of decisions (e.g. multiple rules firing on one frame).
    /// Each decision is evaluated independently.
    pub fn evaluate_all(&mut self, decisions: Vec<Decision>) -> Vec<Decision> {
        decisions.into_iter().filter_map(|d| self.evaluate(d)).collect()
    }

    /// How many unique rule+asset keys are currently tracked in state.
    pub fn tracked_keys(&self) -> usize {
        self.state.len()
    }
}

fn state_key(asset_id: &str, rule_id: &str) -> String {
    format!("{asset_id}::{rule_id}")
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{
        Decision, DecisionSource, FeatureWindow, PolicyPack, PolicyRule, RuleGroup, RuleOperator,
        Severity, SignalMap, VehicleClass,
    };

    // ── Helpers ───────────────────────────────────────────────────────────────

    fn make_policy(cooldown_ms: u64, hysteresis: f64) -> PolicyPack {
        PolicyPack {
            version:       "test-1.0".into(),
            vehicle_class: VehicleClass::Simulator,
            rules: vec![
                PolicyRule {
                    id:          "test_rule".into(),
                    group:       RuleGroup::Thermal,
                    signal:      "coolant_slope".into(),
                    operator:    RuleOperator::Gt,
                    threshold:   100.0,
                    severity:    Severity::High,
                    cooldown_ms,
                    hysteresis,
                    description: "test rule".into(),
                },
            ],
        }
    }

    fn make_decision(asset_id: &str, rule_id: &str, ts: i64, raw_value: f64, threshold: f64) -> Decision {
        Decision {
            ts,
            asset_id:        asset_id.into(),
            severity:        Severity::High,
            rule_id:         rule_id.into(),
            rule_group:      RuleGroup::Thermal,
            confidence:      1.0,
            triggered_by:    vec!["coolant_slope".into()],
            raw_value,
            threshold,
            decision_source: DecisionSource::RuleEngine,
            context:         Some(empty_window()),
        }
    }

    fn empty_window() -> FeatureWindow {
        FeatureWindow {
            ts: 0, asset_id: "SIM-001".into(), window_seconds: 30.0,
            coolant_slope: 0.0, brake_spike_count: 0.0, speed_mean: 0.0,
            rpm_mean: 0.0, engine_load_mean: 0.0, throttle_variance: 0.0,
            hydraulic_spike: false, transmission_heat: false,
            dtc_new: vec![], signals_snapshot: SignalMap::default(),
        }
    }

    // ── Test 1: first fire always passes through ──────────────────────────────

    #[test]
    fn test_first_fire_always_passes_through() {
        let mut engine = TrustEngine::new(make_policy(30_000, 0.0));
        let d = make_decision("TRUCK-001", "test_rule", 1_000, 105.0, 100.0);
        assert!(engine.evaluate(d).is_some(), "first fire must always pass through");
    }

    // ── Test 2: second fire within cooldown is suppressed ─────────────────────

    #[test]
    fn test_second_fire_within_cooldown_is_suppressed() {
        let mut engine = TrustEngine::new(make_policy(30_000, 0.0));

        // First fire at t=1000ms
        engine.evaluate(make_decision("TRUCK-001", "test_rule", 1_000, 105.0, 100.0));

        // Second fire at t=5000ms — only 4 seconds elapsed, cooldown is 30s
        let result = engine.evaluate(make_decision("TRUCK-001", "test_rule", 5_000, 108.0, 100.0));
        assert!(result.is_none(), "second fire within cooldown must be suppressed");
    }

    // ── Test 3: after cooldown, same rule fires again ─────────────────────────

    #[test]
    fn test_after_cooldown_rule_fires_again() {
        let mut engine = TrustEngine::new(make_policy(30_000, 0.0));

        // First fire at t=0
        engine.evaluate(make_decision("TRUCK-001", "test_rule", 0, 105.0, 100.0));

        // Second fire at t=31s — past the 30s cooldown
        let result = engine.evaluate(make_decision("TRUCK-001", "test_rule", 31_000, 108.0, 100.0));
        assert!(result.is_some(), "fire after cooldown must pass through");
    }

    // ── Test 4: hysteresis suppresses re-fire at exact threshold ─────────────

    #[test]
    fn test_hysteresis_suppresses_at_threshold() {
        // Threshold = 100, hysteresis = 5 → re-fire requires raw_value >= 105
        let mut engine = TrustEngine::new(make_policy(0, 5.0));

        // First fire at t=0, value = 102 (above threshold 100)
        engine.evaluate(make_decision("TRUCK-001", "test_rule", 0, 102.0, 100.0));

        // Immediate re-fire (cooldown=0), value = 103 — below threshold+hysteresis (105)
        let result = engine.evaluate(make_decision("TRUCK-001", "test_rule", 1, 103.0, 100.0));
        assert!(result.is_none(), "value 103 below threshold+hysteresis 105 must be suppressed");
    }

    #[test]
    fn test_hysteresis_passes_when_value_exceeds_band() {
        // Threshold = 100, hysteresis = 5 → re-fire requires raw_value >= 105
        let mut engine = TrustEngine::new(make_policy(0, 5.0));

        // First fire
        engine.evaluate(make_decision("TRUCK-001", "test_rule", 0, 102.0, 100.0));

        // Re-fire with value = 106 — at or above threshold+hysteresis (105)
        let result = engine.evaluate(make_decision("TRUCK-001", "test_rule", 1, 106.0, 100.0));
        assert!(result.is_some(), "value 106 above threshold+hysteresis 105 must pass through");
    }

    // ── Test 5: different asset_ids have independent cooldown state ────────────

    #[test]
    fn test_different_assets_have_independent_cooldown() {
        let mut engine = TrustEngine::new(make_policy(30_000, 0.0));

        // TRUCK-001 fires at t=0
        engine.evaluate(make_decision("TRUCK-001", "test_rule", 0, 105.0, 100.0));

        // TRUCK-002 fires same rule at t=1000ms — DIFFERENT asset, should pass
        let result = engine.evaluate(make_decision("TRUCK-002", "test_rule", 1_000, 105.0, 100.0));
        assert!(result.is_some(), "different asset_id must have independent cooldown");

        // TRUCK-001 fires again within its own cooldown — must be suppressed
        let suppressed = engine.evaluate(make_decision("TRUCK-001", "test_rule", 2_000, 108.0, 100.0));
        assert!(suppressed.is_none(), "TRUCK-001 must still be in cooldown");
    }

    // ── Test 6: unknown rule_id passes through (no policy entry = no constraint)

    #[test]
    fn test_unknown_rule_id_passes_through_always() {
        let mut engine = TrustEngine::new(make_policy(30_000, 0.0));

        // Fire with a rule_id not in the policy
        engine.evaluate(make_decision("TRUCK-001", "ai_anomaly_class_3", 0, 0.9, 0.65));

        // Re-fire immediately — no cooldown for unknown rules
        let result = engine.evaluate(make_decision("TRUCK-001", "ai_anomaly_class_3", 1, 0.9, 0.65));
        assert!(result.is_some(), "unknown rule_id (e.g. AI class) must always pass through");
    }

    // ── Test 7: evaluate_all filters a batch correctly ────────────────────────

    #[test]
    fn test_evaluate_all_filters_batch() {
        let mut engine = TrustEngine::new(make_policy(30_000, 0.0));

        // First batch: two different rules, both pass
        let batch1 = vec![
            make_decision("TRUCK-001", "test_rule", 0, 105.0, 100.0),
            make_decision("TRUCK-001", "test_rule_2", 0, 50.0, 40.0),
        ];
        let passed = engine.evaluate_all(batch1);
        assert_eq!(passed.len(), 2, "both new rules must pass through");

        // Second batch: same rules immediately — test_rule suppressed, test_rule_2 also
        let batch2 = vec![
            make_decision("TRUCK-001", "test_rule", 100, 106.0, 100.0),
            make_decision("TRUCK-001", "test_rule_2", 100, 51.0, 40.0),
        ];
        let passed2 = engine.evaluate_all(batch2);
        // test_rule has 30s cooldown and is suppressed; test_rule_2 not in policy so passes
        assert_eq!(passed2.len(), 1, "test_rule suppressed, test_rule_2 passes (unknown rule)");
    }

    // ── Test 8: state is updated only on pass-through ─────────────────────────

    #[test]
    fn test_state_update_only_on_passthrough() {
        // Cooldown=0, hysteresis=10 → second fire suppressed if value < threshold+10
        let mut engine = TrustEngine::new(make_policy(0, 10.0));

        engine.evaluate(make_decision("TRUCK-001", "test_rule", 0, 102.0, 100.0));
        // Suppressed: 104 < 110
        engine.evaluate(make_decision("TRUCK-001", "test_rule", 1, 104.0, 100.0));
        // The last_fired_ts should still be 0 (not updated to 1 because suppressed)
        // Now fire at 106 — still < 110, still suppressed
        let result = engine.evaluate(make_decision("TRUCK-001", "test_rule", 2, 106.0, 100.0));
        assert!(result.is_none());

        // Fire at 111 — above 110, must pass
        let result2 = engine.evaluate(make_decision("TRUCK-001", "test_rule", 3, 111.0, 100.0));
        assert!(result2.is_some(), "value 111 above threshold+hysteresis 110 must pass");
    }
}
