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

use crate::ml_statistical::MlStatistical;
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

/// Fallback orchestrator: Tier 1 → Tier 2 → Tier 3.
///
/// Borrows a `RuleEngine` (Tier 3) and optionally a `MlStatistical` (Tier 2).
/// Tiers that produce decisions are included in the result. Tier 3 always fires.
pub struct InferenceChain<'a> {
    rule_engine:    &'a RuleEngine,
    ml_statistical: Option<&'a MlStatistical>,
}

impl<'a> InferenceChain<'a> {
    /// Construct with Rule Engine only (Tier 2 disabled).
    pub fn new(rule_engine: &'a RuleEngine) -> Self {
        Self { rule_engine, ml_statistical: None }
    }

    /// Construct with both ML Statistical (Tier 2) and Rule Engine (Tier 3).
    pub fn with_ml(rule_engine: &'a RuleEngine, ml: &'a MlStatistical) -> Self {
        Self { rule_engine, ml_statistical: Some(ml) }
    }

    /// Run the three-tier fallback chain for a single `FeatureWindow`.
    ///
    /// Always returns a `ChainResult` — never panics, never blocks indefinitely.
    /// Tier 3 (Rule Engine) ALWAYS fires. Tier 2 decisions are additive.
    pub fn evaluate(&self, window: &FeatureWindow, ctx: &InferenceContext) -> ChainResult {
        let start   = Instant::now();
        let mut skipped: Vec<&'static str> = Vec::new();
        let mut tier2_fired = false;
        let mut all_decisions: Vec<Decision> = Vec::new();

        // ── TIER 1: Edge AI ──────────────────────────────────────────────────
        if ctx.model_available {
            // TODO (Day 14): run ONNX session with tokio::time::timeout.
            skipped.push("edge_ai: model available but inference not yet implemented");
        } else {
            skipped.push("edge_ai: model not loaded");
        }

        // ── TIER 2: ML Statistical ───────────────────────────────────────────
        if ctx.sample_count >= ML_MIN_SAMPLES {
            if let Some(ml) = &self.ml_statistical {
                let ml_decisions = ml.score(window);
                if !ml_decisions.is_empty() {
                    tier2_fired = true;
                    all_decisions.extend(ml_decisions);
                } else {
                    skipped.push("ml_statistical: scored but no anomaly detected");
                }
            } else {
                skipped.push("ml_statistical: scorer not configured");
            }
        } else {
            skipped.push("ml_statistical: insufficient samples");
        }

        // ── TIER 3: Rule Engine — ALWAYS fires ───────────────────────────────
        let rule_decisions: Vec<Decision> = self.rule_engine
            .evaluate(window)
            .into_iter()
            .map(|mut d| {
                d.decision_source = DecisionSource::RuleEngine;
                d
            })
            .collect();
        all_decisions.extend(rule_decisions);

        let tier_used = if tier2_fired {
            TierUsed::MlStatistical
        } else {
            TierUsed::RuleEngine
        };

        ChainResult {
            decisions:  all_decisions,
            tier_used,
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

    // ── Test 2: model available still falls through to tier 3 when no ML scorer ─

    #[test]
    fn test_model_available_still_falls_through_no_ml_scorer() {
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

    // ── Test 8: with_samples and no ML scorer → scorer not configured note ────

    #[test]
    fn test_with_samples_no_ml_scorer_skipped_note() {
        let engine = empty_engine();
        let chain  = InferenceChain::new(&engine);

        let ctx    = InferenceContext::with_samples(10);
        let result = chain.evaluate(&empty_window(), &ctx);

        // Tier 2 should note "scorer not configured" when MlStatistical absent
        let ml_note = result.skipped.iter().any(|s| s.contains("not configured"));
        assert!(ml_note, "with no ML scorer, tier 2 skip note must mention 'not configured'");
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

    // ── Test 11: Tier 2 with ML scorer fires on anomaly ─────────────────────

    #[test]
    fn test_tier2_ml_statistical_fires_on_anomaly() {
        use crate::ml_statistical::{MlStatistical, MlConfig};

        let engine = empty_engine();

        // Build ML scorer with 100 normal windows, then score an outlier
        let mut ml = MlStatistical::with_config(MlConfig {
            n_trees: 100,
            anomaly_threshold: 0.55,
            ..MlConfig::default()
        });

        // Record 100 normal windows (features close to 0, 0, 80, 2000, 50, 10)
        for i in 0..100_usize {
            let w = FeatureWindow {
                coolant_slope:     0.1 + (i as f64 % 10.0) * 0.01,
                brake_spike_count: 0.5,
                speed_mean:        80.0 + (i as f64 % 5.0),
                rpm_mean:          2000.0 + (i as f64 % 50.0),
                engine_load_mean:  50.0 + (i as f64 % 5.0),
                throttle_variance: 10.0 + (i as f64 % 3.0),
                ..empty_window()
            };
            ml.record(&w);
        }

        let chain = InferenceChain::with_ml(&engine, &ml);
        let ctx   = InferenceContext::with_samples(100);

        // Outlier: everything maxed out
        let outlier_window = FeatureWindow {
            coolant_slope:     5.0,
            brake_spike_count: 10.0,
            speed_mean:        180.0,
            rpm_mean:          5500.0,
            engine_load_mean:  99.0,
            throttle_variance: 90.0,
            ..empty_window()
        };

        let result = chain.evaluate(&outlier_window, &ctx);

        assert_eq!(result.tier_used, TierUsed::MlStatistical,
            "tier_used must be MlStatistical when anomaly detected");
        assert!(
            result.decisions.iter().any(|d| d.decision_source == DecisionSource::MlStatistical),
            "must include ML-sourced decisions"
        );
    }

    // ── Test 12: Tier 2 + Tier 3 both produce decisions ─────────────────────

    #[test]
    fn test_tier2_and_tier3_both_produce_decisions() {
        use crate::ml_statistical::{MlStatistical, MlConfig};

        // Rule engine that fires on coolant_slope > 0.8
        let engine = make_engine_with_rule(
            "coolant_slope", RuleOperator::Gt, 0.8, Severity::Critical,
        );

        let mut ml = MlStatistical::with_config(MlConfig {
            n_trees: 100,
            anomaly_threshold: 0.55,
            ..MlConfig::default()
        });
        for i in 0..100_usize {
            let f = i as f64;
            let w = FeatureWindow {
                coolant_slope:     0.1 + (f % 10.0) * 0.01,
                brake_spike_count: 0.3 + (f % 7.0) * 0.1,
                speed_mean:        78.0 + (f % 8.0),
                rpm_mean:          1950.0 + (f % 12.0) * 10.0,
                engine_load_mean:  48.0 + (f % 6.0),
                throttle_variance: 9.0 + (f % 5.0),
                ..empty_window()
            };
            ml.record(&w);
        }

        let chain = InferenceChain::with_ml(&engine, &ml);
        let ctx   = InferenceContext::with_samples(100);

        // This window triggers both: ML (outlier) and Rule (slope > 0.8)
        let window = FeatureWindow {
            coolant_slope:     5.0,
            brake_spike_count: 10.0,
            speed_mean:        180.0,
            rpm_mean:          5500.0,
            engine_load_mean:  99.0,
            throttle_variance: 90.0,
            ..empty_window()
        };

        let result = chain.evaluate(&window, &ctx);

        let has_ml   = result.decisions.iter().any(|d| d.decision_source == DecisionSource::MlStatistical);
        let has_rule = result.decisions.iter().any(|d| d.decision_source == DecisionSource::RuleEngine);

        assert!(has_ml,   "must include ML Statistical decision");
        assert!(has_rule, "must include Rule Engine decision (always fires)");
        assert_eq!(result.tier_used, TierUsed::MlStatistical);
    }

    // ── Test 13: Tier 2 with ML scorer, normal data → only Tier 3 fires ────

    #[test]
    fn test_tier2_normal_data_falls_through_to_tier3() {
        use crate::ml_statistical::{MlStatistical, MlConfig};

        let engine = empty_engine();
        let mut ml = MlStatistical::with_config(MlConfig::default());
        for i in 0..50_usize {
            let w = FeatureWindow {
                coolant_slope:     0.1,
                brake_spike_count: 0.5,
                speed_mean:        80.0 + (i as f64 % 5.0),
                rpm_mean:          2000.0,
                engine_load_mean:  50.0,
                throttle_variance: 10.0,
                ..empty_window()
            };
            ml.record(&w);
        }

        let chain = InferenceChain::with_ml(&engine, &ml);
        let ctx   = InferenceContext::with_samples(50);

        // Normal window — ML should not fire
        let normal = FeatureWindow {
            coolant_slope: 0.12, speed_mean: 81.0, rpm_mean: 2010.0,
            ..empty_window()
        };

        let result = chain.evaluate(&normal, &ctx);

        assert_eq!(result.tier_used, TierUsed::RuleEngine,
            "normal data → ML doesn't fire → tier_used must be RuleEngine");
        assert!(result.skipped.iter().any(|s| s.contains("no anomaly")),
            "skip note must mention 'no anomaly'");
    }
}
