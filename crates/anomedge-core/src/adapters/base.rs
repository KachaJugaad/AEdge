//! adapters/base.rs
//! Shared types, traits, and error definitions for all telematics adapters.
//! Every adapter module depends on this — implement first.

use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::types::{SignalEvent, SignalSource};

// ─── AdapterError ─────────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum AdapterError {
    #[error("Malformed frame: {0}")]
    MalformedFrame(String),

    #[error("Unknown PID: {0}")]
    UnknownPid(String),

    #[error("Unknown SPN: {0}")]
    UnknownSpn(u32),

    #[error("Validation failure: {0}")]
    ValidationFailure(String),

    #[error("JSON decode error: {0}")]
    JsonError(#[from] serde_json::Error),
}

// ─── TelematicsConfig ─────────────────────────────────────────────────────────

/// Per-asset configuration passed to every adapter at construction time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelematicsConfig {
    /// Vehicle / asset identifier, e.g. "TRUCK-001"
    pub asset_id: String,

    /// Driver identifier, e.g. "DRV-042". Empty string when unknown.
    pub driver_id: String,

    /// High-level vehicle class: "light_duty", "heavy_duty", "construction", "agricultural"
    pub vehicle_class: String,
}

// ─── CanFrame ─────────────────────────────────────────────────────────────────

/// A single raw CAN bus frame.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanFrame {
    /// 11-bit or 29-bit CAN ID
    pub id: u32,

    /// Up to 8 payload bytes
    pub data: Vec<u8>,

    /// Capture timestamp (Unix ms)
    pub ts: u64,
}

// ─── RawTelematicsFrame ───────────────────────────────────────────────────────

/// The raw input that any vehicle source delivers before normalization.
/// Fields are all optional — an adapter only populates what its source provides.
#[derive(Debug, Clone, Default)]
pub struct RawTelematicsFrame {
    /// Capture timestamp in Unix milliseconds. Defaults to 0 when absent.
    pub timestamp: u64,

    /// OBD-II PID readings: key = PID string e.g. "0105", value = raw numeric value.
    pub pid_readings: HashMap<String, f64>,

    /// J1939 SPN readings: key = SPN number, value = raw numeric value.
    pub j1939_spns: HashMap<u32, f64>,

    /// Raw CAN frames (direct bus tap).
    pub can_frames: Vec<CanFrame>,

    /// FordPass / VisionLink / JDLink JSON payload when available.
    pub raw_json: Option<serde_json::Value>,
}

// ─── TelematicsAdapter trait ──────────────────────────────────────────────────

/// The core adapter contract. Implement this for every vehicle type.
///
/// # Threading
/// Adapters are `Send + Sync` so they can be shared across threads in a fleet
/// pipeline without cloning.
pub trait TelematicsAdapter: Send + Sync {
    /// Map a raw telematics frame to a normalised `SignalEvent`.
    ///
    /// Returns `Err(AdapterError)` if the frame is malformed or fails validation.
    /// Never panics.
    fn normalize(&self, frame: &RawTelematicsFrame) -> Result<SignalEvent, AdapterError>;

    /// Source identifier that will be embedded in every `SignalEvent`.
    fn source(&self) -> SignalSource;

    /// Returns `true` when the frame contains enough data for normalization.
    /// An empty frame (no PIDs, no SPNs, no JSON) must return `false`.
    fn validate(&self, frame: &RawTelematicsFrame) -> bool;

    /// PID / SPN / field names this adapter can decode.
    fn supported_signals(&self) -> Vec<&'static str>;
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

/// Clamp a value to [min, max].
#[inline]
pub fn clamp(val: f64, min: f64, max: f64) -> f64 {
    val.max(min).min(max)
}

/// Return the current time as Unix milliseconds.
#[inline]
pub fn now_millis() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clamp_below_min() {
        assert!((clamp(-5.0, 0.0, 100.0) - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_clamp_above_max() {
        assert!((clamp(200.0, 0.0, 100.0) - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_clamp_within_range() {
        assert!((clamp(50.0, 0.0, 100.0) - 50.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_raw_telematics_frame_default_is_empty() {
        let frame = RawTelematicsFrame::default();
        assert!(frame.pid_readings.is_empty());
        assert!(frame.j1939_spns.is_empty());
        assert!(frame.can_frames.is_empty());
        assert!(frame.raw_json.is_none());
    }

    #[test]
    fn test_adapter_error_display() {
        let e = AdapterError::MalformedFrame("bad CRC".into());
        assert!(e.to_string().contains("bad CRC"));
    }
}
