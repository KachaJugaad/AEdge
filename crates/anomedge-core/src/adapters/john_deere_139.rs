//! adapters/john_deere_139.rs
//! John Deere 139 telematics adapter (JDLink / JD Operations Center API).
//!
//! Sources handled in order:
//!   1. J1939 standard SPNs (110, 190, 84 — same formulas as Caterpillar)
//!   2. JD proprietary SPNs (520204–520215)
//!   3. JDLink REST API JSON ("readings" array format or flat JSON)
//!
//! Fault code format: "JD-SPN{spn}-FMI{fmi}" per JD service manual.

use crate::types::{SignalEvent, SignalMap, SignalSource};
use super::base::{
    AdapterError, RawTelematicsFrame, TelematicsAdapter, TelematicsConfig, now_millis,
};

// ─── Standard J1939 SPN constants (shared with Caterpillar) ──────────────────

const SPN_COOLANT_TEMP: u32     = 110;
const SPN_ENGINE_RPM: u32       = 190;
const SPN_VEHICLE_SPEED: u32    = 84;
const SPN_THROTTLE_POS: u32     = 91;
const SPN_ENGINE_LOAD: u32      = 92;
const SPN_FUEL_LEVEL: u32       = 96;
const SPN_BATTERY_VOLTAGE: u32  = 168;
const SPN_OIL_PRESSURE: u32     = 100;
const SPN_TRANS_TEMP: u32       = 127;
const SPN_DEF_LEVEL: u32        = 4076;

// ─── JD Proprietary SPN constants ─────────────────────────────────────────────

const SPN_JD_HYDRAULIC_PRESSURE: u32  = 520204;   // Implement hydraulic pressure
const SPN_JD_HYDRAULIC_OIL_TEMP: u32  = 520205;   // Hydraulic oil temperature → extra["hydraulic_oil_temp"]
const SPN_JD_BOOM_POSITION: u32       = 520206;   // Loader lift arm angle
const SPN_JD_LOAD_WEIGHT: u32         = 520207;   // Body payload weight
const SPN_JD_PAYLOAD_PERCENT: u32     = 520208;   // Payload % rated capacity → extra["payload_percent"]
const SPN_JD_PTO_RPM: u32             = 520209;   // Ground drive motor speed → extra["pto_rpm"]
const SPN_JD_BOOST_PRESSURE: u32      = 520210;   // Turbocharger boost pressure
const SPN_JD_EXHAUST_TEMP: u32        = 520211;   // SCR inlet temperature
const SPN_JD_BLADE_POSITION: u32      = 520212;   // Blade cross-slope (672G) → extra["blade_position"]
const SPN_JD_BLADE_DOWN_FORCE: u32    = 520213;   // Blade draft force → extra["blade_down_force"]
const SPN_JD_DPF_SOOT_LEVEL: u32      = 520214;   // DPF soot loading %
const SPN_JD_DPF_ASH_LEVEL: u32       = 520215;   // DPF ash loading %

// ─── J1939 decode scale factors (SAE J1939-71) ───────────────────────────────

/// SPN 110: coolant temp — 1/32 °C/bit, offset −40°C.
const COOLANT_TEMP_SCALE: f64        = 1.0 / 32.0;
const COOLANT_TEMP_OFFSET: f64       = 40.0;

/// SPN 190: engine speed — 0.125 RPM/bit.
const RPM_SCALE: f64                 = 0.125;

/// SPN 84: vehicle speed — 1/256 km/h per bit.
const SPEED_SCALE: f64               = 1.0 / 256.0;

/// SPN 91, 92, 96: percentage — 0.4 %/bit.
const PERCENT_SCALE: f64             = 0.4;

/// SPN 100: oil pressure — 4 kPa/bit.
const OIL_PRESSURE_SCALE: f64        = 4.0;

/// SPN 168: battery voltage — 0.05 V/bit.
const BATTERY_VOLTAGE_SCALE: f64     = 0.05;

/// SPN 127: transmission temp — 1/32 °C/bit, offset −273 (Kelvin → Celsius).
const TRANS_TEMP_SCALE: f64          = 1.0 / 32.0;
const TRANS_TEMP_KELVIN_OFFSET: f64  = 273.0;

/// SPN 4076: DEF level — 0.4 %/bit.
const DEF_LEVEL_SCALE: f64           = 0.4;

// ─── JD Proprietary SPN scale factors (JD service manual) ────────────────────

/// SPN 520204: hydraulic pressure — 10 kPa/bit.
const JD_HYDRAULIC_PRESSURE_SCALE: f64 = 10.0;

/// SPN 520206: boom/lift arm angle — 0.1 degrees/bit.
const JD_BOOM_POSITION_SCALE: f64      = 0.1;

/// SPN 520207: payload weight — 100 kg/bit.
const JD_LOAD_WEIGHT_SCALE: f64        = 100.0;

/// SPN 520208: payload percentage — 0.4 %/bit.
const JD_PAYLOAD_PERCENT_SCALE: f64    = 0.4;

/// SPN 520214: DPF soot loading — 0.4 %/bit.
const JD_DPF_SOOT_SCALE: f64           = 0.4;

/// SPN 520215: DPF ash loading — 0.4 %/bit.
const JD_DPF_ASH_SCALE: f64            = 0.4;

// ─── JDLink API field name → signal mapping ───────────────────────────────────

/// JDLink flat JSON / "readings" array field name → SignalMap field name.
/// Used for matching; see `apply_jdlink_field` below.
const JDLINK_ENGINE_SPEED: &str             = "engineSpeed";
const JDLINK_GROUND_SPEED: &str             = "groundSpeed";
const JDLINK_COOLANT_TEMP: &str             = "engineCoolantTemperature";
const JDLINK_THROTTLE: &str                 = "throttlePosition";
const JDLINK_ENGINE_LOAD: &str              = "engineLoad";
const JDLINK_FUEL_LEVEL: &str               = "fuelLevelPercent";
const JDLINK_BATTERY_VOLTAGE: &str          = "batteryVoltage";
const JDLINK_OIL_PRESSURE: &str             = "engineOilPressure";
const JDLINK_HYDRAULIC_PRESSURE: &str       = "hydraulicOilPressure";
const JDLINK_TRANS_TEMP: &str               = "transmissionOilTemperature";
const JDLINK_DEF_LEVEL: &str                = "exhaustFluidLevel";
const JDLINK_LOAD_WEIGHT: &str              = "payloadMass";
const JDLINK_BOOST_PRESSURE: &str           = "boostPressure";
const JDLINK_EXHAUST_TEMP: &str             = "exhaustTemperature";
const JDLINK_BOOM_POSITION: &str            = "liftArmAngle";

// ─── JohnDeere139Adapter ──────────────────────────────────────────────────────

pub struct JohnDeere139Adapter {
    config: TelematicsConfig,
}

impl JohnDeere139Adapter {
    pub fn new(config: TelematicsConfig) -> Self {
        Self { config }
    }

    /// Decode a J1939 / JD-proprietary SPN raw value to engineering units.
    fn decode_spn(&self, spn: u32, raw: f64) -> f64 {
        match spn {
            // Standard J1939 SPNs
            SPN_COOLANT_TEMP            => raw * COOLANT_TEMP_SCALE - COOLANT_TEMP_OFFSET,
            SPN_ENGINE_RPM              => raw * RPM_SCALE,
            SPN_VEHICLE_SPEED           => raw * SPEED_SCALE,
            SPN_THROTTLE_POS            => raw * PERCENT_SCALE,
            SPN_ENGINE_LOAD             => raw * PERCENT_SCALE,
            SPN_FUEL_LEVEL              => raw * PERCENT_SCALE,
            SPN_OIL_PRESSURE            => raw * OIL_PRESSURE_SCALE,
            SPN_BATTERY_VOLTAGE         => raw * BATTERY_VOLTAGE_SCALE,
            SPN_TRANS_TEMP              => raw * TRANS_TEMP_SCALE - TRANS_TEMP_KELVIN_OFFSET,
            SPN_DEF_LEVEL               => raw * DEF_LEVEL_SCALE,
            // JD proprietary SPNs — named SignalMap fields
            SPN_JD_HYDRAULIC_PRESSURE   => raw * JD_HYDRAULIC_PRESSURE_SCALE,
            SPN_JD_BOOM_POSITION        => raw * JD_BOOM_POSITION_SCALE,
            SPN_JD_LOAD_WEIGHT          => raw * JD_LOAD_WEIGHT_SCALE,
            SPN_JD_PAYLOAD_PERCENT      => raw * JD_PAYLOAD_PERCENT_SCALE,
            SPN_JD_DPF_SOOT_LEVEL       => raw * JD_DPF_SOOT_SCALE,
            SPN_JD_DPF_ASH_LEVEL        => raw * JD_DPF_ASH_SCALE,
            // JD proprietary SPNs — routed to extra (no named SignalMap field)
            SPN_JD_HYDRAULIC_OIL_TEMP   => raw,   // pass-through °C
            SPN_JD_PTO_RPM              => raw,   // pass-through RPM
            SPN_JD_BOOST_PRESSURE       => raw,   // pass-through kPa
            SPN_JD_EXHAUST_TEMP         => raw,   // pass-through °C
            SPN_JD_BLADE_POSITION       => raw,   // pass-through degrees
            SPN_JD_BLADE_DOWN_FORCE     => raw,   // pass-through kN
            // All others — pass-through
            _                           => raw,
        }
    }

    /// Write a decoded SPN value into the correct `SignalMap` field.
    /// SPNs with no named field are stored in `signals.extra`.
    fn apply_spn(&self, spn: u32, decoded: f64, signals: &mut SignalMap) {
        match spn {
            SPN_COOLANT_TEMP            => signals.coolant_temp       = Some(decoded),
            SPN_ENGINE_RPM              => signals.engine_rpm         = Some(decoded),
            SPN_VEHICLE_SPEED           => signals.vehicle_speed      = Some(decoded),
            SPN_THROTTLE_POS            => signals.throttle_position  = Some(decoded),
            SPN_ENGINE_LOAD             => signals.engine_load        = Some(decoded),
            SPN_FUEL_LEVEL              => signals.fuel_level         = Some(decoded),
            SPN_OIL_PRESSURE            => signals.oil_pressure       = Some(decoded),
            SPN_BATTERY_VOLTAGE         => signals.battery_voltage    = Some(decoded),
            SPN_TRANS_TEMP              => signals.transmission_temp  = Some(decoded),
            SPN_DEF_LEVEL               => signals.def_level          = Some(decoded),
            SPN_JD_HYDRAULIC_PRESSURE   => signals.hydraulic_pressure = Some(decoded),
            SPN_JD_BOOM_POSITION        => signals.boom_position      = Some(decoded),
            SPN_JD_LOAD_WEIGHT          => signals.load_weight        = Some(decoded),
            SPN_JD_PAYLOAD_PERCENT      => {
                signals.extra.insert("payload_percent".into(), serde_json::Value::from(decoded));
            }
            SPN_JD_DPF_SOOT_LEVEL       => {
                signals.extra.insert("dpf_soot_level".into(), serde_json::Value::from(decoded));
            }
            SPN_JD_DPF_ASH_LEVEL        => {
                signals.extra.insert("dpf_ash_level".into(), serde_json::Value::from(decoded));
            }
            SPN_JD_HYDRAULIC_OIL_TEMP   => {
                signals.extra.insert("hydraulic_oil_temp".into(), serde_json::Value::from(decoded));
            }
            SPN_JD_PTO_RPM              => {
                signals.extra.insert("pto_rpm".into(), serde_json::Value::from(decoded));
            }
            SPN_JD_BOOST_PRESSURE       => signals.boost_pressure = Some(decoded),
            SPN_JD_EXHAUST_TEMP         => signals.exhaust_temp   = Some(decoded),
            SPN_JD_BLADE_POSITION       => {
                signals.extra.insert("blade_position".into(), serde_json::Value::from(decoded));
            }
            SPN_JD_BLADE_DOWN_FORCE     => {
                signals.extra.insert("blade_down_force".into(), serde_json::Value::from(decoded));
            }
            _                           => { /* truly unknown SPN — skip */ }
        }
    }

    /// Apply a JDLink API field name → value pair to the signal map.
    fn apply_jdlink_field(&self, name: &str, value: f64, signals: &mut SignalMap) {
        match name {
            JDLINK_ENGINE_SPEED        => signals.engine_rpm         = Some(value),
            JDLINK_GROUND_SPEED        => signals.vehicle_speed      = Some(value),
            JDLINK_COOLANT_TEMP        => signals.coolant_temp       = Some(value),
            JDLINK_THROTTLE            => signals.throttle_position  = Some(value),
            JDLINK_ENGINE_LOAD         => signals.engine_load        = Some(value),
            JDLINK_FUEL_LEVEL          => signals.fuel_level         = Some(value),
            JDLINK_BATTERY_VOLTAGE     => signals.battery_voltage    = Some(value),
            JDLINK_OIL_PRESSURE        => signals.oil_pressure       = Some(value),
            JDLINK_HYDRAULIC_PRESSURE  => signals.hydraulic_pressure = Some(value),
            JDLINK_TRANS_TEMP          => signals.transmission_temp  = Some(value),
            JDLINK_DEF_LEVEL           => signals.def_level          = Some(value),
            JDLINK_LOAD_WEIGHT         => signals.load_weight        = Some(value),
            JDLINK_BOOST_PRESSURE      => signals.boost_pressure     = Some(value),
            JDLINK_EXHAUST_TEMP        => signals.exhaust_temp       = Some(value),
            JDLINK_BOOM_POSITION       => signals.boom_position      = Some(value),
            _                          => { /* unknown field — skip */ }
        }
    }

    /// Decode JDLink REST API JSON response.
    ///
    /// Handles two JSON formats:
    /// - `{"readings": [{"name": "engineSpeed", "value": 2200}, ...]}` (array format)
    /// - `{"engineSpeed": 2200, "groundSpeed": 45}` (flat format)
    fn decode_jdlink_json(&self, json: &serde_json::Value, signals: &mut SignalMap) {
        // Try "readings" array format first (primary JDLink API v3 format)
        if let Some(readings) = json.get("readings").and_then(|v| v.as_array()) {
            for reading in readings {
                let name  = reading.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let value = reading.get("value").and_then(|v| v.as_f64());
                if let Some(val) = value {
                    self.apply_jdlink_field(name, val, signals);
                }
            }
        } else {
            // Flat JSON format
            if let Some(obj) = json.as_object() {
                for (key, val) in obj {
                    if let Some(v) = val.as_f64() {
                        self.apply_jdlink_field(key, v, signals);
                    }
                }
            }
        }

        // JD fault codes: [{"spn": 110, "fmi": 4}, ...] → "JD-SPN110-FMI4"
        if let Some(arr) = json.get("activeDtcs").and_then(|v| v.as_array()) {
            let codes: Vec<String> = arr
                .iter()
                .filter_map(|entry| {
                    let spn = entry.get("spn").and_then(|v| v.as_u64())?;
                    let fmi = entry.get("fmi").and_then(|v| v.as_u64())?;
                    Some(format!("JD-SPN{spn}-FMI{fmi}"))
                })
                .collect();
            if !codes.is_empty() {
                signals.dtc_codes = Some(codes);
            }
        }

        // Machine / engine hours into extra HashMap
        if let Some(hours) = json.get("engineHours").and_then(|v| v.as_f64()) {
            signals.extra.insert(
                "machine_hours".into(),
                serde_json::Value::from(hours),
            );
        }
    }
}

impl TelematicsAdapter for JohnDeere139Adapter {
    fn source(&self) -> SignalSource {
        SignalSource::JohnDeere139
    }

    /// Valid when at least one of: J1939 SPNs or JDLink JSON is present.
    fn validate(&self, frame: &RawTelematicsFrame) -> bool {
        !frame.j1939_spns.is_empty() || frame.raw_json.is_some()
    }

    fn normalize(&self, frame: &RawTelematicsFrame) -> Result<SignalEvent, AdapterError> {
        if !self.validate(frame) {
            return Err(AdapterError::ValidationFailure(
                "John Deere 139 frame has no J1939 SPNs or JDLink JSON".into(),
            ));
        }

        let mut signals = SignalMap::default();

        // Pass 1: J1939 SPN readings
        for (&spn, &raw) in &frame.j1939_spns {
            let decoded = self.decode_spn(spn, raw);
            self.apply_spn(spn, decoded, &mut signals);
        }

        // Pass 2: JDLink JSON (overrides SPN values on conflict — API data is fresher)
        if let Some(json) = &frame.raw_json {
            self.decode_jdlink_json(json, &mut signals);
        }

        let ts = if frame.timestamp == 0 {
            now_millis()
        } else {
            frame.timestamp as i64
        };

        Ok(SignalEvent {
            ts,
            asset_id:  self.config.asset_id.clone(),
            driver_id: self.config.driver_id.clone(),
            source:    self.source(),
            signals,
            raw_frame: frame.raw_json.clone(),
        })
    }

    fn supported_signals(&self) -> Vec<&'static str> {
        vec![
            "coolant_temp",
            "engine_rpm",
            "vehicle_speed",
            "throttle_position",
            "engine_load",
            "fuel_level",
            "battery_voltage",
            "oil_pressure",
            "transmission_temp",
            "def_level",
            "hydraulic_pressure",
            "boom_position",
            "load_weight",
            "boost_pressure",
            "exhaust_temp",
        ]
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn config() -> TelematicsConfig {
        TelematicsConfig {
            asset_id:      "JD-460E-001".into(),
            driver_id:     "DRV-300".into(),
            vehicle_class: "agricultural".into(),
        }
    }

    fn empty_frame() -> RawTelematicsFrame {
        RawTelematicsFrame::default()
    }

    // ── Validate ─────────────────────────────────────────────────────────────

    #[test]
    fn test_jd_validate_empty_frame_returns_false() {
        let adapter = JohnDeere139Adapter::new(config());
        assert!(!adapter.validate(&empty_frame()), "Empty frame must fail validation");
    }

    #[test]
    fn test_jd_validate_with_json_returns_true() {
        let adapter = JohnDeere139Adapter::new(config());
        let frame = RawTelematicsFrame {
            raw_json: Some(serde_json::json!({"readings": [{"name": "engineSpeed", "value": 2200}]})),
            ..Default::default()
        };
        assert!(adapter.validate(&frame));
    }

    // ── JDLink "readings" array format ────────────────────────────────────────

    #[test]
    fn test_jd_readings_array_ground_speed() {
        // [{"name": "groundSpeed", "value": 45}] → vehicle_speed = 45.0 km/h
        let adapter = JohnDeere139Adapter::new(config());
        let frame = RawTelematicsFrame {
            timestamp: 1_700_000_000_000,
            raw_json: Some(serde_json::json!({
                "readings": [
                    {"name": "groundSpeed", "value": 45}
                ]
            })),
            ..Default::default()
        };
        let event = adapter.normalize(&frame).unwrap();
        let speed = event.signals.vehicle_speed.expect("vehicle_speed must be present");
        assert!((speed - 45.0).abs() < 0.001, "Expected 45.0 km/h, got {speed}");
    }

    #[test]
    fn test_jd_readings_array_engine_speed() {
        // [{"name": "engineSpeed", "value": 2200}] → engine_rpm = 2200.0
        let adapter = JohnDeere139Adapter::new(config());
        let frame = RawTelematicsFrame {
            timestamp: 1_700_000_000_000,
            raw_json: Some(serde_json::json!({
                "readings": [
                    {"name": "engineSpeed", "value": 2200}
                ]
            })),
            ..Default::default()
        };
        let event = adapter.normalize(&frame).unwrap();
        let rpm = event.signals.engine_rpm.expect("engine_rpm must be present");
        assert!((rpm - 2200.0).abs() < 0.001, "Expected 2200 RPM, got {rpm}");
    }

    #[test]
    fn test_jd_readings_array_coolant_temp() {
        // engineCoolantTemperature → coolant_temp = 87.0°C
        let adapter = JohnDeere139Adapter::new(config());
        let frame = RawTelematicsFrame {
            timestamp: 1_700_000_000_000,
            raw_json: Some(serde_json::json!({
                "readings": [
                    {"name": "engineCoolantTemperature", "value": 87.0}
                ]
            })),
            ..Default::default()
        };
        let event = adapter.normalize(&frame).unwrap();
        let temp = event.signals.coolant_temp.expect("coolant_temp must be present");
        assert!((temp - 87.0).abs() < 0.001, "Expected 87.0°C, got {temp}");
    }

    #[test]
    fn test_jd_readings_multi_signals() {
        // Multiple signals in a single readings array
        let adapter = JohnDeere139Adapter::new(config());
        let frame = RawTelematicsFrame {
            timestamp: 1_700_000_000_000,
            raw_json: Some(serde_json::json!({
                "readings": [
                    {"name": "engineSpeed",              "value": 1800},
                    {"name": "groundSpeed",              "value": 32},
                    {"name": "engineCoolantTemperature", "value": 91.5},
                    {"name": "fuelLevelPercent",         "value": 73.0}
                ]
            })),
            ..Default::default()
        };
        let event = adapter.normalize(&frame).unwrap();
        assert!((event.signals.engine_rpm.unwrap()    - 1800.0).abs() < 0.001);
        assert!((event.signals.vehicle_speed.unwrap() -   32.0).abs() < 0.001);
        assert!((event.signals.coolant_temp.unwrap()  -   91.5).abs() < 0.001);
        assert!((event.signals.fuel_level.unwrap()    -   73.0).abs() < 0.001);
    }

    // ── J1939 SPN decoding ────────────────────────────────────────────────────

    #[test]
    fn test_jd_j1939_spn110_coolant_temp() {
        // SPN 110: raw=4128 → 4128/32 − 40 = 129.0 − 40.0 = 89.0°C
        let adapter = JohnDeere139Adapter::new(config());
        let mut j1939_spns = HashMap::new();
        j1939_spns.insert(SPN_COOLANT_TEMP, 4128.0);
        let frame = RawTelematicsFrame {
            timestamp: 1_700_000_000_000,
            j1939_spns,
            ..Default::default()
        };
        let event = adapter.normalize(&frame).unwrap();
        let temp = event.signals.coolant_temp.expect("coolant_temp must be present");
        assert!((temp - 89.0).abs() < 1.0, "Expected ≈89°C, got {temp}");
    }

    #[test]
    fn test_jd_proprietary_spn520204_hydraulic_pressure() {
        // SPN 520204: raw=300 → 300 × 10 = 3000 kPa
        let adapter = JohnDeere139Adapter::new(config());
        let mut j1939_spns = HashMap::new();
        j1939_spns.insert(SPN_JD_HYDRAULIC_PRESSURE, 300.0);
        let frame = RawTelematicsFrame {
            timestamp: 1_700_000_000_000,
            j1939_spns,
            ..Default::default()
        };
        let event = adapter.normalize(&frame).unwrap();
        let pressure = event.signals.hydraulic_pressure.expect("hydraulic_pressure must be present");
        assert!((pressure - 3000.0).abs() < 0.001, "Expected 3000 kPa, got {pressure}");
    }

    // ── Fault code format ─────────────────────────────────────────────────────

    #[test]
    fn test_jd_fault_code_format() {
        // activeDtcs [{"spn": 110, "fmi": 4}] → "JD-SPN110-FMI4"
        let adapter = JohnDeere139Adapter::new(config());
        let frame = RawTelematicsFrame {
            timestamp: 1_700_000_000_000,
            raw_json: Some(serde_json::json!({
                "engineSpeed": 1200.0,
                "activeDtcs": [{"spn": 110, "fmi": 4}, {"spn": 190, "fmi": 2}]
            })),
            ..Default::default()
        };
        let event = adapter.normalize(&frame).unwrap();
        let dtcs = event.signals.dtc_codes.expect("dtc_codes must be present");
        assert_eq!(dtcs, vec!["JD-SPN110-FMI4", "JD-SPN190-FMI2"]);
    }

    #[test]
    fn test_jd_source_is_john_deere_139() {
        let adapter = JohnDeere139Adapter::new(config());
        let frame = RawTelematicsFrame {
            timestamp: 1_700_000_000_000,
            raw_json: Some(serde_json::json!({"engineSpeed": 1500.0})),
            ..Default::default()
        };
        let event = adapter.normalize(&frame).unwrap();
        assert_eq!(event.source, SignalSource::JohnDeere139);
    }
}
