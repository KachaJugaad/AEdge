// packages/contracts/src/index.ts
// AnomEdge Shared Contracts — Version 1.0 — FROZEN after Day 1 merge
// DO NOT CHANGE without all-team sign-off.

export type Severity = 'NORMAL' | 'WATCH' | 'WARN' | 'HIGH' | 'CRITICAL';

// ─── Raw Signal (from Simulator or real telematics adapter) ──────────────────

export type SignalSource =
  | 'SIMULATOR'
  | 'OBD2_GENERIC'
  | 'FORD_F450'
  | 'CAT_HEAVY'
  | 'JOHN_DEERE_139'
  | 'CUSTOM';

export interface SignalMap {
  // Common OBD-II signals (all vehicles)
  coolant_temp?:        number;   // °C
  engine_rpm?:          number;   // RPM
  vehicle_speed?:       number;   // km/h
  throttle_position?:   number;   // %
  engine_load?:         number;   // %
  fuel_level?:          number;   // %
  intake_air_temp?:     number;   // °C
  battery_voltage?:     number;   // V
  brake_pedal?:         number;   // 0=off, 1=on or % pressure
  oil_pressure?:        number;   // kPa
  dtc_codes?:           string[]; // Diagnostic Trouble Codes

  // Heavy fleet extensions (Cat / JD139 / F450)
  hydraulic_pressure?:  number;   // kPa — Cat/JD specific
  transmission_temp?:   number;   // °C
  axle_weight?:         number;   // kg
  pto_rpm?:             number;   // Power Take-Off RPM
  boom_position?:       number;   // degrees — Cat excavator
  load_weight?:         number;   // kg — JD haul trucks
  def_level?:           number;   // % — Diesel Exhaust Fluid
  adblue_level?:        number;   // % — alternative name
  boost_pressure?:      number;   // kPa — turbo
  exhaust_temp?:        number;   // °C

  // Arbitrary additional signals from any adapter
  [key: string]: number | string | string[] | undefined;
}

export interface SignalEvent {
  ts:         number;       // Unix ms timestamp
  asset_id:   string;       // Vehicle identifier e.g. "TRUCK-001"
  driver_id:  string;       // e.g. "DRV-042"
  source:     SignalSource; // Which telematics adapter produced this
  signals:    SignalMap;    // Key-value of all PID readings
  raw_frame?: unknown;      // Original bytes (optional, for debugging)
}

// ─── Feature Window (computed by FeatureEngine) ──────────────────────────────

export interface FeatureWindow {
  ts:                 number;
  asset_id:           string;
  window_seconds:     number;           // rolling window size (default 30)
  coolant_slope:      number;           // °C per second (positive = heating)
  brake_spike_count:  number;           // sudden brake events in window
  speed_mean:         number;           // km/h average
  rpm_mean:           number;
  engine_load_mean:   number;
  throttle_variance:  number;           // smoothness indicator
  hydraulic_spike:    boolean;          // heavy fleet: pressure anomaly
  transmission_heat:  boolean;          // heavy fleet: overtemp flag
  dtc_new:            string[];         // new DTC codes since last window
  signals_snapshot:   Partial<SignalMap>; // last known values
}

// ─── Decision (output of InferenceChain, gated by TrustEngine) ───────────────

export type DecisionSource = 'EDGE_AI' | 'ML_STATISTICAL' | 'RULE_ENGINE';

export type RuleGroup =
  | 'thermal'
  | 'braking'
  | 'speed'
  | 'hydraulic'
  | 'electrical'
  | 'dtc'
  | 'transmission'
  | 'fuel'
  | 'composite';

export interface Decision {
  ts:              number;
  asset_id:        string;
  severity:        Severity;
  rule_id:         string;          // e.g. "coolant_overheat_critical"
  rule_group:      RuleGroup;
  confidence:      number;          // 0.0–1.0
  triggered_by:    string[];        // which feature(s) fired this rule
  raw_value:       number;          // the value that crossed threshold
  threshold:       number;          // the threshold it crossed
  decision_source: DecisionSource;  // which inference tier produced this
  context:         Partial<FeatureWindow>;
}

// ─── Action (final output to operator, published on: actions) ─────────────────

export interface Action {
  seq:             number;            // monotonic sequence number
  ts:              number;
  asset_id:        string;
  severity:        Severity;
  title:           string;            // Short: "Coolant Overheating"
  guidance:        string;            // Full operator instruction
  rule_id:         string;
  speak:           boolean;           // TTS fires if true (HIGH/CRITICAL always true)
  acknowledged:    boolean;
  source:          'TEMPLATE' | 'LLM';
  decision_source: DecisionSource;    // which tier produced the underlying decision
}

// ─── Policy (loaded from YAML, drives Rule Engine) ────────────────────────────

export type VehicleClass =
  | 'LIGHT_TRUCK'       // Ford F450, pickups
  | 'HEAVY_EQUIPMENT'   // Cat, JD139
  | 'FLEET_DIESEL'      // Generic long-haul
  | 'PASSENGER'
  | 'SIMULATOR';

export interface PolicyRule {
  id:          string;
  group:       RuleGroup;
  signal:      string;    // FeatureWindow field or derived signal name
  operator:    'gt' | 'lt' | 'gte' | 'lte' | 'eq' | 'contains';
  threshold:   number;
  severity:    Severity;
  cooldown_ms: number;    // minimum ms between same-rule alerts
  hysteresis:  number;    // must exceed threshold by this to re-fire
  description: string;
}

export interface PolicyPack {
  version:       string;
  vehicle_class: VehicleClass;
  rules:         PolicyRule[];
}

// ─── EventEnvelope (wraps all bus messages) ───────────────────────────────────

export type BusTopic =
  | 'signals.raw'
  | 'signals.features'
  | 'decisions'
  | 'decisions.gated'
  | 'actions'
  | 'telemetry.sync'
  | 'model.ota'
  | 'system.heartbeat'
  | 'system.error';

export interface EventEnvelope<T = unknown> {
  id:      string;    // UUID
  topic:   BusTopic;
  seq:     number;    // monotonically increasing per topic
  ts:      number;    // Unix ms
  payload: T;
}
