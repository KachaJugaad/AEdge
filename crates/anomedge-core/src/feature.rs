//! feature.rs
//! FeatureEngine — computes a rolling 30-second feature window per asset.
//!
//! Receives `SignalEvent` objects, maintains a per-asset `VecDeque` (O(1) push/pop),
//! and computes the derived features consumed by `InferenceChain`.
//!
//! Performance target: ingest() < 2ms per frame (typical 10Hz feed, 300-sample window).

use std::collections::{HashMap, HashSet, VecDeque};

use crate::types::{FeatureWindow, SignalEvent};

// ─── Constants ────────────────────────────────────────────────────────────────

/// Rolling window duration. All samples older than this are evicted on ingest.
const WINDOW_SECONDS: f64 = 30.0;

/// Brake pedal rising-edge threshold. Crossing from below to at/above counts as
/// one harsh-brake spike.
const BRAKE_SPIKE_THRESHOLD: f64 = 0.8;

/// Maximum consecutive hydraulic-pressure delta (kPa) before `hydraulic_spike`
/// is raised. 500 kPa ≈ 5 bar — significant pressure transient on Cat/JD.
const HYDRAULIC_SPIKE_DELTA_KPA: f64 = 500.0;

/// Transmission fluid temperature above this value (°C) raises `transmission_heat`.
const TRANSMISSION_HEAT_THRESHOLD_C: f64 = 110.0;

// ─── FeatureEngine ────────────────────────────────────────────────────────────

/// Stateful rolling-window feature extractor.
///
/// Maintains one `VecDeque<SignalEvent>` per `asset_id`.
/// Call `ingest()` on every `SignalEvent` published to `signals.raw`;
/// it returns a ready-to-publish `FeatureWindow` for `signals.features`.
pub struct FeatureEngine {
    /// Per-asset sliding window buffer.
    /// `VecDeque` gives O(1) `push_back` / `pop_front`.
    windows: HashMap<String, VecDeque<SignalEvent>>,
}

impl FeatureEngine {
    pub fn new() -> Self {
        Self { windows: HashMap::new() }
    }

    /// Ingest a new signal event and return the updated `FeatureWindow`.
    ///
    /// Complexity: O(n) where n ≤ window size.
    /// Typical n ≤ 300 (10 Hz × 30 s) — always completes well under 2 ms.
    pub fn ingest(&mut self, event: SignalEvent) -> FeatureWindow {
        let asset_id  = event.asset_id.clone();
        let cutoff_ts = event.ts - (WINDOW_SECONDS * 1_000.0) as i64;

        let buf = self.windows.entry(asset_id.clone()).or_default();

        // Evict entries outside the window before pushing the new one.
        while buf.front().map(|e| e.ts < cutoff_ts).unwrap_or(false) {
            buf.pop_front();
        }

        buf.push_back(event);

        // Borrow the complete buffer for the compute step.
        compute_window(buf, &asset_id)
    }

    /// Number of samples currently buffered for `asset_id`.
    /// Used by `InferenceChain` to gate Tier-2 ML (needs ≥ 5 samples).
    pub fn sample_count(&self, asset_id: &str) -> usize {
        self.windows.get(asset_id).map(|b| b.len()).unwrap_or(0)
    }
}

impl Default for FeatureEngine {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Window computation ───────────────────────────────────────────────────────

fn compute_window(buf: &VecDeque<SignalEvent>, asset_id: &str) -> FeatureWindow {
    // Safety: buffer always contains at least the event just pushed.
    let latest = buf.back().expect("buffer non-empty after push");

    // ── Time-series extraction ─────────────────────────────────────────────
    let coolant_vals:   Vec<f64> = buf.iter().filter_map(|e| e.signals.coolant_temp).collect();
    let speed_vals:     Vec<f64> = buf.iter().filter_map(|e| e.signals.vehicle_speed).collect();
    let rpm_vals:       Vec<f64> = buf.iter().filter_map(|e| e.signals.engine_rpm).collect();
    let load_vals:      Vec<f64> = buf.iter().filter_map(|e| e.signals.engine_load).collect();
    let throttle_vals:  Vec<f64> = buf.iter().filter_map(|e| e.signals.throttle_position).collect();
    let brake_vals:     Vec<f64> = buf.iter().filter_map(|e| e.signals.brake_pedal).collect();
    let hydraulic_vals: Vec<f64> = buf.iter().filter_map(|e| e.signals.hydraulic_pressure).collect();
    let trans_vals:     Vec<f64> = buf.iter().filter_map(|e| e.signals.transmission_temp).collect();

    // ── DTC delta ──────────────────────────────────────────────────────────
    // Codes present in any event *before* the latest one.
    let prior_dtcs: HashSet<&str> = buf.iter()
        .take(buf.len().saturating_sub(1))
        .flat_map(|e| e.signals.dtc_codes.iter().flatten())
        .map(|s| s.as_str())
        .collect();

    // Codes in the latest event that are genuinely new.
    let dtc_new: Vec<String> = latest.signals.dtc_codes
        .iter()
        .flatten()
        .filter(|c| !prior_dtcs.contains(c.as_str()))
        .cloned()
        .collect();

    // ── Hydraulic spike ────────────────────────────────────────────────────
    // Fires if any consecutive delta exceeds the threshold.
    let hydraulic_spike = hydraulic_vals.len() > 1
        && max_delta(&hydraulic_vals) > HYDRAULIC_SPIKE_DELTA_KPA;

    // ── Transmission heat ─────────────────────────────────────────────────
    // Fires if any sample in the window exceeded the temp threshold.
    let transmission_heat = trans_vals.iter().any(|&t| t > TRANSMISSION_HEAT_THRESHOLD_C);

    FeatureWindow {
        ts:                latest.ts,
        asset_id:          asset_id.to_string(),
        window_seconds:    WINDOW_SECONDS,
        coolant_slope:     linear_slope(&coolant_vals),
        brake_spike_count: count_spikes(&brake_vals, BRAKE_SPIKE_THRESHOLD),
        speed_mean:        mean(&speed_vals),
        rpm_mean:          mean(&rpm_vals),
        engine_load_mean:  mean(&load_vals),
        throttle_variance: variance(&throttle_vals),
        hydraulic_spike,
        transmission_heat,
        dtc_new,
        signals_snapshot:  latest.signals.clone(),
    }
}

// ─── Math helpers ─────────────────────────────────────────────────────────────

/// Arithmetic mean. Returns 0.0 for an empty slice.
fn mean(vals: &[f64]) -> f64 {
    if vals.is_empty() { return 0.0; }
    vals.iter().sum::<f64>() / vals.len() as f64
}

/// Population variance. Returns 0.0 for fewer than 2 values.
fn variance(vals: &[f64]) -> f64 {
    if vals.len() < 2 { return 0.0; }
    let m = mean(vals);
    mean(&vals.iter().map(|v| (v - m).powi(2)).collect::<Vec<_>>())
}

/// Simple linear slope: (last − first) / count.
/// Returns 0.0 for fewer than 2 values.
/// Matches the TypeScript reference: `(last - first) / arr.length`.
fn linear_slope(vals: &[f64]) -> f64 {
    if vals.len() < 2 { return 0.0; }
    (vals[vals.len() - 1] - vals[0]) / vals.len() as f64
}

/// Count rising-edge spikes: transitions where the value crosses `threshold`
/// upward (previous sample < threshold, current sample >= threshold).
fn count_spikes(vals: &[f64], threshold: f64) -> f64 {
    vals.windows(2)
        .filter(|w| w[0] < threshold && w[1] >= threshold)
        .count() as f64
}

/// Maximum absolute difference between any two consecutive values.
fn max_delta(vals: &[f64]) -> f64 {
    vals.windows(2)
        .map(|w| (w[1] - w[0]).abs())
        .fold(0.0_f64, f64::max)
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{SignalMap, SignalSource};

    // ── Helpers ───────────────────────────────────────────────────────────────

    fn make_event(ts_ms: i64, signals: SignalMap) -> SignalEvent {
        SignalEvent {
            ts:        ts_ms,
            asset_id:  "TRUCK-001".into(),
            driver_id: "DRV-001".into(),
            source:    SignalSource::Obd2Generic,
            signals,
            raw_frame: None,
        }
    }

    fn coolant_event(ts_ms: i64, temp: f64) -> SignalEvent {
        make_event(ts_ms, SignalMap { coolant_temp: Some(temp), ..Default::default() })
    }

    fn brake_event(ts_ms: i64, pedal: f64) -> SignalEvent {
        make_event(ts_ms, SignalMap { brake_pedal: Some(pedal), ..Default::default() })
    }

    // ── Test 1: single event → slopes are 0, means use the one value ──────────

    #[test]
    fn test_single_event_returns_zero_slopes() {
        let mut engine = FeatureEngine::new();
        let event = coolant_event(0, 85.0);
        let window = engine.ingest(event);

        assert_eq!(window.coolant_slope, 0.0, "slope must be 0 with one sample");
        assert_eq!(window.brake_spike_count, 0.0);
        assert_eq!(window.throttle_variance, 0.0);
    }

    // ── Test 2: rising coolant over 10 events → positive slope ────────────────

    #[test]
    fn test_rising_coolant_slope_is_positive() {
        let mut engine = FeatureEngine::new();
        // 10 events: coolant_temp goes from 85 → 94 (step 1°C, 1-second apart)
        for i in 0..10_i64 {
            engine.ingest(coolant_event(i * 1_000, 85.0 + i as f64));
        }
        let window = engine.ingest(coolant_event(10_000, 95.0));

        assert!(
            window.coolant_slope > 0.0,
            "rising coolant must produce positive slope, got {}",
            window.coolant_slope
        );
    }

    // ── Test 3: brake spike counting ──────────────────────────────────────────

    #[test]
    fn test_brake_spike_count_three_events() {
        let mut engine = FeatureEngine::new();
        // Three brake-spike sequences: 0.2 → 0.9  (spike), 0.3 → 1.0 (spike), 0.1 → 0.85 (spike)
        let pedal_sequence = [0.2, 0.9, 0.2, 0.3, 1.0, 0.2, 0.1, 0.85, 0.0];
        for (i, &p) in pedal_sequence.iter().enumerate() {
            engine.ingest(brake_event(i as i64 * 1_000, p));
        }
        let window = engine.ingest(brake_event(9_000, 0.0));

        assert_eq!(
            window.brake_spike_count, 3.0,
            "expected 3 brake spikes, got {}",
            window.brake_spike_count
        );
    }

    // ── Test 4: hydraulic spike fires on > 500 kPa delta ─────────────────────

    #[test]
    fn test_hydraulic_spike_fires_on_large_delta() {
        let mut engine = FeatureEngine::new();
        // Normal pressure then a sudden spike
        engine.ingest(make_event(0, SignalMap { hydraulic_pressure: Some(200.0), ..Default::default() }));
        let window = engine.ingest(make_event(1_000, SignalMap { hydraulic_pressure: Some(750.0), ..Default::default() }));

        assert!(window.hydraulic_spike, "delta of 550 kPa must trigger hydraulic_spike");
    }

    #[test]
    fn test_hydraulic_spike_does_not_fire_on_small_delta() {
        let mut engine = FeatureEngine::new();
        engine.ingest(make_event(0, SignalMap { hydraulic_pressure: Some(200.0), ..Default::default() }));
        let window = engine.ingest(make_event(1_000, SignalMap { hydraulic_pressure: Some(600.0), ..Default::default() }));

        // 600 - 200 = 400 kPa, below 500 threshold
        assert!(!window.hydraulic_spike, "delta of 400 kPa must NOT trigger hydraulic_spike");
    }

    // ── Test 5: transmission_heat fires on temp > 110°C ───────────────────────

    #[test]
    fn test_transmission_heat_fires_above_threshold() {
        let mut engine = FeatureEngine::new();
        engine.ingest(make_event(0, SignalMap { transmission_temp: Some(90.0), ..Default::default() }));
        let window = engine.ingest(make_event(1_000, SignalMap { transmission_temp: Some(115.0), ..Default::default() }));

        assert!(window.transmission_heat, "115°C must raise transmission_heat");
    }

    #[test]
    fn test_transmission_heat_does_not_fire_below_threshold() {
        let mut engine = FeatureEngine::new();
        engine.ingest(make_event(0, SignalMap { transmission_temp: Some(90.0), ..Default::default() }));
        let window = engine.ingest(make_event(1_000, SignalMap { transmission_temp: Some(108.0), ..Default::default() }));

        assert!(!window.transmission_heat, "108°C must NOT raise transmission_heat");
    }

    // ── Test 6: dtc_new isolates only new codes ───────────────────────────────

    #[test]
    fn test_dtc_new_isolates_only_new_codes() {
        let mut engine = FeatureEngine::new();

        // First event: two codes
        engine.ingest(make_event(0, SignalMap {
            dtc_codes: Some(vec!["P0300".into(), "P0301".into()]),
            ..Default::default()
        }));

        // Second event: P0300 already seen, P0420 is new
        let window = engine.ingest(make_event(1_000, SignalMap {
            dtc_codes: Some(vec!["P0300".into(), "P0420".into()]),
            ..Default::default()
        }));

        assert_eq!(window.dtc_new, vec!["P0420".to_string()],
            "only P0420 should appear as a new code");
    }

    #[test]
    fn test_dtc_new_empty_when_no_new_codes() {
        let mut engine = FeatureEngine::new();
        engine.ingest(make_event(0, SignalMap {
            dtc_codes: Some(vec!["P0300".into()]),
            ..Default::default()
        }));
        let window = engine.ingest(make_event(1_000, SignalMap {
            dtc_codes: Some(vec!["P0300".into()]),
            ..Default::default()
        }));

        assert!(window.dtc_new.is_empty(), "no new codes expected");
    }

    // ── Test 7: window trimming ───────────────────────────────────────────────

    #[test]
    fn test_old_events_are_evicted_from_window() {
        let mut engine = FeatureEngine::new();

        // Push an event 35 seconds ago (outside 30s window)
        engine.ingest(coolant_event(0, 200.0)); // would produce very high slope if kept
        // Push a current event at t=35s
        let window = engine.ingest(coolant_event(35_000, 85.0));

        // Only 1 event remains; slope must be 0 (can't compute slope with 1 point)
        assert_eq!(window.coolant_slope, 0.0,
            "evicted events must not influence features");
        assert_eq!(engine.sample_count("TRUCK-001"), 1);
    }

    // ── Test 8: statistical feature correctness ───────────────────────────────

    #[test]
    fn test_speed_mean_rpm_mean_computed_correctly() {
        let mut engine = FeatureEngine::new();
        for (i, speed) in [60.0_f64, 80.0, 100.0].iter().enumerate() {
            engine.ingest(make_event(
                i as i64 * 1_000,
                SignalMap {
                    vehicle_speed: Some(*speed),
                    engine_rpm:    Some(2000.0 + i as f64 * 100.0),
                    ..Default::default()
                },
            ));
        }
        let window = engine.ingest(make_event(3_000, SignalMap {
            vehicle_speed: Some(80.0),
            engine_rpm:    Some(2200.0),
            ..Default::default()
        }));

        // 4 speed values: 60, 80, 100, 80 → mean = 320/4 = 80
        assert!((window.speed_mean - 80.0).abs() < 0.001,
            "speed_mean expected 80.0, got {}", window.speed_mean);
    }

    // ── Test 9: independent asset windows ────────────────────────────────────

    #[test]
    fn test_separate_assets_have_independent_windows() {
        let mut engine = FeatureEngine::new();

        let mut e1 = coolant_event(0, 90.0);
        e1.asset_id = "TRUCK-001".into();
        let mut e2 = coolant_event(0, 105.0);
        e2.asset_id = "TRUCK-002".into();

        engine.ingest(e1);
        let w2 = engine.ingest(e2);

        assert_eq!(w2.asset_id, "TRUCK-002");
        assert_eq!(w2.signals_snapshot.coolant_temp, Some(105.0));
        assert_eq!(engine.sample_count("TRUCK-001"), 1);
        assert_eq!(engine.sample_count("TRUCK-002"), 1);
    }
}
