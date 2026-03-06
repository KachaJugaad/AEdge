//! adapters/obd2.rs
//! Generic OBD-II adapter (ELM327 / Mode 01 PIDs).
//! Handles any vehicle with a standard OBD-II port and no proprietary extensions.

use crate::types::{SignalEvent, SignalMap, SignalSource};
use super::base::{AdapterError, RawTelematicsFrame, TelematicsAdapter, TelematicsConfig, now_millis};

// ─── PID Constants ────────────────────────────────────────────────────────────
// All PID strings are uppercase 4-char Mode-01 codes per SAE J1979.

const PID_COOLANT_TEMP: &str       = "0105";   // Engine Coolant Temperature
const PID_ENGINE_RPM: &str         = "010C";   // Engine Speed
const PID_VEHICLE_SPEED: &str      = "010D";   // Vehicle Speed
const PID_THROTTLE_POS: &str       = "0111";   // Throttle Position
const PID_ENGINE_LOAD: &str        = "0104";   // Calculated Engine Load
const PID_FUEL_LEVEL: &str         = "012F";   // Fuel Tank Level Input
const PID_INTAKE_AIR_TEMP: &str    = "010F";   // Intake Air Temperature
const PID_BATTERY_VOLTAGE: &str    = "0142";   // Control Module Voltage (mV)

// ─── Decode scale factors / offsets (named constants, never inline) ───────────

/// OBD-II SID 01 PID 05: temperature offset. Formula: raw - 40 → °C
const COOLANT_TEMP_OFFSET: f64      = 40.0;

/// OBD-II SID 01 PID 0C: RPM divisor. Formula: raw_value / 4 → RPM
/// The ELM327 reports the two-byte A,B value as (256*A+B); the adapter
/// receives the already-combined numeric value.
const RPM_DIVISOR: f64              = 4.0;

/// OBD-II SID 01 PID 11 / 04: percentage scale. Formula: raw * 100/255 → %
const PERCENT_SCALE: f64            = 100.0 / 255.0;

/// OBD-II SID 01 PID 0F: intake air temperature offset. Same as coolant.
const INTAKE_AIR_TEMP_OFFSET: f64   = 40.0;

/// OBD-II SID 01 PID 42: battery voltage is reported in mV, convert to V.
const BATTERY_VOLTAGE_MV_TO_V: f64  = 1000.0;

// ─── Obd2Adapter ─────────────────────────────────────────────────────────────

pub struct Obd2Adapter {
    config: TelematicsConfig,
}

impl Obd2Adapter {
    pub fn new(config: TelematicsConfig) -> Self {
        Self { config }
    }

    /// Decode a single OBD-II PID raw value to its engineering unit.
    /// Unknown PIDs pass through unchanged rather than returning an error —
    /// the adapter should not hard-fail on unsupported PIDs.
    fn decode_pid(&self, pid: &str, raw: f64) -> f64 {
        match pid {
            PID_COOLANT_TEMP    => raw - COOLANT_TEMP_OFFSET,
            PID_ENGINE_RPM      => raw / RPM_DIVISOR,
            PID_VEHICLE_SPEED   => raw,                       // km/h direct
            PID_THROTTLE_POS    => raw * PERCENT_SCALE,
            PID_ENGINE_LOAD     => raw * PERCENT_SCALE,
            PID_FUEL_LEVEL      => raw * PERCENT_SCALE,
            PID_INTAKE_AIR_TEMP => raw - INTAKE_AIR_TEMP_OFFSET,
            PID_BATTERY_VOLTAGE => raw / BATTERY_VOLTAGE_MV_TO_V,
            _                   => raw,
        }
    }

    /// Map a PID string to the corresponding `SignalMap` field.
    /// Returns `None` for unrecognised PIDs (they are silently skipped).
    fn pid_to_signal(&self, pid: &str) -> Option<PidSignal> {
        match pid {
            PID_COOLANT_TEMP    => Some(PidSignal::CoolantTemp),
            PID_ENGINE_RPM      => Some(PidSignal::EngineRpm),
            PID_VEHICLE_SPEED   => Some(PidSignal::VehicleSpeed),
            PID_THROTTLE_POS    => Some(PidSignal::ThrottlePosition),
            PID_ENGINE_LOAD     => Some(PidSignal::EngineLoad),
            PID_FUEL_LEVEL      => Some(PidSignal::FuelLevel),
            PID_INTAKE_AIR_TEMP => Some(PidSignal::IntakeAirTemp),
            PID_BATTERY_VOLTAGE => Some(PidSignal::BatteryVoltage),
            _                   => None,
        }
    }
}

/// Internal enum to avoid string-matching twice.
enum PidSignal {
    CoolantTemp,
    EngineRpm,
    VehicleSpeed,
    ThrottlePosition,
    EngineLoad,
    FuelLevel,
    IntakeAirTemp,
    BatteryVoltage,
}

impl TelematicsAdapter for Obd2Adapter {
    fn source(&self) -> SignalSource {
        SignalSource::Obd2Generic
    }

    fn validate(&self, frame: &RawTelematicsFrame) -> bool {
        // An OBD-II frame must have at least one PID reading.
        // Frames with only CAN data or only JSON are not valid for this adapter.
        !frame.pid_readings.is_empty()
    }

    fn normalize(&self, frame: &RawTelematicsFrame) -> Result<SignalEvent, AdapterError> {
        if !self.validate(frame) {
            return Err(AdapterError::ValidationFailure(
                "OBD-II frame has no PID readings".into(),
            ));
        }

        let mut signals = SignalMap::default();

        for (pid, &raw) in &frame.pid_readings {
            let pid_upper = pid.to_uppercase();
            let decoded = self.decode_pid(&pid_upper, raw);

            if let Some(signal) = self.pid_to_signal(&pid_upper) {
                match signal {
                    PidSignal::CoolantTemp      => signals.coolant_temp      = Some(decoded),
                    PidSignal::EngineRpm        => signals.engine_rpm        = Some(decoded),
                    PidSignal::VehicleSpeed     => signals.vehicle_speed     = Some(decoded),
                    PidSignal::ThrottlePosition => signals.throttle_position = Some(decoded),
                    PidSignal::EngineLoad       => signals.engine_load       = Some(decoded),
                    PidSignal::FuelLevel        => signals.fuel_level        = Some(decoded),
                    PidSignal::IntakeAirTemp    => signals.intake_air_temp   = Some(decoded),
                    PidSignal::BatteryVoltage   => signals.battery_voltage   = Some(decoded),
                }
            }
            // Unrecognised PIDs are silently skipped — no error.
        }

        // Pull DTC codes from raw_json if present (standard OBD2 extended response)
        if let Some(json) = &frame.raw_json {
            if let Some(dtcs) = json.get("dtc_codes").and_then(|v| v.as_array()) {
                let codes: Vec<String> = dtcs
                    .iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect();
                if !codes.is_empty() {
                    signals.dtc_codes = Some(codes);
                }
            }
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
            PID_COOLANT_TEMP,
            PID_ENGINE_RPM,
            PID_VEHICLE_SPEED,
            PID_THROTTLE_POS,
            PID_ENGINE_LOAD,
            PID_FUEL_LEVEL,
            PID_INTAKE_AIR_TEMP,
            PID_BATTERY_VOLTAGE,
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
            asset_id:     "TRUCK-001".into(),
            driver_id:    "DRV-042".into(),
            vehicle_class: "light_duty".into(),
        }
    }

    fn frame_with_pid(pid: &str, raw: f64) -> RawTelematicsFrame {
        let mut pid_readings = HashMap::new();
        pid_readings.insert(pid.to_string(), raw);
        RawTelematicsFrame {
            timestamp: 1_700_000_000_000,
            pid_readings,
            ..Default::default()
        }
    }

    fn empty_frame() -> RawTelematicsFrame {
        RawTelematicsFrame::default()
    }

    // ── Core PID decode tests ─────────────────────────────────────────────────

    #[test]
    fn test_obd2_coolant_temp_pid_0105() {
        // PID 0105 raw=125 → 125 - 40 = 85.0 °C
        let adapter = Obd2Adapter::new(config());
        let frame = frame_with_pid("0105", 125.0);
        let event = adapter.normalize(&frame).unwrap();
        let coolant = event.signals.coolant_temp.expect("coolant_temp must be present");
        assert!(
            (coolant - 85.0).abs() < 0.001,
            "Expected 85.0°C, got {coolant}"
        );
    }

    #[test]
    fn test_obd2_engine_rpm_pid_010c() {
        // PID 010C raw=3200 → 3200 / 4 = 800 RPM
        let adapter = Obd2Adapter::new(config());
        let frame = frame_with_pid("010C", 3200.0);
        let event = adapter.normalize(&frame).unwrap();
        let rpm = event.signals.engine_rpm.expect("engine_rpm must be present");
        assert!((rpm - 800.0).abs() < 0.001, "Expected 800 RPM, got {rpm}");
    }

    #[test]
    fn test_obd2_vehicle_speed_pid_010d() {
        // PID 010D raw=90 → 90 km/h (direct)
        let adapter = Obd2Adapter::new(config());
        let frame = frame_with_pid("010D", 90.0);
        let event = adapter.normalize(&frame).unwrap();
        let speed = event.signals.vehicle_speed.expect("vehicle_speed must be present");
        assert!((speed - 90.0).abs() < 0.001, "Expected 90 km/h, got {speed}");
    }

    #[test]
    fn test_obd2_throttle_position_pid_0111() {
        // PID 0111 raw=255 → 255 * 100/255 = 100.0%
        let adapter = Obd2Adapter::new(config());
        let frame = frame_with_pid("0111", 255.0);
        let event = adapter.normalize(&frame).unwrap();
        let throttle = event.signals.throttle_position.expect("throttle_position must be present");
        assert!((throttle - 100.0).abs() < 0.01, "Expected 100.0%, got {throttle}");
    }

    #[test]
    fn test_obd2_engine_load_pid_0104() {
        // PID 0104 raw=128 → 128 * 100/255 ≈ 50.196%
        let adapter = Obd2Adapter::new(config());
        let frame = frame_with_pid("0104", 128.0);
        let event = adapter.normalize(&frame).unwrap();
        let load = event.signals.engine_load.expect("engine_load must be present");
        let expected = 128.0 * 100.0 / 255.0;
        assert!((load - expected).abs() < 0.001, "Expected {expected}%, got {load}");
    }

    #[test]
    fn test_obd2_validate_empty_frame_returns_false() {
        let adapter = Obd2Adapter::new(config());
        let frame = empty_frame();
        assert!(!adapter.validate(&frame), "Empty frame must fail validation");
    }

    #[test]
    fn test_obd2_normalize_empty_frame_returns_error() {
        let adapter = Obd2Adapter::new(config());
        let frame = empty_frame();
        assert!(
            adapter.normalize(&frame).is_err(),
            "normalize() on empty frame must return Err"
        );
    }

    #[test]
    fn test_obd2_signal_event_metadata() {
        // Verify asset_id, driver_id, and source are propagated correctly
        let adapter = Obd2Adapter::new(config());
        let frame = frame_with_pid("0105", 100.0);
        let event = adapter.normalize(&frame).unwrap();
        assert_eq!(event.asset_id, "TRUCK-001");
        assert_eq!(event.driver_id, "DRV-042");
        assert_eq!(event.source, SignalSource::Obd2Generic);
    }

    #[test]
    fn test_obd2_unknown_pid_is_silently_skipped() {
        // An unrecognised PID should not cause an error; it is simply dropped.
        let adapter = Obd2Adapter::new(config());
        let mut pid_readings = HashMap::new();
        pid_readings.insert("0105".into(), 80.0);  // known
        pid_readings.insert("CAFE".into(), 42.0);  // unknown
        let frame = RawTelematicsFrame {
            timestamp: 1_700_000_000_000,
            pid_readings,
            ..Default::default()
        };
        let event = adapter.normalize(&frame).unwrap();
        assert!(event.signals.coolant_temp.is_some());
    }

    #[test]
    fn test_obd2_lowercase_pid_key_accepted() {
        // ELM327 dongles sometimes return lowercase keys
        let adapter = Obd2Adapter::new(config());
        let frame = frame_with_pid("010c", 2000.0);  // lowercase
        let event = adapter.normalize(&frame).unwrap();
        let rpm = event.signals.engine_rpm.expect("engine_rpm must be present");
        assert!((rpm - 500.0).abs() < 0.001, "Expected 500 RPM, got {rpm}");
    }

    #[test]
    fn test_obd2_dtc_codes_from_raw_json() {
        // DTC codes passed via raw_json should be propagated to signals.dtc_codes
        let adapter = Obd2Adapter::new(config());
        let mut pid_readings = HashMap::new();
        pid_readings.insert("0105".into(), 80.0);
        let frame = RawTelematicsFrame {
            timestamp: 1_700_000_000_000,
            pid_readings,
            raw_json: Some(serde_json::json!({"dtc_codes": ["P0300", "P0301"]})),
            ..Default::default()
        };
        let event = adapter.normalize(&frame).unwrap();
        let dtcs = event.signals.dtc_codes.expect("dtc_codes must be present");
        assert_eq!(dtcs, vec!["P0300", "P0301"]);
    }
}
