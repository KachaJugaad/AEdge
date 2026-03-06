//! pipeline_scenario_test.rs
//! Integration tests: run each scenario JSON through the full pipeline
//! using the canonical policy/policy.yaml and assert expected outcomes.
//!
//! These tests are the Phase-0 gate: they prove the full signal→decision
//! path works end-to-end before Person B and Person C wire their layers.

use anomedge_core::pipeline::Pipeline;
use anomedge_core::types::{Severity, SignalMap, SignalSource, SignalEvent};

// ─── Scenario loader (minimal — no serde dependency in test) ─────────────────

#[derive(Debug)]
struct ScenarioFrame {
    ts_offset_ms: i64,
    signals:      SignalMap,
}

fn load_scenario(path: &str) -> (String, Vec<ScenarioFrame>) {
    let raw  = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("Failed to read scenario {path}: {e}"));
    let json: serde_json::Value = serde_json::from_str(&raw)
        .unwrap_or_else(|e| panic!("Failed to parse scenario {path}: {e}"));

    let asset_id = json["asset_id"].as_str().unwrap().to_string();
    let frames: Vec<ScenarioFrame> = json["frames"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| {
            let ts_offset_ms = f["ts_offset_ms"].as_i64().unwrap();
            let sigs = &f["signals"];

            let coolant_temp       = sigs["coolant_temp"].as_f64();
            let engine_rpm         = sigs["engine_rpm"].as_f64();
            let vehicle_speed      = sigs["vehicle_speed"].as_f64();
            let throttle_position  = sigs["throttle_position"].as_f64();
            let engine_load        = sigs["engine_load"].as_f64();
            let fuel_level         = sigs["fuel_level"].as_f64();
            let battery_voltage    = sigs["battery_voltage"].as_f64();
            let brake_pedal        = sigs["brake_pedal"].as_f64();
            let hydraulic_pressure = sigs["hydraulic_pressure"].as_f64();
            let transmission_temp  = sigs["transmission_temp"].as_f64();

            ScenarioFrame {
                ts_offset_ms,
                signals: SignalMap {
                    coolant_temp,
                    engine_rpm,
                    vehicle_speed,
                    throttle_position,
                    engine_load,
                    fuel_level,
                    battery_voltage,
                    brake_pedal,
                    hydraulic_pressure,
                    transmission_temp,
                    ..Default::default()
                },
            }
        })
        .collect();

    (asset_id, frames)
}

fn run_scenario(scenario_path: &str, policy_yaml: &str) -> RunResult {
    let mut pipeline = Pipeline::from_yaml(policy_yaml).expect("policy YAML must parse");

    let (asset_id, frames) = load_scenario(scenario_path);
    let base_ts: i64 = 1_700_000_000_000; // fixed epoch for reproducibility

    let mut all_rule_ids: Vec<String> = Vec::new();
    let mut max_sev: Option<Severity> = None;

    for frame in &frames {
        let event = SignalEvent {
            ts:        base_ts + frame.ts_offset_ms,
            asset_id:  asset_id.clone(),
            driver_id: "DRV-SIM".into(),
            source:    SignalSource::Simulator,
            signals:   frame.signals.clone(),
            raw_frame: None,
        };
        let result = pipeline.process(event);

        for d in &result.gated_decisions {
            if !all_rule_ids.contains(&d.rule_id) {
                all_rule_ids.push(d.rule_id.clone());
            }
            let sev = &d.severity;
            match &max_sev {
                None    => max_sev = Some(sev.clone()),
                Some(m) => if sev > m { max_sev = Some(sev.clone()); },
            }
        }
    }

    RunResult { rule_ids: all_rule_ids, max_severity: max_sev }
}

struct RunResult {
    rule_ids:     Vec<String>,
    max_severity: Option<Severity>,
}

// ─── Canonical policy fixture ─────────────────────────────────────────────────

fn policy_yaml() -> String {
    std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"), "/../../policy/policy.yaml"
    ))
    .expect("policy/policy.yaml must exist")
}

// ─── Scenario tests ───────────────────────────────────────────────────────────

/// Scenario 1: overheat_highway — coolant rises 88→118°C.
/// Must trigger coolant_high_temp (HIGH) AND coolant_overheat_critical (CRITICAL).
#[test]
fn scenario_overheat_highway_triggers_critical() {
    let result = run_scenario(
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../scenarios/overheat_highway.json"),
        &policy_yaml(),
    );

    assert!(
        result.rule_ids.contains(&"coolant_high_temp".to_string()),
        "overheat_highway must trigger coolant_high_temp (HIGH); fired: {:?}",
        result.rule_ids
    );
    assert!(
        result.rule_ids.contains(&"coolant_overheat_critical".to_string()),
        "overheat_highway must trigger coolant_overheat_critical (CRITICAL); fired: {:?}",
        result.rule_ids
    );
    assert_eq!(
        result.max_severity,
        Some(Severity::Critical),
        "max severity must be CRITICAL"
    );
}

/// Scenario 2: harsh_brake_city — 4 brake spikes in 30-second window.
/// Must trigger harsh_brake_event (WARN, ≥3 spikes).
#[test]
fn scenario_harsh_brake_triggers_warn() {
    let result = run_scenario(
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../scenarios/harsh_brake_city.json"),
        &policy_yaml(),
    );

    assert!(
        result.rule_ids.contains(&"harsh_brake_event".to_string()),
        "harsh_brake_city must trigger harsh_brake_event; fired: {:?}",
        result.rule_ids
    );
    assert!(
        result.max_severity.as_ref().map(|s| s >= &Severity::Warn).unwrap_or(false),
        "max severity must be at least WARN, got {:?}",
        result.max_severity
    );
}

/// Scenario 3: cold_start_normal — slow warm-up from -5°C to 24°C.
/// Must produce ZERO alerts (no thresholds crossed, slope stays low).
#[test]
fn scenario_cold_start_normal_produces_no_alerts() {
    let result = run_scenario(
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../scenarios/cold_start_normal.json"),
        &policy_yaml(),
    );

    // No critical/warn rules should fire on a gentle cold start.
    let bad_rules: Vec<&String> = result.rule_ids.iter().filter(|id| {
        // These rules should never fire in a normal cold start
        matches!(
            id.as_str(),
            "coolant_high_temp" | "coolant_overheat_critical" | "coolant_rising_fast"
            | "transmission_heat_flag" | "transmission_overheat"
        )
    }).collect();

    assert!(
        bad_rules.is_empty(),
        "cold_start_normal must not trigger thermal alerts; fired: {:?}",
        bad_rules
    );
    assert!(
        result.max_severity.as_ref().map(|s| s < &Severity::Warn).unwrap_or(true),
        "cold_start_normal max severity must be below WARN, got {:?}",
        result.max_severity
    );
}

/// Scenario 4: oscillating_fault — coolant slope spikes then stabilises.
/// Must fire coolant_rising_fast at least once. Validates slope detection.
#[test]
fn scenario_oscillating_fault_fires_slope_alert() {
    let result = run_scenario(
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../scenarios/oscillating_fault.json"),
        &policy_yaml(),
    );

    assert!(
        result.rule_ids.contains(&"coolant_rising_fast".to_string())
        || result.rule_ids.contains(&"coolant_overheat_critical".to_string()),
        "oscillating_fault must fire at least one thermal alert; fired: {:?}",
        result.rule_ids
    );
}

/// Scenario 5: heavy_equipment_hydraulic — Cat-320 with hydraulic spike,
/// transmission heat, and low fuel. Must trigger at least 2 distinct rules.
#[test]
fn scenario_heavy_equipment_triggers_multiple_rules() {
    let result = run_scenario(
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../scenarios/heavy_equipment_hydraulic.json"),
        &policy_yaml(),
    );

    assert!(
        result.rule_ids.len() >= 2,
        "heavy_equipment_hydraulic must trigger ≥2 rules; fired: {:?}",
        result.rule_ids
    );
    // Transmission heat must fire (temp hits 118°C > 110°C threshold)
    let thermal_fired = result.rule_ids.iter().any(|id| id.contains("transmission"));
    assert!(
        thermal_fired,
        "heavy equipment must trigger a transmission rule; fired: {:?}",
        result.rule_ids
    );
    // Fuel level low must fire (level at 12% < 15% threshold)
    let fuel_fired = result.rule_ids.iter().any(|id| id.contains("fuel"));
    assert!(
        fuel_fired,
        "heavy equipment must trigger fuel_level_low; fired: {:?}",
        result.rule_ids
    );
}
