//! rules.rs
//! RuleEngine — evaluates a `FeatureWindow` against policy thresholds.
//!
//! Policy thresholds live in `policy/policy.yaml` and are loaded at startup via serde_yaml.
//! No numbers are hardcoded here — all thresholds come from `PolicyPack`.
//!
//! Performance target: evaluate() < 1ms per frame (O(r) where r ≈ 20 rules).

use crate::types::{
    Decision, DecisionSource, FeatureWindow, PolicyPack, PolicyRule, RuleOperator, SignalMap,
};

// ─── RuleEngine ───────────────────────────────────────────────────────────────

/// Evaluates a `FeatureWindow` against a loaded `PolicyPack`.
///
/// This engine is Tier 3 of the InferenceChain — it ALWAYS fires and never
/// falls through. Every call to `evaluate()` returns a (possibly empty) list
/// of triggered `Decision` objects.
pub struct RuleEngine {
    policy: PolicyPack,
}

impl RuleEngine {
    pub fn new(policy: PolicyPack) -> Self {
        Self { policy }
    }

    /// Deserialise a `PolicyPack` from a YAML string and build a `RuleEngine`.
    /// Convenience constructor for production use.
    pub fn from_yaml(yaml: &str) -> Result<Self, serde_yaml::Error> {
        let policy: PolicyPack = serde_yaml::from_str(yaml)?;
        Ok(Self::new(policy))
    }

    /// Evaluate all rules against the window.
    ///
    /// Returns every rule that fired (0..n). Multiple rules may fire on the
    /// same window. The caller (TrustEngine) applies cooldown and hysteresis.
    ///
    /// Complexity: O(r) where r = rules count.
    pub fn evaluate(&self, window: &FeatureWindow) -> Vec<Decision> {
        self.policy.rules.iter().filter_map(|rule| {
            let raw_value = resolve_signal(window, &rule.signal)?;

            if check_operator(raw_value, rule.operator.clone(), rule.threshold) {
                Some(build_decision(window, rule, raw_value))
            } else {
                None
            }
        }).collect()
    }

    pub fn policy(&self) -> &PolicyPack {
        &self.policy
    }
}

// ─── Signal resolution ────────────────────────────────────────────────────────

/// Resolve a `signal` path to a numeric value from the `FeatureWindow`.
///
/// Supported paths:
/// - Top-level fields: `"coolant_slope"`, `"brake_spike_count"`, etc.
/// - Nested snapshot:  `"signals_snapshot.coolant_temp"`, etc.
/// - Bool fields return `1.0` (true) or `0.0` (false).
/// - Derived:          `"dtc_new_count"` = `dtc_new.len() as f64`.
///
/// Returns `None` for unrecognised paths or absent optional signals.
fn resolve_signal(window: &FeatureWindow, signal: &str) -> Option<f64> {
    match signal {
        "coolant_slope"      => Some(window.coolant_slope),
        "brake_spike_count"  => Some(window.brake_spike_count),
        "speed_mean"         => Some(window.speed_mean),
        "rpm_mean"           => Some(window.rpm_mean),
        "engine_load_mean"   => Some(window.engine_load_mean),
        "throttle_variance"  => Some(window.throttle_variance),
        "hydraulic_spike"    => Some(if window.hydraulic_spike { 1.0 } else { 0.0 }),
        "transmission_heat"  => Some(if window.transmission_heat { 1.0 } else { 0.0 }),
        "dtc_new_count"      => Some(window.dtc_new.len() as f64),
        s if s.starts_with("signals_snapshot.") => {
            let field = &s["signals_snapshot.".len()..];
            resolve_snapshot_field(&window.signals_snapshot, field)
        }
        _ => None,
    }
}

/// Resolve a named field from `SignalMap` to `f64`.
/// Returns `None` if the field is not set in the snapshot.
fn resolve_snapshot_field(snap: &SignalMap, field: &str) -> Option<f64> {
    match field {
        "coolant_temp"       => snap.coolant_temp,
        "engine_rpm"         => snap.engine_rpm,
        "vehicle_speed"      => snap.vehicle_speed,
        "throttle_position"  => snap.throttle_position,
        "engine_load"        => snap.engine_load,
        "fuel_level"         => snap.fuel_level,
        "intake_air_temp"    => snap.intake_air_temp,
        "battery_voltage"    => snap.battery_voltage,
        "brake_pedal"        => snap.brake_pedal,
        "oil_pressure"       => snap.oil_pressure,
        "hydraulic_pressure" => snap.hydraulic_pressure,
        "transmission_temp"  => snap.transmission_temp,
        "axle_weight"        => snap.axle_weight,
        "pto_rpm"            => snap.pto_rpm,
        "boom_position"      => snap.boom_position,
        "load_weight"        => snap.load_weight,
        "def_level"          => snap.def_level,
        "adblue_level"       => snap.adblue_level,
        "boost_pressure"     => snap.boost_pressure,
        "exhaust_temp"       => snap.exhaust_temp,
        _ => None,
    }
}

// ─── Operator evaluation ─────────────────────────────────────────────────────

fn check_operator(value: f64, op: RuleOperator, threshold: f64) -> bool {
    match op {
        RuleOperator::Gt       => value > threshold,
        RuleOperator::Lt       => value < threshold,
        RuleOperator::Gte      => value >= threshold,
        RuleOperator::Lte      => value <= threshold,
        RuleOperator::Eq       => (value - threshold).abs() < f64::EPSILON,
        RuleOperator::Contains => value >= threshold, // numeric "contains" → gte
    }
}

// ─── Decision builder ─────────────────────────────────────────────────────────

fn build_decision(window: &FeatureWindow, rule: &PolicyRule, raw_value: f64) -> Decision {
    Decision {
        ts:              window.ts,
        asset_id:        window.asset_id.clone(),
        severity:        rule.severity.clone(),
        rule_id:         rule.id.clone(),
        rule_group:      rule.group.clone(),
        confidence:      1.0,                              // rule engine is deterministic
        triggered_by:    vec![rule.signal.clone()],
        raw_value,
        threshold:       rule.threshold,
        decision_source: DecisionSource::RuleEngine,
        context:         Some(window.clone()),
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{
        PolicyRule, RuleGroup, RuleOperator, Severity, SignalMap, VehicleClass,
    };

    // ── Helpers ───────────────────────────────────────────────────────────────

    fn make_engine(rules: Vec<PolicyRule>) -> RuleEngine {
        RuleEngine::new(PolicyPack {
            version:       "test-1.0".into(),
            vehicle_class: VehicleClass::Simulator,
            rules,
        })
    }

    fn rule(
        id: &str,
        group: RuleGroup,
        signal: &str,
        op: RuleOperator,
        threshold: f64,
        severity: Severity,
    ) -> PolicyRule {
        PolicyRule {
            id:          id.into(),
            group,
            signal:      signal.into(),
            operator:    op,
            threshold,
            severity,
            cooldown_ms: 30_000,
            hysteresis:  0.0,
            description: id.into(),
        }
    }

    fn empty_window() -> FeatureWindow {
        FeatureWindow {
            ts:                1_700_000_000_000,
            asset_id:          "SIM-001".into(),
            window_seconds:    30.0,
            coolant_slope:     0.0,
            brake_spike_count: 0.0,
            speed_mean:        0.0,
            rpm_mean:          0.0,
            engine_load_mean:  0.0,
            throttle_variance: 0.0,
            hydraulic_spike:   false,
            transmission_heat: false,
            dtc_new:           vec![],
            signals_snapshot:  SignalMap::default(),
        }
    }

    fn window_with_slope(slope: f64) -> FeatureWindow {
        FeatureWindow { coolant_slope: slope, ..empty_window() }
    }

    fn window_with_brake(count: f64) -> FeatureWindow {
        FeatureWindow { brake_spike_count: count, ..empty_window() }
    }

    fn window_with_coolant(temp: f64) -> FeatureWindow {
        let mut w = empty_window();
        w.signals_snapshot.coolant_temp = Some(temp);
        w
    }

    // ── Test 1: coolant_slope > 0.8 fires CRITICAL ────────────────────────────

    #[test]
    fn test_coolant_slope_above_threshold_fires() {
        let engine = make_engine(vec![
            rule("coolant_rising_fast", RuleGroup::Thermal,
                 "coolant_slope", RuleOperator::Gt, 0.8, Severity::Critical),
        ]);

        let decisions = engine.evaluate(&window_with_slope(1.0));

        assert_eq!(decisions.len(), 1);
        let d = &decisions[0];
        assert_eq!(d.rule_id, "coolant_rising_fast");
        assert_eq!(d.severity, Severity::Critical);
        assert!((d.raw_value - 1.0).abs() < 0.001);
        assert_eq!(d.decision_source, DecisionSource::RuleEngine);
    }

    // ── Test 2: brake_spike_count = 2 below threshold 3 does NOT fire ─────────

    #[test]
    fn test_brake_below_threshold_does_not_fire() {
        let engine = make_engine(vec![
            rule("harsh_brake", RuleGroup::Braking,
                 "brake_spike_count", RuleOperator::Gte, 3.0, Severity::Warn),
        ]);

        let decisions = engine.evaluate(&window_with_brake(2.0));
        assert!(decisions.is_empty(), "count 2 must not fire at threshold 3");
    }

    // ── Test 3: brake_spike_count = 4 at threshold 3 fires WARN ──────────────

    #[test]
    fn test_brake_above_threshold_fires_warn() {
        let engine = make_engine(vec![
            rule("harsh_brake", RuleGroup::Braking,
                 "brake_spike_count", RuleOperator::Gte, 3.0, Severity::Warn),
        ]);

        let decisions = engine.evaluate(&window_with_brake(4.0));
        assert_eq!(decisions.len(), 1);
        assert_eq!(decisions[0].severity, Severity::Warn);
    }

    // ── Test 4: nested signal_snapshot.coolant_temp = 110 fires at 108 ────────

    #[test]
    fn test_nested_snapshot_signal_fires() {
        let engine = make_engine(vec![
            rule("coolant_critical", RuleGroup::Thermal,
                 "signals_snapshot.coolant_temp", RuleOperator::Gte, 108.0, Severity::Critical),
        ]);

        let decisions = engine.evaluate(&window_with_coolant(110.0));
        assert_eq!(decisions.len(), 1);
        assert_eq!(decisions[0].rule_id, "coolant_critical");
        assert!((decisions[0].raw_value - 110.0).abs() < 0.001);
    }

    #[test]
    fn test_nested_snapshot_signal_below_threshold_does_not_fire() {
        let engine = make_engine(vec![
            rule("coolant_critical", RuleGroup::Thermal,
                 "signals_snapshot.coolant_temp", RuleOperator::Gte, 108.0, Severity::Critical),
        ]);

        let decisions = engine.evaluate(&window_with_coolant(105.0));
        assert!(decisions.is_empty());
    }

    // ── Test 5: window with all-zero features returns no decisions ────────────

    #[test]
    fn test_zero_window_no_decisions_for_high_thresholds() {
        let engine = make_engine(vec![
            rule("coolant_rising_fast", RuleGroup::Thermal,
                 "coolant_slope", RuleOperator::Gt, 0.8, Severity::Critical),
            rule("harsh_brake", RuleGroup::Braking,
                 "brake_spike_count", RuleOperator::Gte, 3.0, Severity::Warn),
        ]);

        let decisions = engine.evaluate(&empty_window());
        assert!(decisions.is_empty(), "all-zero window must not trigger high-threshold rules");
    }

    // ── Test 6: multiple rules fire simultaneously ────────────────────────────

    #[test]
    fn test_multiple_rules_fire_simultaneously() {
        let engine = make_engine(vec![
            rule("coolant_rising_fast", RuleGroup::Thermal,
                 "coolant_slope", RuleOperator::Gt, 0.8, Severity::Critical),
            rule("harsh_brake", RuleGroup::Braking,
                 "brake_spike_count", RuleOperator::Gte, 3.0, Severity::Warn),
            rule("coolant_high", RuleGroup::Thermal,
                 "signals_snapshot.coolant_temp", RuleOperator::Gt, 100.0, Severity::High),
        ]);

        let mut window = window_with_slope(1.5);
        window.brake_spike_count = 4.0;
        window.signals_snapshot.coolant_temp = Some(112.0);

        let decisions = engine.evaluate(&window);

        assert_eq!(decisions.len(), 3, "all three rules must fire");
        let rule_ids: Vec<&str> = decisions.iter().map(|d| d.rule_id.as_str()).collect();
        assert!(rule_ids.contains(&"coolant_rising_fast"));
        assert!(rule_ids.contains(&"harsh_brake"));
        assert!(rule_ids.contains(&"coolant_high"));
    }

    // ── Test 7: unknown signal path returns no decision ───────────────────────

    #[test]
    fn test_unknown_signal_path_skipped_no_panic() {
        let engine = make_engine(vec![
            rule("bad_signal", RuleGroup::Composite,
                 "does_not_exist.at_all", RuleOperator::Gt, 0.0, Severity::Watch),
        ]);

        let decisions = engine.evaluate(&empty_window());
        assert!(decisions.is_empty(), "unknown signal must be silently skipped");
    }

    // ── Test 8: lt operator fires when value below threshold ──────────────────

    #[test]
    fn test_lt_operator_fires_correctly() {
        let engine = make_engine(vec![
            rule("low_battery", RuleGroup::Electrical,
                 "signals_snapshot.battery_voltage", RuleOperator::Lt, 11.5, Severity::Warn),
        ]);

        let mut window = empty_window();
        window.signals_snapshot.battery_voltage = Some(10.8);

        let decisions = engine.evaluate(&window);
        assert_eq!(decisions.len(), 1);
        assert_eq!(decisions[0].rule_id, "low_battery");
    }

    // ── Test 9: hydraulic_spike bool signal fires via gte threshold 1.0 ───────

    #[test]
    fn test_hydraulic_spike_bool_signal_fires() {
        let engine = make_engine(vec![
            rule("hydraulic_spike", RuleGroup::Hydraulic,
                 "hydraulic_spike", RuleOperator::Gte, 1.0, Severity::High),
        ]);

        let window = FeatureWindow { hydraulic_spike: true, ..empty_window() };
        let decisions = engine.evaluate(&window);
        assert_eq!(decisions.len(), 1);

        let window_false = FeatureWindow { hydraulic_spike: false, ..empty_window() };
        assert!(engine.evaluate(&window_false).is_empty());
    }

    // ── Test 10: dtc_new_count signal fires when new codes present ────────────

    #[test]
    fn test_dtc_new_count_signal_fires() {
        let engine = make_engine(vec![
            rule("dtc_new_codes", RuleGroup::Dtc,
                 "dtc_new_count", RuleOperator::Gt, 0.0, Severity::Warn),
        ]);

        let window_with_dtc = FeatureWindow {
            dtc_new: vec!["P0300".into()],
            ..empty_window()
        };
        let decisions = engine.evaluate(&window_with_dtc);
        assert_eq!(decisions.len(), 1);
        assert!((decisions[0].raw_value - 1.0).abs() < 0.001);

        let decisions_empty = engine.evaluate(&empty_window());
        assert!(decisions_empty.is_empty());
    }

    // ── Test 11: from_yaml loads policy correctly ─────────────────────────────

    #[test]
    fn test_from_yaml_loads_and_evaluates() {
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
    description: "Test rule"
"#;
        let engine = RuleEngine::from_yaml(yaml).expect("YAML must parse");
        let decisions = engine.evaluate(&window_with_slope(1.0));
        assert_eq!(decisions.len(), 1);
        assert_eq!(decisions[0].rule_id, "coolant_rising_fast");
    }

    // ── Test 12: confidence is always 1.0 for rule engine ────────────────────

    #[test]
    fn test_decision_confidence_is_one() {
        let engine = make_engine(vec![
            rule("test_rule", RuleGroup::Thermal,
                 "coolant_slope", RuleOperator::Gt, 0.0, Severity::Watch),
        ]);

        let decisions = engine.evaluate(&window_with_slope(0.5));
        assert_eq!(decisions.len(), 1);
        assert!((decisions[0].confidence - 1.0).abs() < f64::EPSILON);
    }
}
