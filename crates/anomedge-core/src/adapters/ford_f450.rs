//! adapters/ford_f450.rs
//! Ford F450 Super Duty telematics adapter.
//!
//! Handles three input paths in order:
//!   1. Standard OBD-II Mode-01 PIDs (shared with Obd2Adapter)
//!   2. Ford-specific extended Mode-22 PIDs (proprietary F450 signals)
//!   3. FordPass / SYNC 4 JSON telemetry (cloud API response)
//!   4. Raw CAN frames (direct MS-CAN bus tap @ 125 kbps)

use crate::types::{SignalEvent, SignalMap, SignalSource};
use super::base::{
    AdapterError, CanFrame, RawTelematicsFrame, TelematicsAdapter, TelematicsConfig, now_millis,
};

// ─── Standard OBD-II PIDs (Mode 01) ──────────────────────────────────────────

const PID_COOLANT_TEMP: &str    = "0105";
const PID_ENGINE_RPM: &str      = "010C";
const PID_VEHICLE_SPEED: &str   = "010D";
const PID_THROTTLE_POS: &str    = "0111";
const PID_ENGINE_LOAD: &str     = "0104";
const PID_FUEL_LEVEL: &str      = "012F";
const PID_BATTERY_VOLTAGE: &str = "0142";

// ─── Ford Mode-22 Extended PIDs ───────────────────────────────────────────────

const PID_OIL_PRESSURE: &str    = "22FF00";   // kPa
const PID_TRANS_TEMP: &str      = "22FF01";   // raw - 40 → °C
const PID_DEF_LEVEL: &str       = "22FF03";   // raw * 100/255 → %

// ─── OBD-II decode constants ──────────────────────────────────────────────────

const COOLANT_TEMP_OFFSET: f64      = 40.0;
const RPM_DIVISOR: f64              = 4.0;
const PERCENT_SCALE: f64            = 100.0 / 255.0;
const BATTERY_VOLTAGE_MV_TO_V: f64  = 1000.0;

// ─── Ford Mode-22 decode constants ───────────────────────────────────────────

/// Oil pressure raw → kPa (Mode 22 FF00)
const OIL_PRESSURE_SCALE: f64      = 1.0;   // raw is already in kPa
/// Transmission temp raw offset → °C (Mode 22 FF01)
const TRANS_TEMP_OFFSET: f64       = 40.0;
/// DEF level scale → % (Mode 22 FF03)
const DEF_LEVEL_SCALE: f64         = 100.0 / 255.0;

// ─── CAN frame IDs (Ford MS-CAN) ─────────────────────────────────────────────

const CAN_ID_ENGINE_DATA: u32      = 0x3B3;   // engine_rpm + engine_load
const CAN_ID_TRANS_STATUS: u32     = 0x420;   // transmission_temp
const CAN_ID_VEHICLE_SPEED: u32    = 0x217;   // vehicle_speed

// ─── CAN decode constants ─────────────────────────────────────────────────────

/// Engine RPM: (byte1 << 8 | byte0) / 4.0
const CAN_RPM_DIVISOR: f64         = 4.0;
/// Engine load: byte2 * 100/255
const CAN_LOAD_SCALE: f64          = 100.0 / 255.0;
/// Transmission temp: byte0 - 40
const CAN_TRANS_TEMP_OFFSET: f64   = 40.0;
/// Vehicle speed: (byte1 << 8 | byte0) / 10.0 → km/h
const CAN_SPEED_DIVISOR: f64       = 10.0;

// ─── FordPass JSON field names ────────────────────────────────────────────────

const FP_COOLANT_TEMP: &str        = "engineCoolantTemp";
const FP_TRANS_TEMP: &str          = "transmissionFluidTemp";
const FP_DEF_LEVEL: &str           = "defFluidLevel";
const FP_ENGINE_RPM: &str          = "engineRpm";
const FP_VEHICLE_SPEED: &str       = "speed";
const FP_THROTTLE: &str            = "throttlePosition";
const FP_FUEL_LEVEL: &str          = "fuelLevel";
const FP_BATTERY_VOLTAGE: &str     = "batteryVoltage";
const FP_OIL_PRESSURE: &str        = "oilPressure";

// ─── FordF450Adapter ──────────────────────────────────────────────────────────

pub struct FordF450Adapter {
    config: TelematicsConfig,
}

impl FordF450Adapter {
    pub fn new(config: TelematicsConfig) -> Self {
        Self { config }
    }

    /// Decode a PID value (standard OBD-II or Ford Mode-22 extension).
    fn decode_pid(&self, pid: &str, raw: f64) -> f64 {
        match pid {
            PID_COOLANT_TEMP    => raw - COOLANT_TEMP_OFFSET,
            PID_ENGINE_RPM      => raw / RPM_DIVISOR,
            PID_VEHICLE_SPEED   => raw,
            PID_THROTTLE_POS    => raw * PERCENT_SCALE,
            PID_ENGINE_LOAD     => raw * PERCENT_SCALE,
            PID_FUEL_LEVEL      => raw * PERCENT_SCALE,
            PID_BATTERY_VOLTAGE => raw / BATTERY_VOLTAGE_MV_TO_V,
            PID_OIL_PRESSURE    => raw * OIL_PRESSURE_SCALE,
            PID_TRANS_TEMP      => raw - TRANS_TEMP_OFFSET,
            PID_DEF_LEVEL       => raw * DEF_LEVEL_SCALE,
            _                   => raw,
        }
    }

    /// Apply decoded PID value to the correct `SignalMap` field.
    fn apply_pid(&self, pid: &str, decoded: f64, signals: &mut SignalMap) {
        match pid {
            PID_COOLANT_TEMP    => signals.coolant_temp      = Some(decoded),
            PID_ENGINE_RPM      => signals.engine_rpm        = Some(decoded),
            PID_VEHICLE_SPEED   => signals.vehicle_speed     = Some(decoded),
            PID_THROTTLE_POS    => signals.throttle_position = Some(decoded),
            PID_ENGINE_LOAD     => signals.engine_load       = Some(decoded),
            PID_FUEL_LEVEL      => signals.fuel_level        = Some(decoded),
            PID_BATTERY_VOLTAGE => signals.battery_voltage   = Some(decoded),
            PID_OIL_PRESSURE    => signals.oil_pressure      = Some(decoded),
            PID_TRANS_TEMP      => signals.transmission_temp = Some(decoded),
            PID_DEF_LEVEL       => signals.def_level         = Some(decoded),
            _                   => { /* unsupported PID — skip */ }
        }
    }

    /// Decode FordPass JSON object fields into the signal map.
    /// Only numeric fields are processed; unknown fields are silently skipped.
    fn decode_fordpass_json(&self, json: &serde_json::Value, signals: &mut SignalMap) {
        let apply_f64 = |key: &str| -> Option<f64> {
            json.get(key).and_then(|v| v.as_f64())
        };

        if let Some(v) = apply_f64(FP_COOLANT_TEMP)    { signals.coolant_temp      = Some(v); }
        if let Some(v) = apply_f64(FP_TRANS_TEMP)       { signals.transmission_temp = Some(v); }
        if let Some(v) = apply_f64(FP_DEF_LEVEL)        { signals.def_level         = Some(v); }
        if let Some(v) = apply_f64(FP_ENGINE_RPM)       { signals.engine_rpm        = Some(v); }
        if let Some(v) = apply_f64(FP_VEHICLE_SPEED)    { signals.vehicle_speed     = Some(v); }
        if let Some(v) = apply_f64(FP_THROTTLE)         { signals.throttle_position = Some(v); }
        if let Some(v) = apply_f64(FP_FUEL_LEVEL)       { signals.fuel_level        = Some(v); }
        if let Some(v) = apply_f64(FP_BATTERY_VOLTAGE)  { signals.battery_voltage   = Some(v); }
        if let Some(v) = apply_f64(FP_OIL_PRESSURE)     { signals.oil_pressure      = Some(v); }

        // DTC codes from FordPass format (field: "dtcCodes")
        if let Some(arr) = json.get("dtcCodes").and_then(|v| v.as_array()) {
            let codes: Vec<String> = arr
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect();
            if !codes.is_empty() {
                signals.dtc_codes = Some(codes);
            }
        }
    }

    /// Decode Ford F450 CAN frames (MS-CAN @ 125 kbps).
    fn decode_can_frames(&self, frames: &[CanFrame], signals: &mut SignalMap) {
        for frame in frames {
            match frame.id {
                CAN_ID_ENGINE_DATA => {
                    // Bytes 0-1: engine RPM (big-endian pair → / 4.0)
                    if frame.data.len() >= 3 {
                        let rpm_raw = ((frame.data[1] as u16) << 8) | (frame.data[0] as u16);
                        signals.engine_rpm = Some(f64::from(rpm_raw) / CAN_RPM_DIVISOR);
                        // Byte 2: engine load (raw * 100/255)
                        signals.engine_load = Some(f64::from(frame.data[2]) * CAN_LOAD_SCALE);
                    }
                }
                CAN_ID_TRANS_STATUS => {
                    // Byte 0: transmission temp (raw - 40)
                    if !frame.data.is_empty() {
                        signals.transmission_temp =
                            Some(f64::from(frame.data[0]) - CAN_TRANS_TEMP_OFFSET);
                    }
                }
                CAN_ID_VEHICLE_SPEED => {
                    // Bytes 0-1: vehicle speed big-endian pair / 10.0 → km/h
                    if frame.data.len() >= 2 {
                        let spd_raw = ((frame.data[1] as u16) << 8) | (frame.data[0] as u16);
                        signals.vehicle_speed = Some(f64::from(spd_raw) / CAN_SPEED_DIVISOR);
                    }
                }
                _ => { /* unknown CAN ID — skip */ }
            }
        }
    }
}

impl TelematicsAdapter for FordF450Adapter {
    fn source(&self) -> SignalSource {
        SignalSource::FordF450
    }

    /// A Ford F450 frame is valid if it has at least one of:
    /// - PID readings (OBD-II or Mode-22)
    /// - FordPass JSON payload
    /// - Raw CAN frames
    fn validate(&self, frame: &RawTelematicsFrame) -> bool {
        !frame.pid_readings.is_empty()
            || frame.raw_json.is_some()
            || !frame.can_frames.is_empty()
    }

    fn normalize(&self, frame: &RawTelematicsFrame) -> Result<SignalEvent, AdapterError> {
        if !self.validate(frame) {
            return Err(AdapterError::ValidationFailure(
                "Ford F450 frame has no PID readings, JSON, or CAN frames".into(),
            ));
        }

        let mut signals = SignalMap::default();

        // Pass 1: OBD-II + Mode-22 PID readings
        for (pid, &raw) in &frame.pid_readings {
            let pid_upper = pid.to_uppercase();
            let decoded = self.decode_pid(&pid_upper, raw);
            self.apply_pid(&pid_upper, decoded, &mut signals);
        }

        // Pass 2: FordPass JSON (overrides PID values on conflict — cloud data is fresher)
        if let Some(json) = &frame.raw_json {
            self.decode_fordpass_json(json, &mut signals);
        }

        // Pass 3: CAN frames (lowest priority — overridden by JSON when both present)
        // We only apply CAN values for signals not already set by PIDs or JSON.
        if !frame.can_frames.is_empty() {
            let mut can_signals = SignalMap::default();
            self.decode_can_frames(&frame.can_frames, &mut can_signals);

            // Merge: CAN fills gaps, does not overwrite already-set values
            if signals.engine_rpm.is_none()        { signals.engine_rpm        = can_signals.engine_rpm; }
            if signals.engine_load.is_none()       { signals.engine_load       = can_signals.engine_load; }
            if signals.transmission_temp.is_none() { signals.transmission_temp = can_signals.transmission_temp; }
            if signals.vehicle_speed.is_none()     { signals.vehicle_speed     = can_signals.vehicle_speed; }
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
            asset_id:      "F450-001".into(),
            driver_id:     "DRV-100".into(),
            vehicle_class: "light_duty".into(),
        }
    }

    fn empty_frame() -> RawTelematicsFrame {
        RawTelematicsFrame::default()
    }

    // ── Validate ─────────────────────────────────────────────────────────────

    #[test]
    fn test_ford_validate_empty_frame_returns_false() {
        let adapter = FordF450Adapter::new(config());
        assert!(!adapter.validate(&empty_frame()), "Empty frame must fail validation");
    }

    #[test]
    fn test_ford_validate_frame_with_pid_returns_true() {
        let adapter = FordF450Adapter::new(config());
        let mut pid_readings = HashMap::new();
        pid_readings.insert("0105".into(), 100.0);
        let frame = RawTelematicsFrame {
            pid_readings,
            ..Default::default()
        };
        assert!(adapter.validate(&frame));
    }

    #[test]
    fn test_ford_validate_frame_with_json_only_returns_true() {
        let adapter = FordF450Adapter::new(config());
        let frame = RawTelematicsFrame {
            raw_json: Some(serde_json::json!({"engineCoolantTemp": 90.0})),
            ..Default::default()
        };
        assert!(adapter.validate(&frame));
    }

    // ── FordPass JSON decoding ────────────────────────────────────────────────

    #[test]
    fn test_ford_json_coolant_temp() {
        // {"engineCoolantTemp": 92.5} → coolant_temp = 92.5
        let adapter = FordF450Adapter::new(config());
        let frame = RawTelematicsFrame {
            timestamp: 1_700_000_000_000,
            raw_json: Some(serde_json::json!({"engineCoolantTemp": 92.5})),
            ..Default::default()
        };
        let event = adapter.normalize(&frame).unwrap();
        let temp = event.signals.coolant_temp.expect("coolant_temp must be present");
        assert!((temp - 92.5).abs() < 0.001, "Expected 92.5°C, got {temp}");
    }

    #[test]
    fn test_ford_json_transmission_temp() {
        let adapter = FordF450Adapter::new(config());
        let frame = RawTelematicsFrame {
            timestamp: 1_700_000_000_000,
            raw_json: Some(serde_json::json!({"transmissionFluidTemp": 75.0})),
            ..Default::default()
        };
        let event = adapter.normalize(&frame).unwrap();
        let temp = event.signals.transmission_temp.expect("transmission_temp must be present");
        assert!((temp - 75.0).abs() < 0.001, "Expected 75.0°C, got {temp}");
    }

    #[test]
    fn test_ford_json_def_level() {
        let adapter = FordF450Adapter::new(config());
        let frame = RawTelematicsFrame {
            timestamp: 1_700_000_000_000,
            raw_json: Some(serde_json::json!({"defFluidLevel": 68.0})),
            ..Default::default()
        };
        let event = adapter.normalize(&frame).unwrap();
        let level = event.signals.def_level.expect("def_level must be present");
        assert!((level - 68.0).abs() < 0.001, "Expected 68.0%, got {level}");
    }

    // ── OBD-II PID decoding ───────────────────────────────────────────────────

    #[test]
    fn test_ford_obd2_coolant_temp_pid_0105() {
        // raw=110 → 110 - 40 = 70°C
        let adapter = FordF450Adapter::new(config());
        let mut pid_readings = HashMap::new();
        pid_readings.insert("0105".into(), 110.0);
        let frame = RawTelematicsFrame {
            timestamp: 1_700_000_000_000,
            pid_readings,
            ..Default::default()
        };
        let event = adapter.normalize(&frame).unwrap();
        let temp = event.signals.coolant_temp.expect("coolant_temp must be present");
        assert!((temp - 70.0).abs() < 0.001, "Expected 70.0°C, got {temp}");
    }

    #[test]
    fn test_ford_mode22_oil_pressure() {
        // PID 22FF00 raw=350 → 350 kPa (scale=1.0)
        let adapter = FordF450Adapter::new(config());
        let mut pid_readings = HashMap::new();
        pid_readings.insert("22FF00".into(), 350.0);
        let frame = RawTelematicsFrame {
            timestamp: 1_700_000_000_000,
            pid_readings,
            ..Default::default()
        };
        let event = adapter.normalize(&frame).unwrap();
        let pressure = event.signals.oil_pressure.expect("oil_pressure must be present");
        assert!((pressure - 350.0).abs() < 0.001, "Expected 350 kPa, got {pressure}");
    }

    #[test]
    fn test_ford_mode22_trans_temp() {
        // PID 22FF01 raw=115 → 115 - 40 = 75°C
        let adapter = FordF450Adapter::new(config());
        let mut pid_readings = HashMap::new();
        pid_readings.insert("22FF01".into(), 115.0);
        let frame = RawTelematicsFrame {
            timestamp: 1_700_000_000_000,
            pid_readings,
            ..Default::default()
        };
        let event = adapter.normalize(&frame).unwrap();
        let temp = event.signals.transmission_temp.expect("transmission_temp must be present");
        assert!((temp - 75.0).abs() < 0.001, "Expected 75.0°C, got {temp}");
    }

    // ── CAN frame decoding ────────────────────────────────────────────────────

    #[test]
    fn test_ford_can_engine_rpm_0x3b3() {
        // CAN 0x3B3 bytes [0x00, 0x1F, 0x80] → rpm_raw = (0x1F<<8)|0x00 = 7936
        // engine_rpm = 7936 / 4.0 = 1984.0 RPM
        let adapter = FordF450Adapter::new(config());
        let frame = RawTelematicsFrame {
            timestamp: 1_700_000_000_000,
            can_frames: vec![CanFrame {
                id: 0x3B3,
                data: vec![0x00, 0x1F, 0x80],
                ts: 1_700_000_000_000,
            }],
            ..Default::default()
        };
        let event = adapter.normalize(&frame).unwrap();
        let rpm = event.signals.engine_rpm.expect("engine_rpm must be present");
        // rpm_raw = (0x1F << 8) | 0x00 = 7936 → / 4.0 = 1984.0
        assert!((rpm - 1984.0).abs() < 0.001, "Expected 1984 RPM, got {rpm}");
    }

    #[test]
    fn test_ford_can_vehicle_speed_0x217() {
        // CAN 0x217 bytes [0xE8, 0x03] → spd_raw = (0x03<<8)|0xE8 = 1000
        // vehicle_speed = 1000 / 10.0 = 100.0 km/h
        let adapter = FordF450Adapter::new(config());
        let frame = RawTelematicsFrame {
            timestamp: 1_700_000_000_000,
            can_frames: vec![CanFrame {
                id: 0x217,
                data: vec![0xE8, 0x03],
                ts: 1_700_000_000_000,
            }],
            ..Default::default()
        };
        let event = adapter.normalize(&frame).unwrap();
        let speed = event.signals.vehicle_speed.expect("vehicle_speed must be present");
        assert!((speed - 100.0).abs() < 0.001, "Expected 100.0 km/h, got {speed}");
    }

    #[test]
    fn test_ford_json_overrides_pid_on_coolant_temp() {
        // When both PID and JSON are present, JSON (cloud) value wins
        let adapter = FordF450Adapter::new(config());
        let mut pid_readings = HashMap::new();
        pid_readings.insert("0105".into(), 130.0);  // PID: 130-40 = 90°C
        let frame = RawTelematicsFrame {
            timestamp: 1_700_000_000_000,
            pid_readings,
            raw_json: Some(serde_json::json!({"engineCoolantTemp": 95.0})),  // JSON: 95°C
            ..Default::default()
        };
        let event = adapter.normalize(&frame).unwrap();
        let temp = event.signals.coolant_temp.expect("coolant_temp must be present");
        // JSON takes precedence over PID
        assert!((temp - 95.0).abs() < 0.001, "Expected JSON value 95.0°C, got {temp}");
    }

    #[test]
    fn test_ford_source_is_ford_f450() {
        let adapter = FordF450Adapter::new(config());
        let frame = RawTelematicsFrame {
            raw_json: Some(serde_json::json!({"engineCoolantTemp": 80.0})),
            ..Default::default()
        };
        let event = adapter.normalize(&frame).unwrap();
        assert_eq!(event.source, SignalSource::FordF450);
    }
}
