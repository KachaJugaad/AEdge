// crates/anomedge-core/src/types.rs
// Rust mirror of packages/contracts/src/index.ts — FROZEN after Day 1 merge
// Every type here must stay in sync with the TypeScript contracts.

use std::collections::HashMap;
use serde::{Deserialize, Serialize};

// ─── Severity ────────────────────────────────────────────────────────────────

/// Alert severity level. Ordering: NORMAL < WATCH < WARN < HIGH < CRITICAL.
/// Derived Ord enables direct comparison: Severity::Critical > Severity::High.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Severity {
    Normal,
    Watch,
    Warn,
    High,
    Critical,
}

// ─── SignalSource ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SignalSource {
    #[serde(rename = "SIMULATOR")]
    Simulator,
    #[serde(rename = "OBD2_GENERIC")]
    Obd2Generic,
    #[serde(rename = "FORD_F450")]
    FordF450,
    #[serde(rename = "CAT_HEAVY")]
    CatHeavy,
    #[serde(rename = "JOHN_DEERE_139")]
    JohnDeere139,
    #[serde(rename = "CUSTOM")]
    Custom,
}

// ─── SignalMap ────────────────────────────────────────────────────────────────

/// Key-value of all sensor readings. All named fields are optional —
/// not every vehicle reports every signal.
/// Extra adapter-specific signals are captured in `extra`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SignalMap {
    // Common OBD-II signals
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coolant_temp:       Option<f64>,   // °C
    #[serde(skip_serializing_if = "Option::is_none")]
    pub engine_rpm:         Option<f64>,   // RPM
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vehicle_speed:      Option<f64>,   // km/h
    #[serde(skip_serializing_if = "Option::is_none")]
    pub throttle_position:  Option<f64>,   // %
    #[serde(skip_serializing_if = "Option::is_none")]
    pub engine_load:        Option<f64>,   // %
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fuel_level:         Option<f64>,   // %
    #[serde(skip_serializing_if = "Option::is_none")]
    pub intake_air_temp:    Option<f64>,   // °C
    #[serde(skip_serializing_if = "Option::is_none")]
    pub battery_voltage:    Option<f64>,   // V
    #[serde(skip_serializing_if = "Option::is_none")]
    pub brake_pedal:        Option<f64>,   // 0=off, 1=full
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oil_pressure:       Option<f64>,   // kPa
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dtc_codes:          Option<Vec<String>>,

    // Heavy fleet extensions
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hydraulic_pressure: Option<f64>,   // kPa — Cat/JD
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transmission_temp:  Option<f64>,   // °C
    #[serde(skip_serializing_if = "Option::is_none")]
    pub axle_weight:        Option<f64>,   // kg
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pto_rpm:            Option<f64>,   // Power Take-Off RPM
    #[serde(skip_serializing_if = "Option::is_none")]
    pub boom_position:      Option<f64>,   // degrees — Cat excavator
    #[serde(skip_serializing_if = "Option::is_none")]
    pub load_weight:        Option<f64>,   // kg — JD haul trucks
    #[serde(skip_serializing_if = "Option::is_none")]
    pub def_level:          Option<f64>,   // % — Diesel Exhaust Fluid
    #[serde(skip_serializing_if = "Option::is_none")]
    pub adblue_level:       Option<f64>,   // % — alternative DEF name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub boost_pressure:     Option<f64>,   // kPa — turbo
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exhaust_temp:       Option<f64>,   // °C

    // Arbitrary additional signals from any adapter
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

// ─── SignalEvent ──────────────────────────────────────────────────────────────

/// A single telemetry frame from any vehicle source.
/// Published on: signals.raw
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignalEvent {
    pub ts:        i64,          // Unix ms timestamp
    pub asset_id:  String,       // Vehicle identifier e.g. "TRUCK-001"
    pub driver_id: String,       // e.g. "DRV-042"
    pub source:    SignalSource,
    pub signals:   SignalMap,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_frame: Option<serde_json::Value>,
}

// ─── FeatureWindow ────────────────────────────────────────────────────────────

/// Computed features over a rolling window. Published on: signals.features
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureWindow {
    pub ts:                i64,
    pub asset_id:          String,
    pub window_seconds:    f64,    // rolling window size (default 30)
    pub coolant_slope:     f64,    // °C per second (positive = heating)
    pub brake_spike_count: f64,    // sudden brake events in window
    pub speed_mean:        f64,    // km/h average
    pub rpm_mean:          f64,
    pub engine_load_mean:  f64,
    pub throttle_variance: f64,    // smoothness indicator
    pub hydraulic_spike:   bool,   // heavy fleet: pressure anomaly
    pub transmission_heat: bool,   // heavy fleet: overtemp flag
    pub dtc_new:           Vec<String>,
    pub signals_snapshot:  SignalMap, // last known values
}

// ─── Decision ─────────────────────────────────────────────────────────────────

/// Which inference tier produced a Decision.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DecisionSource {
    #[serde(rename = "EDGE_AI")]
    EdgeAi,
    #[serde(rename = "ML_STATISTICAL")]
    MlStatistical,
    #[serde(rename = "RULE_ENGINE")]
    RuleEngine,
}

/// Anomaly classification group.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RuleGroup {
    Thermal,
    Braking,
    Speed,
    Hydraulic,
    Electrical,
    Dtc,
    Transmission,
    Fuel,
    Composite,
}

/// Output of InferenceChain, filtered by TrustEngine.
/// Published on: decisions (raw) and decisions.gated (after trust filter).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Decision {
    pub ts:              i64,
    pub asset_id:        String,
    pub severity:        Severity,
    pub rule_id:         String,         // e.g. "coolant_overheat_critical"
    pub rule_group:      RuleGroup,
    pub confidence:      f64,            // 0.0–1.0
    pub triggered_by:    Vec<String>,    // which feature(s) fired this
    pub raw_value:       f64,            // the value that crossed threshold
    pub threshold:       f64,            // the threshold it crossed
    pub decision_source: DecisionSource,
    pub context:         Option<FeatureWindow>,
}

// ─── Action ───────────────────────────────────────────────────────────────────

/// Final operator-facing output. Published on: actions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Action {
    pub seq:             u64,
    pub ts:              i64,
    pub asset_id:        String,
    pub severity:        Severity,
    pub title:           String,         // Short: "Coolant Overheating"
    pub guidance:        String,         // Full operator instruction
    pub rule_id:         String,
    pub speak:           bool,           // TTS fires if true (HIGH/CRITICAL always true)
    pub acknowledged:    bool,
    pub source:          ActionSource,
    pub decision_source: DecisionSource,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ActionSource {
    #[serde(rename = "TEMPLATE")]
    Template,
    #[serde(rename = "LLM")]
    Llm,
}

// ─── Policy ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum VehicleClass {
    LightTruck,
    HeavyEquipment,
    FleetDiesel,
    Passenger,
    Simulator,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RuleOperator {
    Gt,
    Lt,
    Gte,
    Lte,
    Eq,
    Contains,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyRule {
    pub id:          String,
    pub group:       RuleGroup,
    pub signal:      String,     // FeatureWindow field or derived signal name
    pub operator:    RuleOperator,
    pub threshold:   f64,
    pub severity:    Severity,
    pub cooldown_ms: u64,        // minimum ms between same-rule alerts
    pub hysteresis:  f64,        // must exceed threshold by this to re-fire
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyPack {
    pub version:       String,
    pub vehicle_class: VehicleClass,
    pub rules:         Vec<PolicyRule>,
}

// ─── EventEnvelope + BusTopic ────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum BusTopic {
    #[serde(rename = "signals.raw")]
    SignalsRaw,
    #[serde(rename = "signals.features")]
    SignalsFeatures,
    #[serde(rename = "decisions")]
    Decisions,
    #[serde(rename = "decisions.gated")]
    DecisionsGated,
    #[serde(rename = "actions")]
    Actions,
    #[serde(rename = "telemetry.sync")]
    TelemetrySync,
    #[serde(rename = "model.ota")]
    ModelOta,
    #[serde(rename = "system.heartbeat")]
    SystemHeartbeat,
    #[serde(rename = "system.error")]
    SystemError,
}

/// Wraps every message on the bus.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventEnvelope<T> {
    pub id:      String,    // UUID
    pub topic:   BusTopic,
    pub seq:     u64,       // monotonically increasing per topic
    pub ts:      i64,       // Unix ms
    pub payload: T,
}
