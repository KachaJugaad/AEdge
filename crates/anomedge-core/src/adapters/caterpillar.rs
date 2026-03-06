//! adapters/caterpillar.rs
//! Caterpillar heavy equipment telematics adapter.
//!
//! Sources handled in order:
//!   1. J1939 SPN readings (direct ECM / telematics gateway)
//!   2. Cat VisionLink JSON API response
//!   3. Raw CAN / J1939 frames (29-bit extended ID)
//!
//! Standard J1939 SPNs follow SAE J1939-71.
//! Cat-specific SPNs (2413, 1430, etc.) follow Cat SIS2 documentation.

use crate::types::{SignalEvent, SignalMap, SignalSource};
use super::base::{
    AdapterError, CanFrame, RawTelematicsFrame, TelematicsAdapter, TelematicsConfig, now_millis,
};

// ─── J1939 SPN constants (SAE J1939-71) ──────────────────────────────────────

const SPN_COOLANT_TEMP: u32        = 110;    // Engine Coolant Temperature
const SPN_ENGINE_RPM: u32          = 190;    // Engine Speed
const SPN_VEHICLE_SPEED: u32       = 84;     // Wheel-Based Vehicle Speed
const SPN_THROTTLE_POS: u32        = 91;     // Accelerator Pedal Position 1
const SPN_ENGINE_LOAD: u32         = 92;     // Engine Percent Load At Current Speed
const SPN_FUEL_LEVEL: u32          = 96;     // Fuel Level 1
const SPN_OIL_PRESSURE: u32        = 100;    // Engine Oil Pressure
const SPN_BATTERY_VOLTAGE: u32     = 168;    // Battery Potential / Power Input 1
const SPN_TRANS_TEMP: u32          = 127;    // Transmission Oil Temperature 1

// Cat-specific SPNs
const SPN_HYDRAULIC_PRESSURE: u32  = 2413;   // Hydraulic System Pressure
const SPN_LOAD_WEIGHT: u32         = 1430;   // Payload Mass

// ─── J1939 decode scale factors (SAE J1939-71 Appendix B) ────────────────────

/// SPN 110: Engine coolant temperature. 1/32 °C/bit, offset -40°C.
/// Formula: raw × (1/32) − 40 → °C
const COOLANT_TEMP_SCALE: f64      = 1.0 / 32.0;   // = 0.03125
const COOLANT_TEMP_OFFSET: f64     = 40.0;

/// SPN 190: Engine speed. 0.125 RPM/bit.
const RPM_SCALE: f64               = 0.125;

/// SPN 84: Vehicle speed. 1/256 km/h per bit (approx 0.00390625).
const SPEED_SCALE: f64             = 1.0 / 256.0;  // = 0.00390625

/// SPN 91, 92, 96: Percentage signals. 0.4 %/bit.
const PERCENT_SCALE: f64           = 0.4;

/// SPN 100: Oil pressure. 4 kPa/bit.
const OIL_PRESSURE_SCALE: f64      = 4.0;

/// SPN 168: Battery voltage. 0.05 V/bit.
const BATTERY_VOLTAGE_SCALE: f64   = 0.05;

/// SPN 127: Transmission temperature. 0.03125 °C/bit, offset -273 (Kelvin → Celsius).
const TRANS_TEMP_SCALE: f64        = 1.0 / 32.0;
const TRANS_TEMP_OFFSET: f64       = 273.0;

/// SPN 2413: Cat hydraulic pressure. 0.5 kPa/bit (Cat SIS2).
const HYDRAULIC_PRESSURE_SCALE: f64 = 0.5;

/// SPN 1430: Payload mass. 0.5 kg/bit.
const LOAD_WEIGHT_SCALE: f64       = 0.5;

// ─── J1939 PGN constants (for CAN frame decode) ───────────────────────────────

const PGN_EEC1: u32    = 0xF004;   // Electronic Engine Controller 1 (engine RPM)
const PGN_EEC2: u32    = 0xF005;   // Electronic Engine Controller 2 (throttle/load)
const PGN_ET1: u32     = 0xFEEE;   // Engine Temperature 1 (coolant/oil temp)
const PGN_TCI: u32     = 0xF001;   // Transmission Control 1 (trans temp)

// ─── VisionLink JSON field names ──────────────────────────────────────────────

const VL_COOLANT_TEMP: &str        = "engineCoolantTemperature";
const VL_ENGINE_RPM: &str          = "engineSpeed";
const VL_VEHICLE_SPEED: &str       = "groundSpeed";
const VL_FUEL_LEVEL: &str          = "fuelLevel";
const VL_HYDRAULIC_PRESSURE: &str  = "hydraulicSystemPressure";
const VL_TRANS_TEMP: &str          = "transmissionOilTemperature";
const VL_LOAD_WEIGHT: &str         = "payloadWeight";
const VL_ENGINE_LOAD: &str         = "engineLoad";
const VL_BATTERY_VOLTAGE: &str     = "batteryVoltage";
const VL_DEF_LEVEL: &str           = "dieselExhaustFluid";
const VL_OIL_PRESSURE: &str        = "engineOilPressure";

/// Prefix for all Cat fault codes (VisionLink "activeFaultCodes" array).
const CAT_FAULT_PREFIX: &str       = "CAT-";

// ─── CaterpillarAdapter ───────────────────────────────────────────────────────

pub struct CaterpillarAdapter {
    config: TelematicsConfig,
}

impl CaterpillarAdapter {
    pub fn new(config: TelematicsConfig) -> Self {
        Self { config }
    }

    /// Decode a J1939 SPN raw value to its engineering unit.
    fn decode_spn(&self, spn: u32, raw: f64) -> f64 {
        match spn {
            SPN_COOLANT_TEMP       => raw * COOLANT_TEMP_SCALE - COOLANT_TEMP_OFFSET,
            SPN_ENGINE_RPM         => raw * RPM_SCALE,
            SPN_VEHICLE_SPEED      => raw * SPEED_SCALE,
            SPN_THROTTLE_POS       => raw * PERCENT_SCALE,
            SPN_ENGINE_LOAD        => raw * PERCENT_SCALE,
            SPN_FUEL_LEVEL         => raw * PERCENT_SCALE,
            SPN_OIL_PRESSURE       => raw * OIL_PRESSURE_SCALE,
            SPN_BATTERY_VOLTAGE    => raw * BATTERY_VOLTAGE_SCALE,
            SPN_TRANS_TEMP         => raw * TRANS_TEMP_SCALE - TRANS_TEMP_OFFSET,
            SPN_HYDRAULIC_PRESSURE => raw * HYDRAULIC_PRESSURE_SCALE,
            SPN_LOAD_WEIGHT        => raw * LOAD_WEIGHT_SCALE,
            _                      => raw,  // unknown SPN — pass through
        }
    }

    /// Write a decoded SPN value into the correct `SignalMap` field.
    fn apply_spn(&self, spn: u32, decoded: f64, signals: &mut SignalMap) {
        match spn {
            SPN_COOLANT_TEMP       => signals.coolant_temp      = Some(decoded),
            SPN_ENGINE_RPM         => signals.engine_rpm        = Some(decoded),
            SPN_VEHICLE_SPEED      => signals.vehicle_speed     = Some(decoded),
            SPN_THROTTLE_POS       => signals.throttle_position = Some(decoded),
            SPN_ENGINE_LOAD        => signals.engine_load       = Some(decoded),
            SPN_FUEL_LEVEL         => signals.fuel_level        = Some(decoded),
            SPN_OIL_PRESSURE       => signals.oil_pressure      = Some(decoded),
            SPN_BATTERY_VOLTAGE    => signals.battery_voltage   = Some(decoded),
            SPN_TRANS_TEMP         => signals.transmission_temp = Some(decoded),
            SPN_HYDRAULIC_PRESSURE => signals.hydraulic_pressure = Some(decoded),
            SPN_LOAD_WEIGHT        => signals.load_weight       = Some(decoded),
            _                      => { /* unsupported SPN — skip */ }
        }
    }

    /// Decode Cat VisionLink JSON API response.
    fn decode_visionlink_json(&self, json: &serde_json::Value, signals: &mut SignalMap) {
        let f = |key: &str| json.get(key).and_then(|v| v.as_f64());

        if let Some(v) = f(VL_COOLANT_TEMP)       { signals.coolant_temp       = Some(v); }
        if let Some(v) = f(VL_ENGINE_RPM)          { signals.engine_rpm         = Some(v); }
        if let Some(v) = f(VL_VEHICLE_SPEED)       { signals.vehicle_speed      = Some(v); }
        if let Some(v) = f(VL_FUEL_LEVEL)          { signals.fuel_level         = Some(v); }
        if let Some(v) = f(VL_HYDRAULIC_PRESSURE)  { signals.hydraulic_pressure = Some(v); }
        if let Some(v) = f(VL_TRANS_TEMP)          { signals.transmission_temp  = Some(v); }
        if let Some(v) = f(VL_LOAD_WEIGHT)         { signals.load_weight        = Some(v); }
        if let Some(v) = f(VL_ENGINE_LOAD)         { signals.engine_load        = Some(v); }
        if let Some(v) = f(VL_BATTERY_VOLTAGE)     { signals.battery_voltage    = Some(v); }
        if let Some(v) = f(VL_DEF_LEVEL)           { signals.def_level          = Some(v); }
        if let Some(v) = f(VL_OIL_PRESSURE)        { signals.oil_pressure       = Some(v); }

        // Cat VisionLink fault codes: [{"code": "F1234"}, ...] → "CAT-F1234"
        if let Some(arr) = json.get("activeFaultCodes").and_then(|v| v.as_array()) {
            let codes: Vec<String> = arr
                .iter()
                .filter_map(|entry| {
                    entry.get("code")
                        .and_then(|c| c.as_str())
                        .map(|c| format!("{CAT_FAULT_PREFIX}{c}"))
                })
                .collect();
            if !codes.is_empty() {
                signals.dtc_codes = Some(codes);
            }
        }

        // Machine hours go into the `extra` HashMap (not a named SignalMap field)
        if let Some(hours) = f("machineHours") {
            signals.extra.insert(
                "machine_hours".into(),
                serde_json::Value::from(hours),
            );
        }
    }

    /// Decode raw J1939 CAN frames (29-bit extended ID).
    /// Extracts PGN from the 29-bit J1939 arbitration ID: bits 8-25.
    fn decode_j1939_frames(&self, frames: &[CanFrame], signals: &mut SignalMap) {
        for frame in frames {
            // J1939 29-bit ID layout: [priority(3)][R][DP][PGN(16)][SA(8)]
            // PGN occupies bits 8-23 of the 29-bit ID.
            let pgn = (frame.id >> 8) & 0x0001_FFFF;

            match pgn {
                PGN_EEC1 => {
                    // Bytes 3-4 (SPN 190): engine speed, 0.125 RPM/bit
                    if frame.data.len() >= 5 {
                        let rpm_raw = ((frame.data[4] as u16) << 8) | (frame.data[3] as u16);
                        signals.engine_rpm = Some(f64::from(rpm_raw) * RPM_SCALE);
                    }
                }
                PGN_EEC2 => {
                    // Byte 1 (SPN 91): throttle, 0.4 %/bit
                    // Byte 2 (SPN 92): engine load, 0.4 %/bit
                    if frame.data.len() >= 3 {
                        signals.throttle_position = Some(f64::from(frame.data[1]) * PERCENT_SCALE);
                        signals.engine_load       = Some(f64::from(frame.data[2]) * PERCENT_SCALE);
                    }
                }
                PGN_ET1 => {
                    // Byte 0 (SPN 110): coolant temp, offset -40, 1°C/bit (simplified CAN)
                    // Byte 2 (SPN 175): oil temp, same encoding
                    if frame.data.len() >= 3 {
                        signals.coolant_temp = Some(f64::from(frame.data[0]) - COOLANT_TEMP_OFFSET);
                    }
                }
                PGN_TCI => {
                    // Byte 3 (SPN 127): transmission temp, offset -40
                    if frame.data.len() >= 4 {
                        signals.transmission_temp = Some(f64::from(frame.data[3]) - COOLANT_TEMP_OFFSET);
                    }
                }
                _ => { /* unknown PGN — skip */ }
            }
        }
    }
}

impl TelematicsAdapter for CaterpillarAdapter {
    fn source(&self) -> SignalSource {
        SignalSource::CatHeavy
    }

    /// Valid when at least one of: J1939 SPNs, VisionLink JSON, or CAN frames.
    fn validate(&self, frame: &RawTelematicsFrame) -> bool {
        !frame.j1939_spns.is_empty()
            || frame.raw_json.is_some()
            || !frame.can_frames.is_empty()
    }

    fn normalize(&self, frame: &RawTelematicsFrame) -> Result<SignalEvent, AdapterError> {
        if !self.validate(frame) {
            return Err(AdapterError::ValidationFailure(
                "Caterpillar frame has no J1939 SPNs, JSON, or CAN frames".into(),
            ));
        }

        let mut signals = SignalMap::default();

        // Pass 1: J1939 SPN readings (direct telematics gateway)
        for (&spn, &raw) in &frame.j1939_spns {
            let decoded = self.decode_spn(spn, raw);
            self.apply_spn(spn, decoded, &mut signals);
        }

        // Pass 2: VisionLink JSON (overrides SPN values on conflict)
        if let Some(json) = &frame.raw_json {
            self.decode_visionlink_json(json, &mut signals);
        }

        // Pass 3: CAN frames (fill gaps only — do not overwrite already-set values)
        if !frame.can_frames.is_empty() {
            let mut can_signals = SignalMap::default();
            self.decode_j1939_frames(&frame.can_frames, &mut can_signals);

            if signals.engine_rpm.is_none()        { signals.engine_rpm        = can_signals.engine_rpm; }
            if signals.throttle_position.is_none() { signals.throttle_position = can_signals.throttle_position; }
            if signals.engine_load.is_none()       { signals.engine_load       = can_signals.engine_load; }
            if signals.coolant_temp.is_none()      { signals.coolant_temp      = can_signals.coolant_temp; }
            if signals.transmission_temp.is_none() { signals.transmission_temp = can_signals.transmission_temp; }
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
            "oil_pressure",
            "battery_voltage",
            "transmission_temp",
            "hydraulic_pressure",
            "load_weight",
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
            asset_id:      "CAT-320-001".into(),
            driver_id:     "DRV-200".into(),
            vehicle_class: "heavy_duty".into(),
        }
    }

    fn empty_frame() -> RawTelematicsFrame {
        RawTelematicsFrame::default()
    }

    // ── Validate ─────────────────────────────────────────────────────────────

    #[test]
    fn test_cat_validate_empty_frame_returns_false() {
        let adapter = CaterpillarAdapter::new(config());
        assert!(!adapter.validate(&empty_frame()), "Empty frame must fail validation");
    }

    #[test]
    fn test_cat_validate_with_spns_returns_true() {
        let adapter = CaterpillarAdapter::new(config());
        let mut j1939_spns = HashMap::new();
        j1939_spns.insert(110u32, 4128.0);
        let frame = RawTelematicsFrame { j1939_spns, ..Default::default() };
        assert!(adapter.validate(&frame));
    }

    // ── J1939 SPN decoding ────────────────────────────────────────────────────

    #[test]
    fn test_cat_j1939_spn110_coolant_temp() {
        // SPN 110: raw = 4128 → 4128 / 32 − 40 = 129.0 − 40.0 = 89.0°C
        // Using COOLANT_TEMP_SCALE = 1/32 and COOLANT_TEMP_OFFSET = 40
        let adapter = CaterpillarAdapter::new(config());
        let mut j1939_spns = HashMap::new();
        j1939_spns.insert(SPN_COOLANT_TEMP, 4128.0);  // (89 + 40) × 32 = 4128
        let frame = RawTelematicsFrame {
            timestamp: 1_700_000_000_000,
            j1939_spns,
            ..Default::default()
        };
        let event = adapter.normalize(&frame).unwrap();
        let coolant = event.signals.coolant_temp.expect("coolant_temp must be present");
        assert!(
            (coolant - 89.0).abs() < 1.0,
            "Expected ≈89°C, got {coolant}"
        );
    }

    #[test]
    fn test_cat_j1939_spn190_engine_rpm() {
        // SPN 190: raw = 16000 → 16000 × 0.125 = 2000 RPM
        let adapter = CaterpillarAdapter::new(config());
        let mut j1939_spns = HashMap::new();
        j1939_spns.insert(SPN_ENGINE_RPM, 16000.0);
        let frame = RawTelematicsFrame {
            timestamp: 1_700_000_000_000,
            j1939_spns,
            ..Default::default()
        };
        let event = adapter.normalize(&frame).unwrap();
        let rpm = event.signals.engine_rpm.expect("engine_rpm must be present");
        assert!((rpm - 2000.0).abs() < 0.001, "Expected 2000 RPM, got {rpm}");
    }

    #[test]
    fn test_cat_j1939_spn84_vehicle_speed() {
        // SPN 84: raw = 25600 → 25600 / 256 = 100 km/h
        let adapter = CaterpillarAdapter::new(config());
        let mut j1939_spns = HashMap::new();
        j1939_spns.insert(SPN_VEHICLE_SPEED, 25600.0);
        let frame = RawTelematicsFrame {
            timestamp: 1_700_000_000_000,
            j1939_spns,
            ..Default::default()
        };
        let event = adapter.normalize(&frame).unwrap();
        let speed = event.signals.vehicle_speed.expect("vehicle_speed must be present");
        assert!((speed - 100.0).abs() < 0.01, "Expected 100 km/h, got {speed}");
    }

    #[test]
    fn test_cat_j1939_spn2413_hydraulic_pressure() {
        // SPN 2413: raw = 4000 → 4000 × 0.5 = 2000 kPa
        let adapter = CaterpillarAdapter::new(config());
        let mut j1939_spns = HashMap::new();
        j1939_spns.insert(SPN_HYDRAULIC_PRESSURE, 4000.0);
        let frame = RawTelematicsFrame {
            timestamp: 1_700_000_000_000,
            j1939_spns,
            ..Default::default()
        };
        let event = adapter.normalize(&frame).unwrap();
        let pressure = event.signals.hydraulic_pressure.expect("hydraulic_pressure must be present");
        assert!((pressure - 2000.0).abs() < 0.001, "Expected 2000 kPa, got {pressure}");
    }

    #[test]
    fn test_cat_j1939_spn1430_load_weight() {
        // SPN 1430: raw = 60000 → 60000 × 0.5 = 30000 kg
        let adapter = CaterpillarAdapter::new(config());
        let mut j1939_spns = HashMap::new();
        j1939_spns.insert(SPN_LOAD_WEIGHT, 60000.0);
        let frame = RawTelematicsFrame {
            timestamp: 1_700_000_000_000,
            j1939_spns,
            ..Default::default()
        };
        let event = adapter.normalize(&frame).unwrap();
        let weight = event.signals.load_weight.expect("load_weight must be present");
        assert!((weight - 30000.0).abs() < 0.001, "Expected 30000 kg, got {weight}");
    }

    // ── VisionLink JSON decoding ──────────────────────────────────────────────

    #[test]
    fn test_cat_visionlink_coolant_temp() {
        let adapter = CaterpillarAdapter::new(config());
        let frame = RawTelematicsFrame {
            timestamp: 1_700_000_000_000,
            raw_json: Some(serde_json::json!({"engineCoolantTemperature": 95.0})),
            ..Default::default()
        };
        let event = adapter.normalize(&frame).unwrap();
        let temp = event.signals.coolant_temp.expect("coolant_temp must be present");
        assert!((temp - 95.0).abs() < 0.001, "Expected 95.0°C, got {temp}");
    }

    #[test]
    fn test_cat_visionlink_hydraulic_pressure() {
        let adapter = CaterpillarAdapter::new(config());
        let frame = RawTelematicsFrame {
            timestamp: 1_700_000_000_000,
            raw_json: Some(serde_json::json!({"hydraulicSystemPressure": 3500.0})),
            ..Default::default()
        };
        let event = adapter.normalize(&frame).unwrap();
        let pressure = event.signals.hydraulic_pressure.expect("hydraulic_pressure must be present");
        assert!((pressure - 3500.0).abs() < 0.001, "Expected 3500 kPa, got {pressure}");
    }

    #[test]
    fn test_cat_visionlink_fault_codes_prefixed() {
        let adapter = CaterpillarAdapter::new(config());
        let frame = RawTelematicsFrame {
            timestamp: 1_700_000_000_000,
            raw_json: Some(serde_json::json!({
                "engineCoolantTemperature": 90.0,
                "activeFaultCodes": [{"code": "F1234"}, {"code": "E5678"}]
            })),
            ..Default::default()
        };
        let event = adapter.normalize(&frame).unwrap();
        let dtcs = event.signals.dtc_codes.expect("dtc_codes must be present");
        assert_eq!(dtcs, vec!["CAT-F1234", "CAT-E5678"]);
    }

    #[test]
    fn test_cat_source_is_cat_heavy() {
        let adapter = CaterpillarAdapter::new(config());
        let mut j1939_spns = HashMap::new();
        j1939_spns.insert(SPN_ENGINE_RPM, 8000.0);
        let frame = RawTelematicsFrame {
            timestamp: 1_700_000_000_000,
            j1939_spns,
            ..Default::default()
        };
        let event = adapter.normalize(&frame).unwrap();
        assert_eq!(event.source, SignalSource::CatHeavy);
    }
}
