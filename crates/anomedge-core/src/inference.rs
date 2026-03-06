//! inference.rs
//! InferenceChain — three-tier fallback orchestrator.
//!
//! Tier 1 → Edge AI (INT8 ONNX, < 50ms, confidence ≥ 0.65)
//! Tier 2 → ML Statistical (Isolation Forest, ≥ 5 samples)
//! Tier 3 → Rule Engine (YAML thresholds, ALWAYS fires, never skips)
//!
//! Each tier falls through to the next if it can't produce a confident result.
//! Tier 3 is the safety net: it always returns a decision (possibly NORMAL).
//!
//! Phase 0: Tier 1 and Tier 2 are structurally wired but not yet active.
//!   - Tier 1 activates in Day 14 when the ONNX model is integrated.
//!   - Tier 2 activates with a statistical model in a later phase.
//!   Until then, every call falls through to Tier 3 (RuleEngine).

use std::time::Instant;

use crate::rules::RuleEngine;
use crate::types::{Decision, DecisionSource, FeatureWindow};

// ─── Chain configuration ──────────────────────────────────────────────────────

/// Maximum wall-clock time allowed for Edge AI inference before falling through.
pub const EDGE_AI_TIMEOUT_MS: u64 = 50;

/// Minimum confidence score from Edge AI to accept its result.
pub const EDGE_AI_MIN_CONFIDENCE: f64 = 0.65;

/// Minimum sample count in the FeatureEngine window before Tier 2 may fire.
pub const ML_MIN_SAMPLES: usize = 5;

// ─── Context passed to evaluate() ────────────────────────────────────────────

/// Caller-supplied context for each evaluate() call.
pub struct InferenceContext {
    /// True if an INT8 ONNX model is loaded and ready.
    /// When false, Tier 1 is unconditionally skipped.
    pub model_available: bool,
    /// Current sample count in the rolling FeatureEngine window for this asset.
    /// When < ML_MIN_SAMPLES, Tier 2 is skipped.
    pub sample_count:    usize,
}

impl InferenceContext {
    /// Convenience constructor: no model, no history (pure Phase-0 state).
    pub fn phase0() -> Self {
        Self { model_available: false, sample_count: 0 }
    }

    pub fn with_samples(sample_count: usize) -> Self {
        Self { model_available: false, sample_count }
    }
}

// ─── Result ───────────────────────────────────────────────────────────────────

/// Which inference tier produced the decisions in a `ChainResult`.
#[derive(Debug, Clone, PartialEq)]
pub enum TierUsed {
    /// Tier 1: INT8 ONNX model (not yet active in Phase 0).
    EdgeAi,
    /// Tier 2: ML statistical detection (not yet active in Phase 0).
    MlStatistical,
    /// Tier 3: Rule engine — always the final fallback.
    RuleEngine,
}

/// Output of one `InferenceChain::evaluate()` call.
#[derive(Debug, Clone)]
pub struct ChainResult {
    /// Decisions produced by the winning tier.
    /// May be empty if no thresholds/patterns were triggered.
    pub decisions:  Vec<Decision>,
    /// Which tier produced `decisions`.
    pub tier_used:  TierUsed,
    /// Total evaluate() wall-clock time in microseconds.
    pub latency_us: u64,
    /// Human-readable notes on why earlier tiers were skipped.
    pub skipped:    Vec<&'static str>,
}

// ─── InferenceChain ───────────────────────────────────────────────────────────

/// Stateless fallback orchestrator.
///
/// Borrows a `RuleEngine` (Tier 3). Tiers 1 and 2 are injected via
/// `InferenceContext`; when a model becomes available the context signals it.
pub struct InferenceChain<'a> {
    rule_engine: &'a RuleEngine,
}

impl<'a> InferenceChain<'a> {
    pub fn new(rule_engine: &'a RuleEngine) -> Self {
        Self { rule_engine }
    }

    /// Run the three-tier fallback chain for a single `FeatureWindow`.
    ///
    /// Always returns a `ChainResult` — never panics, never blocks indefinitely.
    pub fn evaluate(&self, window: &FeatureWindow, ctx: &InferenceContext) -> ChainResult {
        let start   = Instant::now();
        let mut skipped: Vec<&'static str> = Vec::new();

        // ── TIER 1: Edge AI ──────────────────────────────────────────────────
        // Phase 0: model not loaded → always skip.
        // Phase 1 (Day 14): replace the branch body with ONNX inference +
        //   timeout guard + confidence threshold check.
        if ctx.model_available {
            // TODO (Day 14): run ONNX session with tokio::time::timeout.
            // For now, mark as not-yet-implemented and fall through.
            skipped.push("edge_ai: model available but inference not yet implemented (Phase 0)");
        } else {
            skipped.push("edge_ai: model not loaded");
        }

        // ── TIER 2: ML Statistical ───────────────────────────────────────────
        // Phase 0: no in-memory model → skip.
        // Phase 1: load pre-computed Isolation Forest parameters and run
        //   anomaly scoring on the FeatureWindow's numeric features.
        if ctx.sample_count >= ML_MIN_SAMPLES {
            // TODO (Phase 1): run statistical anomaly detection.
            // For now, fall through to Rule Engine.
            skipped.push("ml_statistical: sufficient samples but model not yet implemented (Phase 0)");
        } else {
            skipped.push("ml_statistical: insufficient samples");
        }

        // ── TIER 3: Rule Engine — ALWAYS fires ───────────────────────────────
        let decisions: Vec<Decision> = self.rule_engine
            .evaluate(window)
            .into_iter()
            .map(|mut d| {
                // Ensure decision_source reflects Tier 3.
                d.decision_source = DecisionSource::RuleEngine;
                d
            })
            .collect();

        ChainResult {
            decisions,
            tier_used:  TierUsed::RuleEngine,
            latency_us: start.elapsed().as_micros() as u64,
            skipped,
        }
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::RuleEngine;
    use crate::types::{
        FeatureWindow, PolicyPack, PolicyRule, RuleGroup, RuleOperator, Severity,
        SignalMap, VehicleClass,
    };

    // ── Helpers ───────────────────────────────────────────────────────────────

    fn make_engine_with_rule(
        signal: &str,
        op: RuleOperator,
        threshold: f64,
        severity: Severity,
    ) -> RuleEngine {
        RuleEngine::new(PolicyPack {
            version:       "test-1.0".into(),
            vehicle_class: VehicleClass::Simulator,
            rules: vec![PolicyRule {
                id:          "test_rule".into(),
                group:       RuleGroup::Thermal,
                signal:      signal.into(),
                operator:    op,
                threshold,
                severity,
                cooldown_ms: 30_000,
                hysteresis:  0.0,
                description: "test".into(),
            }],
        })
    }

    fn empty_engine() -> RuleEngine {
        RuleEngine::new(PolicyPack {
            version:       "test-1.0".into(),
            vehicle_class: VehicleClass::Simulator,
            rules:         vec![],
        })
    }

    fn empty_window() -> FeatureWindow {
        FeatureWindow {
            ts: 1_700_000_000_000, asset_id: "SIM-001".into(), window_seconds: 30.0,
            coolant_slope: 0.0, brake_spike_count: 0.0, speed_mean: 0.0,
            rpm_mean: 0.0, engine_load_mean: 0.0, throttle_variance: 0.0,
            hydraulic_spike: false, transmission_heat: false,
            dtc_new: vec![], signals_snapshot: SignalMap::default(),
        }
    }

    fn window_with_slope(slope: f64) -> FeatureWindow {
        FeatureWindow { coolant_slope: slope, ..empty_window() }
    }

    // ── Test 1: phase0 context → tier 3 fires ────────────────────────────────

    #[test]
    fn test_phase0_context_uses_rule_engine() {
        let engine = make_engine_with_rule(
            "coolant_slope", RuleOperator::Gt, 0.8, Severity::Critical,
        );
        let chain = InferenceChain::new(&engine);

        let result = chain.evaluate(&window_with_slope(1.0), &InferenceContext::phase0());

        assert_eq!(result.tier_used, TierUsed::RuleEngine);
        assert_eq!(result.decisions.len(), 1);
        assert_eq!(result.decisions[0].rule_id, "test_rule");
    }

    // ── Test 2: model available still falls through to tier 3 in phase 0 ─────

    #[test]
    fn test_model_available_still_falls_through_phase0() {
        let engine = make_engine_with_rule(
            "coolant_slope", RuleOperator::Gt, 0.8, Severity::Critical,
        );
        let chain = InferenceChain::new(&engine);

        let ctx = InferenceContext { model_available: true, sample_count: 10 };
        let result = chain.evaluate(&window_with_slope(1.0), &ctx);

        assert_eq!(result.tier_used, TierUsed::RuleEngine);
        // Both tier 1 and tier 2 should be noted as skipped
        assert!(result.skipped.iter().any(|s| s.contains("edge_ai")));
        assert!(result.skipped.iter().any(|s| s.contains("ml_statistical")));
    }

    // ── Test 3: decisions have decision_source = RuleEngine ───────────────────

    #[test]
    fn test_decisions_have_rule_engine_source() {
        let engine = make_engine_with_rule(
            "coolant_slope", RuleOperator::Gt, 0.8, Severity::Critical,
        );
        let chain = InferenceChain::new(&engine);

        let result = chain.evaluate(&window_with_slope(1.0), &InferenceContext::phase0());

        assert_eq!(result.decisions[0].decision_source, DecisionSource::RuleEngine);
    }

    // ── Test 4: no rules fire → empty decisions, tier 3 still used ───────────

    #[test]
    fn test_no_rules_fire_returns_empty_decisions() {
        let engine = make_engine_with_rule(
            "coolant_slope", RuleOperator::Gt, 0.8, Severity::Critical,
        );
        let chain = InferenceChain::new(&engine);

        // slope = 0.5 < threshold 0.8 — no rule fires
        let result = chain.evaluate(&window_with_slope(0.5), &InferenceContext::phase0());

        assert_eq!(result.tier_used, TierUsed::RuleEngine);
        assert!(result.decisions.is_empty(), "no rules triggered → empty decisions");
    }

    // ── Test 5: latency is tracked ────────────────────────────────────────────

    #[test]
    fn test_latency_is_tracked() {
        let engine = empty_engine();
        let chain  = InferenceChain::new(&engine);

        let result = chain.evaluate(&empty_window(), &InferenceContext::phase0());

        // latency_us should be populated (even if 0 on fast hardware, the field exists)
        // We just check it doesn't overflow or panic
        let _ = result.latency_us;
    }

    // ── Test 6: multiple rules fire simultaneously ────────────────────────────

    #[test]
    fn test_multiple_rules_fire_simultaneously() {
        let engine = RuleEngine::new(PolicyPack {
            version:       "test-1.0".into(),
            vehicle_class: VehicleClass::Simulator,
            rules: vec![
                PolicyRule {
                    id: "rule_a".into(), group: RuleGroup::Thermal,
                    signal: "coolant_slope".into(), operator: RuleOperator::Gt,
                    threshold: 0.5, severity: Severity::Warn,
                    cooldown_ms: 0, hysteresis: 0.0, description: "a".into(),
                },
                PolicyRule {
                    id: "rule_b".into(), group: RuleGroup::Braking,
                    signal: "brake_spike_count".into(), operator: RuleOperator::Gte,
                    threshold: 2.0, severity: Severity::High,
                    cooldown_ms: 0, hysteresis: 0.0, description: "b".into(),
                },
            ],
        });
        let chain = InferenceChain::new(&engine);

        let window = FeatureWindow {
            coolant_slope:     1.0,
            brake_spike_count: 3.0,
            ..empty_window()
        };

        let result = chain.evaluate(&window, &InferenceContext::phase0());
        assert_eq!(result.decisions.len(), 2, "both rules must fire");
    }

    // ── Test 7: skipped list always populated ─────────────────────────────────

    #[test]
    fn test_skipped_list_always_populated() {
        let engine = empty_engine();
        let chain  = InferenceChain::new(&engine);

        let result = chain.evaluate(&empty_window(), &InferenceContext::phase0());

        assert!(!result.skipped.is_empty(), "skipped list must always explain fallthrough path");
        assert_eq!(result.skipped.len(), 2, "both tier 1 and tier 2 must be noted");
    }

    // ── Test 8: with_samples convenience constructor ──────────────────────────

    #[test]
    fn test_with_samples_context_marks_ml_as_skipped_with_note() {
        let engine = empty_engine();
        let chain  = InferenceChain::new(&engine);

        let ctx    = InferenceContext::with_samples(10);
        let result = chain.evaluate(&empty_window(), &ctx);

        // Tier 2 should note "sufficient samples but model not yet implemented"
        let ml_note = result.skipped.iter().any(|s| s.contains("sufficient samples"));
        assert!(ml_note, "with sample_count >= 5, tier 2 skip note must mention sufficient samples");
    }

    // ── Test 9: insufficient samples skips tier 2 with different note ─────────

    #[test]
    fn test_insufficient_samples_note_for_tier2() {
        let engine = empty_engine();
        let chain  = InferenceChain::new(&engine);

        let ctx    = InferenceContext::with_samples(3);
        let result = chain.evaluate(&empty_window(), &ctx);

        let ml_note = result.skipped.iter().any(|s| s.contains("insufficient samples"));
        assert!(ml_note, "sample_count < 5 must note insufficient samples");
    }

    // ── Test 10: evaluate with real policy.yaml round-trip ───────────────────

    #[test]
    fn test_evaluate_with_yaml_policy() {
        let yaml = r#"
version: "test-1.0"
vehicle_class: SIMULATOR
rules:
  - id: coolant_rising_fast
    group: thermal
    signal: coolant_slope
    operator: gt
    threshold: 0.8
    severity: CRITICAL
    cooldown_ms: 20000
    hysteresis: 0.1
    description: "rapid coolant rise"
  - id: speed_high
    group: speed
    signal: speed_mean
    operator: gt
    threshold: 110.0
    severity: HIGH
    cooldown_ms: 30000
    hysteresis: 2.0
    description: "over speed"
"#;
        let engine = RuleEngine::from_yaml(yaml).expect("YAML must parse");
        let chain  = InferenceChain::new(&engine);

        let window = FeatureWindow {
            coolant_slope: 1.2,
            speed_mean:    115.0,
            ..empty_window()
        };

        let result = chain.evaluate(&window, &InferenceContext::phase0());
        assert_eq!(result.decisions.len(), 2, "both rules must fire from YAML policy");

        let ids: Vec<&str> = result.decisions.iter().map(|d| d.rule_id.as_str()).collect();
        assert!(ids.contains(&"coolant_rising_fast"));
        assert!(ids.contains(&"speed_high"));
    }
}
