//! AnomEdge Core — CLI demo runner
//!
//! Usage:
//!   cargo run -- --scenario ../../scenarios/overheat_highway.json
//!   cargo run -- --scenario ../../scenarios/harsh_brake_city.json --policy ../../policy/policy.yaml

use std::fs;
use std::path::PathBuf;
use anomedge_core::pipeline::Pipeline;
use anomedge_core::types::{SignalEvent, SignalMap, SignalSource};
use serde::Deserialize;

// ─── Scenario JSON shape ───────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct Scenario {
    name:                  String,
    asset_id:              String,
    expected_alerts:       Vec<String>,
    expected_max_severity: Option<String>,
    frames:                Vec<Frame>,
}

#[derive(Debug, Deserialize)]
struct Frame {
    ts_offset_ms: i64,
    signals:      serde_json::Value,
}

// ─── Main ─────────────────────────────────────────────────────────────────────

fn main() {
    let args: Vec<String> = std::env::args().collect();

    let scenario_path = find_arg(&args, "--scenario")
        .unwrap_or_else(|| "scenarios/overheat_highway.json".to_string());

    let policy_path = find_arg(&args, "--policy")
        .unwrap_or_else(|| "policy/policy.yaml".to_string());

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = manifest_dir.join("../../");

    let scenario_file = root.join(&scenario_path);
    let policy_file   = root.join(&policy_path);

    let scenario_json = fs::read_to_string(&scenario_file)
        .unwrap_or_else(|e| panic!("Cannot read scenario {}: {e}", scenario_file.display()));

    let policy_yaml = fs::read_to_string(&policy_file)
        .unwrap_or_else(|e| panic!("Cannot read policy {}: {e}", policy_file.display()));

    let scenario: Scenario = serde_json::from_str(&scenario_json)
        .unwrap_or_else(|e| panic!("Bad scenario JSON: {e}"));

    let mut pipeline = Pipeline::from_yaml(&policy_yaml)
        .unwrap_or_else(|e| panic!("Bad policy YAML: {e}"));

    eprintln!("\n=== AnomEdge Demo: {} ===\n", scenario.name);
    eprintln!("Policy:   {}", policy_path);
    eprintln!("Scenario: {}", scenario_path);
    eprintln!("Frames:   {}\n", scenario.frames.len());

    let base_ts: i64 = 1_700_000_000_000;
    let mut all_rule_ids: Vec<String> = Vec::new();
    let mut max_severity_rank: i32 = -1;
    let mut max_severity = String::new();

    for frame in &scenario.frames {
        let s: std::collections::HashMap<String, serde_json::Value> =
            serde_json::from_value(frame.signals.clone()).unwrap_or_default();

        let signals = SignalMap {
            coolant_temp:       get_f64(&s, "coolant_temp"),
            engine_rpm:         get_f64(&s, "engine_rpm"),
            vehicle_speed:      get_f64(&s, "vehicle_speed"),
            throttle_position:  get_f64(&s, "throttle_position"),
            engine_load:        get_f64(&s, "engine_load"),
            fuel_level:         get_f64(&s, "fuel_level"),
            intake_air_temp:    get_f64(&s, "intake_air_temp"),
            battery_voltage:    get_f64(&s, "battery_voltage"),
            brake_pedal:        get_f64(&s, "brake_pressure").map(|v| v / 100.0),
            oil_pressure:       get_f64(&s, "oil_pressure"),
            dtc_codes:          None,
            hydraulic_pressure: get_f64(&s, "hydraulic_pressure"),
            transmission_temp:  get_f64(&s, "transmission_temp"),
            axle_weight:        None,
            pto_rpm:            None,
            boom_position:      None,
            load_weight:        None,
            def_level:          None,
            adblue_level:       None,
            boost_pressure:     None,
            exhaust_temp:       None,
            extra:              s.clone(),
        };

        let event = SignalEvent {
            ts:        base_ts + frame.ts_offset_ms,
            asset_id:  scenario.asset_id.clone(),
            driver_id: "DRV-SIM".to_string(),
            source:    SignalSource::Simulator,
            signals,
            raw_frame: None,
        };

        let result = pipeline.process(event);

        for d in &result.gated_decisions {
            let sev_str = format!("{:?}", d.severity);
            eprintln!(
                "  [ts+{:>5}ms] ALERT  rule={:<30} severity={:<8} raw={:.2}",
                frame.ts_offset_ms,
                d.rule_id,
                sev_str,
                d.raw_value,
            );
            if !all_rule_ids.contains(&d.rule_id) {
                all_rule_ids.push(d.rule_id.clone());
            }
            let rank = severity_rank(&sev_str);
            if rank > max_severity_rank {
                max_severity_rank = rank;
                max_severity = sev_str;
            }
        }
    }

    eprintln!("\n--- Results ---");
    eprintln!("Fired rules:   {:?}", all_rule_ids);
    eprintln!("Max severity:  {}", if max_severity.is_empty() { "(none)".to_string() } else { max_severity.clone() });

    eprintln!("\n--- Expectations ---");
    eprintln!("Expected rules: {:?}", scenario.expected_alerts);
    eprintln!("Expected max:   {:?}", scenario.expected_max_severity);

    let mut passed = true;
    for expected in &scenario.expected_alerts {
        if !all_rule_ids.contains(expected) {
            eprintln!("FAIL: expected rule '{}' did not fire", expected);
            passed = false;
        }
    }
    if let Some(exp_sev) = &scenario.expected_max_severity {
        if &max_severity != exp_sev {
            eprintln!("FAIL: expected max severity '{}', got '{}'", exp_sev, max_severity);
            passed = false;
        }
    }

    if passed {
        eprintln!("\nPASS — all expectations met");
        std::process::exit(0);
    } else {
        eprintln!("\nFAIL — see above");
        std::process::exit(1);
    }
}

fn find_arg(args: &[String], flag: &str) -> Option<String> {
    args.windows(2)
        .find(|w| w[0] == flag)
        .map(|w| w[1].clone())
}

fn get_f64(map: &std::collections::HashMap<String, serde_json::Value>, key: &str) -> Option<f64> {
    map.get(key).and_then(|v| v.as_f64())
}

fn severity_rank(sev: &str) -> i32 {
    match sev {
        "Watch"    | "WATCH"    => 0,
        "Low"      | "LOW"      => 1,
        "Warn"     | "WARN"     => 2,
        "High"     | "HIGH"     => 3,
        "Critical" | "CRITICAL" => 4,
        _ => -1,
    }
}
