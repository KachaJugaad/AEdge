//! pipeline.rs
//! Pipeline — wires FeatureEngine → InferenceChain → TrustEngine.
//!
//! Single entry-point `process(SignalEvent)` moves a raw telemetry frame all
//! the way through to `decisions.gated`-ready output.
//!
//! Call order per frame:
//!   1. FeatureEngine::ingest()  → FeatureWindow  (signals.features topic)
//!   2. InferenceChain::evaluate() → ChainResult   (decisions topic)
//!   3. TrustEngine::evaluate_all() → Vec<Decision> (decisions.gated topic)
//!
//! The Pipeline owns all three engines. RuleEngine is also owned here and
//! loaned to InferenceChain on each process() call (no heap allocation).

use crate::feature::FeatureEngine;
use crate::inference::{ChainResult, InferenceChain, InferenceContext};
use crate::rules::RuleEngine;
use crate::trust::TrustEngine;
use crate::types::{Decision, FeatureWindow, PolicyPack, SignalEvent};

// ─── PipelineResult ───────────────────────────────────────────────────────────

/// Everything produced by one `process()` call.
#[derive(Debug)]
pub struct PipelineResult {
    /// Computed feature window — publish to `signals.features`.
    pub window:          FeatureWindow,
    /// Raw inference output (all decisions before trust filter).
    pub chain_result:    ChainResult,
    /// Trust-filtered decisions — publish to `decisions.gated`.
    pub gated_decisions: Vec<Decision>,
}

impl PipelineResult {
    /// True if at least one decision survived the trust filter.
    pub fn has_alerts(&self) -> bool {
        !self.gated_decisions.is_empty()
    }

    /// Highest severity among gated decisions, or None if none fired.
    pub fn max_severity(&self) -> Option<&crate::types::Severity> {
        self.gated_decisions.iter().map(|d| &d.severity).max()
    }
}

// ─── Pipeline ────────────────────────────────────────────────────────────────

/// Full signal-to-gated-decision processing pipeline.
pub struct Pipeline {
    feature_engine: FeatureEngine,
    rule_engine:    RuleEngine,
    trust_engine:   TrustEngine,
}

impl Pipeline {
    /// Construct from an already-parsed `PolicyPack`.
    /// The pack is cloned once — `RuleEngine` and `TrustEngine` each own a copy.
    pub fn new(policy: PolicyPack) -> Self {
        Self {
            feature_engine: FeatureEngine::new(),
            rule_engine:    RuleEngine::new(policy.clone()),
            trust_engine:   TrustEngine::new(policy),
        }
    }

    /// Convenience: parse `policy.yaml` and construct.
    pub fn from_yaml(yaml: &str) -> Result<Self, serde_yaml::Error> {
        let policy: PolicyPack = serde_yaml::from_str(yaml)?;
        Ok(Self::new(policy))
    }

    /// Load the canonical policy file at the given path and construct.
    pub fn from_policy_file(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let yaml = std::fs::read_to_string(path)?;
        Ok(Self::from_yaml(&yaml)?)
    }

    /// Process one `SignalEvent` through the full pipeline.
    ///
    /// Internally creates an `InferenceChain` view over the owned `RuleEngine`
    /// — no allocation, no Arc, Tier-1/2 stubs fall through to Tier-3.
    pub fn process(&mut self, event: SignalEvent) -> PipelineResult {
        // ── Step 1: Feature computation ───────────────────────────────────────
        let asset_id = event.asset_id.clone();
        let window   = self.feature_engine.ingest(event);
        let samples  = self.feature_engine.sample_count(&asset_id);

        // ── Step 2: Inference chain ───────────────────────────────────────────
        let chain        = InferenceChain::new(&self.rule_engine);
        let ctx          = InferenceContext::with_samples(samples);
        let chain_result = chain.evaluate(&window, &ctx);

        // ── Step 3: Trust filter ──────────────────────────────────────────────
        let gated_decisions = self.trust_engine
            .evaluate_all(chain_result.decisions.clone());

        PipelineResult { window, chain_result, gated_decisions }
    }

    /// Process a batch of events in arrival order and collect all gated results.
    /// Useful for running scenario files through the full pipeline in tests.
    pub fn process_batch(&mut self, events: Vec<SignalEvent>) -> Vec<PipelineResult> {
        events.into_iter().map(|e| self.process(e)).collect()
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{PolicyRule, RuleGroup, RuleOperator, Severity, SignalMap,
                       SignalSource, VehicleClass};

    // ── Helpers ───────────────────────────────────────────────────────────────

    fn make_policy(rules: Vec<PolicyRule>) -> PolicyPack {
        PolicyPack {
            version:       "test-1.0".into(),
            vehicle_class: VehicleClass::Simulator,
            rules,
        }
    }

    fn rule_coolant_slope(threshold: f64, severity: Severity, cooldown_ms: u64) -> PolicyRule {
        PolicyRule {
            id:          "coolant_rising_fast".into(),
            group:       RuleGroup::Thermal,
            signal:      "coolant_slope".into(),
            operator:    RuleOperator::Gt,
            threshold,
            severity,
            cooldown_ms,
            hysteresis:  0.0,
            description: "test coolant slope".into(),
        }
    }

    fn rule_coolant_temp(threshold: f64, severity: Severity) -> PolicyRule {
        PolicyRule {
            id:          "coolant_high_temp".into(),
            group:       RuleGroup::Thermal,
            signal:      "signals_snapshot.coolant_temp".into(),
            operator:    RuleOperator::Gt,
            threshold,
            severity,
            cooldown_ms: 30_000,
            hysteresis:  2.0,
            description: "test coolant temp".into(),
        }
    }

    fn make_event(asset_id: &str, ts: i64, signals: SignalMap) -> SignalEvent {
        SignalEvent {
            ts,
            asset_id:  asset_id.into(),
            driver_id: "DRV-001".into(),
            source:    SignalSource::Obd2Generic,
            signals,
            raw_frame: None,
        }
    }

    fn coolant_event(ts: i64, temp: f64) -> SignalEvent {
        make_event("TRUCK-001", ts, SignalMap { coolant_temp: Some(temp), ..Default::default() })
    }

    // ── Test 1: single event produces window + no alerts below threshold ───────

    #[test]
    fn test_single_event_no_alert() {
        let mut pipeline = Pipeline::new(make_policy(vec![
            rule_coolant_slope(0.8, Severity::Critical, 20_000),
        ]));
        let result = pipeline.process(coolant_event(0, 85.0));

        // One sample → slope = 0 → no alert
        assert!(!result.has_alerts(), "single event must not trigger slope rule");
        assert_eq!(result.window.asset_id, "TRUCK-001");
    }

    // ── Test 2: rising coolant crosses threshold → gated decision produced ─────

    #[test]
    fn test_rising_coolant_triggers_gated_decision() {
        // cooldown=0 so every trigger passes TrustEngine.
        let mut pipeline = Pipeline::new(make_policy(vec![
            rule_coolant_slope(0.8, Severity::Critical, 0),
        ]));

        // 10 events at constant 85°C (slope = 0, no fire on any of them).
        // Final event: big jump to 105°C.
        // Slope = (105 - 85) / 11 = 1.82 > 0.8 → fires for the first time here.
        for i in 0..10_i64 {
            pipeline.process(coolant_event(i * 1_000, 85.0));
        }
        let result = pipeline.process(coolant_event(10_000, 105.0));

        assert!(result.has_alerts(), "fast-rising coolant must trigger alert");
        assert_eq!(result.gated_decisions[0].rule_id, "coolant_rising_fast");
        assert_eq!(result.gated_decisions[0].severity, Severity::Critical);
    }

    // ── Test 3: TrustEngine suppresses second fire within cooldown ────────────

    #[test]
    fn test_trust_engine_suppresses_second_fire_within_cooldown() {
        let mut pipeline = Pipeline::new(make_policy(vec![
            rule_coolant_slope(0.8, Severity::High, 30_000), // 30s cooldown
        ]));

        // 10 events at constant 85°C (slope=0, no fire).
        for i in 0..10_i64 {
            pipeline.process(coolant_event(i * 1_000, 85.0));
        }
        // First trigger: big jump at t=10s. slope = (110-85)/11 = 2.27 > 0.8.
        // TrustEngine records t=10_000 as last_fired_ts.
        let first = pipeline.process(coolant_event(10_000, 110.0));
        assert!(first.has_alerts(), "first trigger must pass trust filter");

        // Second fire at t=11s — only 1s elapsed, cooldown=30s → suppress.
        // The new event is still in rising range so slope still > 0.8.
        let suppressed = pipeline.process(coolant_event(11_000, 115.0));
        assert!(
            !suppressed.has_alerts(),
            "second fire 1s after first must be suppressed by 30s cooldown"
        );
    }

    // ── Test 4: after cooldown, alert fires again ─────────────────────────────

    #[test]
    fn test_alert_fires_again_after_cooldown() {
        let mut pipeline = Pipeline::new(make_policy(vec![
            rule_coolant_slope(0.8, Severity::High, 5_000), // 5s cooldown
        ]));

        // 10 events at constant 85°C, then first trigger at t=10s.
        for i in 0..10_i64 {
            pipeline.process(coolant_event(i * 1_000, 85.0));
        }
        let first = pipeline.process(coolant_event(10_000, 110.0));
        assert!(first.has_alerts(), "first trigger must fire");

        // Flush window past first-batch events (all older than 30s from t=45s).
        // Feed 10 constant events at t=15s..24s so slope resets to 0.
        for i in 0..10_i64 {
            pipeline.process(coolant_event(15_000 + i * 1_000, 90.0));
        }
        // Final event at t=25s: big jump. Elapsed since t=10s = 15s > 5s cooldown.
        // Slope = (120-90)/11 = 2.73 > 0.8 → passes both RuleEngine and TrustEngine.
        let second = pipeline.process(coolant_event(25_000, 120.0));
        assert!(second.has_alerts(), "alert must fire again after 5s cooldown expires");
    }

    // ── Test 5: multiple rules fire independently ─────────────────────────────

    #[test]
    fn test_multiple_rules_fire_in_one_frame() {
        let mut pipeline = Pipeline::new(make_policy(vec![
            rule_coolant_slope(0.5, Severity::Critical, 0),
            rule_coolant_temp(100.0, Severity::High),
        ]));

        // Feed events with both rising slope AND temp > 100
        for i in 0..10_i64 {
            pipeline.process(coolant_event(i * 1_000, 90.0 + i as f64));
        }
        let result = pipeline.process(coolant_event(10_000, 105.0));

        // Both rules should fire and be gated
        assert_eq!(
            result.gated_decisions.len(), 2,
            "both slope and temp rules must fire: {:?}",
            result.gated_decisions.iter().map(|d| &d.rule_id).collect::<Vec<_>>()
        );
    }

    // ── Test 6: max_severity returns correct severity ─────────────────────────

    #[test]
    fn test_max_severity() {
        let mut pipeline = Pipeline::new(make_policy(vec![
            rule_coolant_slope(0.5, Severity::Critical, 0),
            rule_coolant_temp(100.0, Severity::High),
        ]));

        for i in 0..10_i64 {
            pipeline.process(coolant_event(i * 1_000, 90.0 + i as f64));
        }
        let result = pipeline.process(coolant_event(10_000, 105.0));

        assert_eq!(result.max_severity(), Some(&Severity::Critical));
    }

    // ── Test 7: from_yaml constructs and runs correctly ───────────────────────

    #[test]
    fn test_from_yaml_pipeline() {
        // cooldown_ms=0 so TrustEngine never suppresses.
        let yaml = r#"
version: "test-1.0"
vehicle_class: SIMULATOR
rules:
  - id: brake_rule
    group: braking
    signal: brake_spike_count
    operator: gte
    threshold: 3.0
    severity: WARN
    cooldown_ms: 0
    hysteresis: 0.0
    description: "harsh brake"
"#;
        let mut pipeline = Pipeline::from_yaml(yaml).expect("yaml must parse");

        // 5 events with 2 spikes — below threshold, no fire yet.
        // [0.1, 0.9, 0.1, 0.9, 0.1] → rising edges at (0→1) and (2→3) = count 2.
        let pre: &[f64] = &[0.1, 0.9, 0.1, 0.9, 0.1];
        for (i, &v) in pre.iter().enumerate() {
            pipeline.process(make_event(
                "SIM-001", i as i64 * 1_000,
                SignalMap { brake_pedal: Some(v), ..Default::default() },
            ));
        }
        // 6th event: value 0.9 → 3rd spike. Window has 3 spikes ≥ 3 → fires.
        // First time threshold is crossed, so TrustEngine passes it through.
        let result = pipeline.process(make_event(
            "SIM-001", 5_000,
            SignalMap { brake_pedal: Some(0.9), ..Default::default() },
        ));

        assert!(result.has_alerts(), "3 brake spikes must trigger alert");
        assert_eq!(result.gated_decisions[0].rule_id, "brake_rule");
    }

    // ── Test 8: process_batch collects results ────────────────────────────────

    #[test]
    fn test_process_batch_collects_results() {
        let mut pipeline = Pipeline::new(make_policy(vec![]));

        let events: Vec<SignalEvent> = (0..5_i64)
            .map(|i| coolant_event(i * 1_000, 80.0 + i as f64))
            .collect();

        let results = pipeline.process_batch(events);
        assert_eq!(results.len(), 5, "batch must return one result per event");
    }

    // ── Test 9: independent assets tracked separately ─────────────────────────

    #[test]
    fn test_independent_assets_tracked_separately() {
        let mut pipeline = Pipeline::new(make_policy(vec![
            rule_coolant_temp(100.0, Severity::High),
        ]));

        // TRUCK-001 at 105°C — should alert
        let r1 = pipeline.process(make_event(
            "TRUCK-001", 0,
            SignalMap { coolant_temp: Some(105.0), ..Default::default() },
        ));
        // TRUCK-002 at 85°C — should not alert
        let r2 = pipeline.process(make_event(
            "TRUCK-002", 0,
            SignalMap { coolant_temp: Some(85.0), ..Default::default() },
        ));

        assert!(r1.has_alerts(),  "TRUCK-001 at 105°C must alert");
        assert!(!r2.has_alerts(), "TRUCK-002 at 85°C must not alert");
    }
}
