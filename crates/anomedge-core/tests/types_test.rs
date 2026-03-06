// crates/anomedge-core/tests/types_test.rs
use anomedge_core::types::*;
use serde_json;

#[test]
fn test_signal_event_serialize_deserialize() {
    let mut signals = SignalMap::default();
    signals.coolant_temp = Some(92.5);
    signals.engine_rpm = Some(2400.0);
    signals.vehicle_speed = Some(88.0);
    signals.dtc_codes = Some(vec!["P0300".to_string()]);

    let event = SignalEvent {
        ts: 1_709_000_000_000,
        asset_id: "TRUCK-001".to_string(),
        driver_id: "DRV-042".to_string(),
        source: SignalSource::Obd2Generic,
        signals,
        raw_frame: None,
    };

    let json = serde_json::to_string(&event).expect("serialize failed");
    let restored: SignalEvent = serde_json::from_str(&json).expect("deserialize failed");

    assert_eq!(event.ts, restored.ts);
    assert_eq!(event.asset_id, restored.asset_id);
    assert_eq!(event.driver_id, restored.driver_id);
    assert_eq!(event.signals.coolant_temp, restored.signals.coolant_temp);
    assert_eq!(event.signals.engine_rpm, restored.signals.engine_rpm);
    assert_eq!(event.signals.dtc_codes, restored.signals.dtc_codes);
}

#[test]
fn test_severity_ordering() {
    assert!(Severity::Critical > Severity::High);
    assert!(Severity::High > Severity::Warn);
    assert!(Severity::Warn > Severity::Watch);
    assert!(Severity::Watch > Severity::Normal);
    assert!(Severity::Normal < Severity::Critical);
}

#[test]
fn test_severity_serializes_as_screaming_snake_case() {
    let json = serde_json::to_string(&Severity::Critical).unwrap();
    assert_eq!(json, "\"CRITICAL\"");

    let json = serde_json::to_string(&Severity::Normal).unwrap();
    assert_eq!(json, "\"NORMAL\"");
}

#[test]
fn test_signal_source_serializes_correctly() {
    assert_eq!(
        serde_json::to_string(&SignalSource::Obd2Generic).unwrap(),
        "\"OBD2_GENERIC\""
    );
    assert_eq!(
        serde_json::to_string(&SignalSource::JohnDeere139).unwrap(),
        "\"JOHN_DEERE_139\""
    );
    assert_eq!(
        serde_json::to_string(&SignalSource::FordF450).unwrap(),
        "\"FORD_F450\""
    );
}

#[test]
fn test_decision_source_serializes_correctly() {
    assert_eq!(
        serde_json::to_string(&DecisionSource::EdgeAi).unwrap(),
        "\"EDGE_AI\""
    );
    assert_eq!(
        serde_json::to_string(&DecisionSource::MlStatistical).unwrap(),
        "\"ML_STATISTICAL\""
    );
    assert_eq!(
        serde_json::to_string(&DecisionSource::RuleEngine).unwrap(),
        "\"RULE_ENGINE\""
    );
}

#[test]
fn test_bus_topic_serializes_with_dots() {
    assert_eq!(
        serde_json::to_string(&BusTopic::SignalsRaw).unwrap(),
        "\"signals.raw\""
    );
    assert_eq!(
        serde_json::to_string(&BusTopic::DecisionsGated).unwrap(),
        "\"decisions.gated\""
    );
    assert_eq!(
        serde_json::to_string(&BusTopic::SystemHeartbeat).unwrap(),
        "\"system.heartbeat\""
    );
}

#[test]
fn test_event_envelope_roundtrip() {
    let mut signals = SignalMap::default();
    signals.engine_rpm = Some(1800.0);

    let event = SignalEvent {
        ts: 1_709_000_000_000,
        asset_id: "TRUCK-001".to_string(),
        driver_id: "DRV-001".to_string(),
        source: SignalSource::Simulator,
        signals,
        raw_frame: None,
    };

    let envelope = EventEnvelope {
        id: "uuid-test-1234".to_string(),
        topic: BusTopic::SignalsRaw,
        seq: 1,
        ts: 1_709_000_000_000,
        payload: event,
    };

    let json = serde_json::to_string(&envelope).unwrap();
    let restored: EventEnvelope<SignalEvent> = serde_json::from_str(&json).unwrap();

    assert_eq!(restored.topic, BusTopic::SignalsRaw);
    assert_eq!(restored.seq, 1);
    assert_eq!(restored.payload.asset_id, "TRUCK-001");
    assert_eq!(restored.payload.signals.engine_rpm, Some(1800.0));
}

#[test]
fn test_policy_pack_roundtrip() {
    let rule = PolicyRule {
        id: "coolant_overheat_critical".to_string(),
        group: RuleGroup::Thermal,
        signal: "coolant_temp".to_string(),
        operator: RuleOperator::Gt,
        threshold: 120.0,
        severity: Severity::Critical,
        cooldown_ms: 30_000,
        hysteresis: 5.0,
        description: "Engine coolant critically overheated".to_string(),
    };

    let pack = PolicyPack {
        version: "1.0.0".to_string(),
        vehicle_class: VehicleClass::FleetDiesel,
        rules: vec![rule],
    };

    let json = serde_json::to_string(&pack).unwrap();
    let restored: PolicyPack = serde_json::from_str(&json).unwrap();

    assert_eq!(restored.version, "1.0.0");
    assert_eq!(restored.rules.len(), 1);
    assert_eq!(restored.rules[0].threshold, 120.0);
    assert_eq!(restored.rules[0].severity, Severity::Critical);
}
