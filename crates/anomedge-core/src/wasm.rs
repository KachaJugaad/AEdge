//! wasm.rs
//! WebAssembly exports via wasm-bindgen for browser use.
//!
//! Compiled with: wasm-pack build --target web
//! Loaded by: Flutter web, React dashboard, or any JS/TS consumer.
//!
//! API mirrors ffi.rs but uses String in/out instead of C pointers:
//!   WasmPipeline::new(policy_yaml) → WasmPipeline
//!   WasmPipeline::process(event_json) → decisions_json
//!   WasmPipeline::process_batch(events_json) → results_json
//!   anomedge_version_wasm() → json

#[cfg(feature = "wasm")]
use wasm_bindgen::prelude::*;

use crate::pipeline::Pipeline;
use crate::types::SignalEvent;

// ─── WasmPipeline ────────────────────────────────────────────────────────────

#[cfg_attr(feature = "wasm", wasm_bindgen)]
pub struct WasmPipeline {
    inner: Pipeline,
}

#[cfg_attr(feature = "wasm", wasm_bindgen)]
impl WasmPipeline {
    /// Create a new pipeline from policy YAML string.
    ///
    /// Throws a JS error if the YAML is invalid.
    #[cfg_attr(feature = "wasm", wasm_bindgen(constructor))]
    pub fn new(policy_yaml: &str) -> Result<WasmPipeline, String> {
        let pipeline = Pipeline::from_yaml(policy_yaml)
            .map_err(|e| format!("Invalid policy YAML: {e}"))?;
        Ok(WasmPipeline { inner: pipeline })
    }

    /// Process a single SignalEvent (JSON string).
    ///
    /// Returns JSON array of gated decisions.
    /// Throws a JS error if the event JSON is invalid.
    pub fn process(&mut self, event_json: &str) -> Result<String, String> {
        let event: SignalEvent = serde_json::from_str(event_json)
            .map_err(|e| format!("Invalid event JSON: {e}"))?;

        let result = self.inner.process(event);

        serde_json::to_string(&result.gated_decisions)
            .map_err(|e| format!("Serialization error: {e}"))
    }

    /// Process a batch of SignalEvent objects (JSON array string).
    ///
    /// Returns JSON array of arrays of gated decisions.
    pub fn process_batch(&mut self, events_json: &str) -> Result<String, String> {
        let events: Vec<SignalEvent> = serde_json::from_str(events_json)
            .map_err(|e| format!("Invalid events JSON: {e}"))?;

        let results = self.inner.process_batch(events);
        let gated: Vec<_> = results.iter().map(|r| &r.gated_decisions).collect();

        serde_json::to_string(&gated)
            .map_err(|e| format!("Serialization error: {e}"))
    }

    /// Returns version info as JSON.
    pub fn version(&self) -> String {
        serde_json::json!({
            "name":    "anomedge-core",
            "version": env!("CARGO_PKG_VERSION"),
            "target":  "wasm32",
            "phase":   "0",
            "tiers":   ["rule_engine"],
            "tiers_pending": ["edge_ai", "ml_statistical"],
        }).to_string()
    }
}

// ─── Standalone function (no pipeline needed) ────────────────────────────────

#[cfg_attr(feature = "wasm", wasm_bindgen)]
pub fn anomedge_version_wasm() -> String {
    serde_json::json!({
        "name":    "anomedge-core",
        "version": env!("CARGO_PKG_VERSION"),
        "target":  "wasm32",
        "phase":   "0",
    }).to_string()
}

// ─── Tests (run on native, not wasm) ─────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_POLICY: &str = r#"
version: "test-1.0"
vehicle_class: SIMULATOR
rules:
  - id: coolant_high
    group: thermal
    signal: signals_snapshot.coolant_temp
    operator: gt
    threshold: 100.0
    severity: HIGH
    cooldown_ms: 0
    hysteresis: 0.0
    description: "coolant high"
"#;

    fn make_event_json(ts: i64, coolant_temp: f64) -> String {
        serde_json::json!({
            "ts": ts,
            "asset_id": "TRUCK-001",
            "driver_id": "DRV-001",
            "source": "SIMULATOR",
            "signals": { "coolant_temp": coolant_temp }
        }).to_string()
    }

    // ── Test 1: construct pipeline ──────────────────────────────────────────

    #[test]
    fn test_wasm_pipeline_construct() {
        let p = WasmPipeline::new(TEST_POLICY);
        assert!(p.is_ok(), "valid YAML must construct");
    }

    // ── Test 2: bad YAML returns Err ────────────────────────────────────────

    #[test]
    fn test_wasm_pipeline_bad_yaml() {
        let p = WasmPipeline::new("not: valid: [[[");
        assert!(p.is_err());
    }

    // ── Test 3: process — no alert below threshold ──────────────────────────

    #[test]
    fn test_wasm_process_no_alert() {
        let mut p = WasmPipeline::new(TEST_POLICY).unwrap();
        let result = p.process(&make_event_json(1_000, 85.0)).unwrap();
        let decisions: Vec<serde_json::Value> = serde_json::from_str(&result).unwrap();
        assert!(decisions.is_empty());
    }

    // ── Test 4: process — alert above threshold ─────────────────────────────

    #[test]
    fn test_wasm_process_alert() {
        let mut p = WasmPipeline::new(TEST_POLICY).unwrap();
        let result = p.process(&make_event_json(1_000, 115.0)).unwrap();
        let decisions: Vec<serde_json::Value> = serde_json::from_str(&result).unwrap();
        assert_eq!(decisions.len(), 1);
        assert_eq!(decisions[0]["rule_id"], "coolant_high");
    }

    // ── Test 5: process bad JSON returns Err ────────────────────────────────

    #[test]
    fn test_wasm_process_bad_json() {
        let mut p = WasmPipeline::new(TEST_POLICY).unwrap();
        let result = p.process("not json");
        assert!(result.is_err());
    }

    // ── Test 6: process_batch ───────────────────────────────────────────────

    #[test]
    fn test_wasm_process_batch() {
        let mut p = WasmPipeline::new(TEST_POLICY).unwrap();
        let batch = serde_json::json!([
            { "ts": 1000, "asset_id": "T-1", "driver_id": "D-1",
              "source": "SIMULATOR", "signals": { "coolant_temp": 85.0 } },
            { "ts": 2000, "asset_id": "T-1", "driver_id": "D-1",
              "source": "SIMULATOR", "signals": { "coolant_temp": 115.0 } },
        ]).to_string();

        let result = p.process_batch(&batch).unwrap();
        let results: Vec<Vec<serde_json::Value>> = serde_json::from_str(&result).unwrap();
        assert_eq!(results.len(), 2);
    }

    // ── Test 7: version returns valid JSON ──────────────────────────────────

    #[test]
    fn test_wasm_version() {
        let p = WasmPipeline::new(TEST_POLICY).unwrap();
        let v: serde_json::Value = serde_json::from_str(&p.version()).unwrap();
        assert_eq!(v["name"], "anomedge-core");
        assert_eq!(v["target"], "wasm32");
    }

    // ── Test 8: standalone version function ─────────────────────────────────

    #[test]
    fn test_standalone_version() {
        let v: serde_json::Value = serde_json::from_str(&anomedge_version_wasm()).unwrap();
        assert_eq!(v["name"], "anomedge-core");
    }
}
