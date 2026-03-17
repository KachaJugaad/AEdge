//! ffi.rs
//! C-compatible FFI exports for Flutter (dart:ffi) and other native callers.
//!
//! All functions use C strings (null-terminated UTF-8) for JSON in/out.
//! The pipeline handle is a boxed pointer stored on the heap.
//!
//! Flutter (Dart) loads this as:
//!   Android: libanomedge_core.so
//!   iOS:     libanomedge_core.dylib
//!
//! API:
//!   pipeline_create(policy_yaml_ptr) → handle (i64), 0 = error
//!   pipeline_process(handle, event_json_ptr) → json_ptr (must free with anomedge_free_string)
//!   pipeline_process_batch(handle, events_json_ptr) → json_ptr
//!   pipeline_destroy(handle) → void
//!   anomedge_free_string(ptr) → void
//!   anomedge_version() → json_ptr (must free)

use std::ffi::{CStr, CString};
use std::os::raw::c_char;

use crate::pipeline::Pipeline;
use crate::types::SignalEvent;

// ─── Pipeline handle management ─────────────────────────────────────────────

/// Create a new pipeline from a policy YAML string.
///
/// Returns an opaque handle (heap pointer as i64). Returns 0 on error.
/// The caller must eventually call `pipeline_destroy` to free the handle.
///
/// # Safety
/// `policy_yaml_ptr` must be a valid null-terminated UTF-8 C string.
#[no_mangle]
pub unsafe extern "C" fn pipeline_create(policy_yaml_ptr: *const c_char) -> i64 {
    if policy_yaml_ptr.is_null() {
        return 0;
    }

    let yaml = match CStr::from_ptr(policy_yaml_ptr).to_str() {
        Ok(s) => s,
        Err(_) => return 0,
    };

    match Pipeline::from_yaml(yaml) {
        Ok(pipeline) => {
            let boxed = Box::new(pipeline);
            Box::into_raw(boxed) as i64
        }
        Err(_) => 0,
    }
}

/// Process a single SignalEvent JSON through the pipeline.
///
/// Returns a JSON string containing the gated decisions array.
/// The caller MUST free the returned string with `anomedge_free_string`.
/// Returns null on error.
///
/// # Safety
/// - `handle` must be a valid handle from `pipeline_create`.
/// - `event_json_ptr` must be a valid null-terminated UTF-8 C string.
#[no_mangle]
pub unsafe extern "C" fn pipeline_process(
    handle: i64,
    event_json_ptr: *const c_char,
) -> *mut c_char {
    if handle == 0 || event_json_ptr.is_null() {
        return std::ptr::null_mut();
    }

    let pipeline = &mut *(handle as *mut Pipeline);

    let json_str = match CStr::from_ptr(event_json_ptr).to_str() {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };

    let event: SignalEvent = match serde_json::from_str(json_str) {
        Ok(e) => e,
        Err(_) => return std::ptr::null_mut(),
    };

    let result = pipeline.process(event);

    // Return only gated_decisions — this is what Person B needs
    let response = match serde_json::to_string(&result.gated_decisions) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };

    match CString::new(response) {
        Ok(cs) => cs.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Process a batch of SignalEvent JSON objects through the pipeline.
///
/// Input: JSON array of SignalEvent objects.
/// Output: JSON array of arrays of gated decisions (one inner array per event).
/// The caller MUST free the returned string with `anomedge_free_string`.
///
/// # Safety
/// - `handle` must be a valid handle from `pipeline_create`.
/// - `events_json_ptr` must be a valid null-terminated UTF-8 C string.
#[no_mangle]
pub unsafe extern "C" fn pipeline_process_batch(
    handle: i64,
    events_json_ptr: *const c_char,
) -> *mut c_char {
    if handle == 0 || events_json_ptr.is_null() {
        return std::ptr::null_mut();
    }

    let pipeline = &mut *(handle as *mut Pipeline);

    let json_str = match CStr::from_ptr(events_json_ptr).to_str() {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };

    let events: Vec<SignalEvent> = match serde_json::from_str(json_str) {
        Ok(e) => e,
        Err(_) => return std::ptr::null_mut(),
    };

    let results = pipeline.process_batch(events);
    let gated: Vec<_> = results.iter().map(|r| &r.gated_decisions).collect();

    let response = match serde_json::to_string(&gated) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };

    match CString::new(response) {
        Ok(cs) => cs.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Destroy a pipeline handle and free its memory.
///
/// # Safety
/// `handle` must be a valid handle from `pipeline_create`, and must not
/// be used again after this call.
#[no_mangle]
pub unsafe extern "C" fn pipeline_destroy(handle: i64) {
    if handle != 0 {
        let _ = Box::from_raw(handle as *mut Pipeline);
    }
}

/// Free a string previously returned by `pipeline_process` or other FFI functions.
///
/// # Safety
/// `ptr` must be a pointer returned by a function in this module, or null.
#[no_mangle]
pub unsafe extern "C" fn anomedge_free_string(ptr: *mut c_char) {
    if !ptr.is_null() {
        let _ = CString::from_raw(ptr);
    }
}

/// Returns version and build info as a JSON string.
/// The caller MUST free the returned string with `anomedge_free_string`.
#[no_mangle]
pub extern "C" fn anomedge_version() -> *mut c_char {
    let info = serde_json::json!({
        "name":    "anomedge-core",
        "version": env!("CARGO_PKG_VERSION"),
        "phase":   "0",
        "tiers":   ["rule_engine"],
        "tiers_pending": ["edge_ai", "ml_statistical"],
    });

    match CString::new(info.to_string()) {
        Ok(cs) => cs.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

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
            "signals": {
                "coolant_temp": coolant_temp
            }
        }).to_string()
    }

    // ── Test 1: create and destroy pipeline ──────────────────────────────────

    #[test]
    fn test_pipeline_create_destroy() {
        let yaml = CString::new(TEST_POLICY).unwrap();
        unsafe {
            let handle = pipeline_create(yaml.as_ptr());
            assert_ne!(handle, 0, "pipeline_create must return valid handle");
            pipeline_destroy(handle);
        }
    }

    // ── Test 2: create with bad YAML returns 0 ──────────────────────────────

    #[test]
    fn test_pipeline_create_bad_yaml() {
        let yaml = CString::new("not: valid: yaml: [[[").unwrap();
        unsafe {
            let handle = pipeline_create(yaml.as_ptr());
            assert_eq!(handle, 0, "bad YAML must return 0");
        }
    }

    // ── Test 3: create with null pointer returns 0 ──────────────────────────

    #[test]
    fn test_pipeline_create_null_ptr() {
        unsafe {
            let handle = pipeline_create(std::ptr::null());
            assert_eq!(handle, 0, "null pointer must return 0");
        }
    }

    // ── Test 4: process single event — no alert below threshold ─────────────

    #[test]
    fn test_process_no_alert_below_threshold() {
        let yaml = CString::new(TEST_POLICY).unwrap();
        unsafe {
            let handle = pipeline_create(yaml.as_ptr());
            assert_ne!(handle, 0);

            let event = CString::new(make_event_json(1_000, 85.0)).unwrap();
            let result_ptr = pipeline_process(handle, event.as_ptr());
            assert!(!result_ptr.is_null());

            let result_str = CStr::from_ptr(result_ptr).to_str().unwrap();
            let decisions: Vec<serde_json::Value> = serde_json::from_str(result_str).unwrap();
            assert!(decisions.is_empty(), "85°C < 100°C threshold → no alert");

            anomedge_free_string(result_ptr);
            pipeline_destroy(handle);
        }
    }

    // ── Test 5: process single event — alert above threshold ────────────────

    #[test]
    fn test_process_alert_above_threshold() {
        let yaml = CString::new(TEST_POLICY).unwrap();
        unsafe {
            let handle = pipeline_create(yaml.as_ptr());

            let event = CString::new(make_event_json(1_000, 115.0)).unwrap();
            let result_ptr = pipeline_process(handle, event.as_ptr());
            assert!(!result_ptr.is_null());

            let result_str = CStr::from_ptr(result_ptr).to_str().unwrap();
            let decisions: Vec<serde_json::Value> = serde_json::from_str(result_str).unwrap();
            assert_eq!(decisions.len(), 1, "115°C > 100°C → must fire");
            assert_eq!(decisions[0]["rule_id"], "coolant_high");

            anomedge_free_string(result_ptr);
            pipeline_destroy(handle);
        }
    }

    // ── Test 6: process with null handle returns null ────────────────────────

    #[test]
    fn test_process_null_handle() {
        let event = CString::new(make_event_json(1_000, 85.0)).unwrap();
        unsafe {
            let result = pipeline_process(0, event.as_ptr());
            assert!(result.is_null(), "null handle must return null");
        }
    }

    // ── Test 7: process with bad JSON returns null ──────────────────────────

    #[test]
    fn test_process_bad_json() {
        let yaml = CString::new(TEST_POLICY).unwrap();
        unsafe {
            let handle = pipeline_create(yaml.as_ptr());
            let bad = CString::new("not json").unwrap();
            let result = pipeline_process(handle, bad.as_ptr());
            assert!(result.is_null(), "bad JSON must return null");
            pipeline_destroy(handle);
        }
    }

    // ── Test 8: process batch ───────────────────────────────────────────────

    #[test]
    fn test_process_batch() {
        let yaml = CString::new(TEST_POLICY).unwrap();
        unsafe {
            let handle = pipeline_create(yaml.as_ptr());

            let batch = serde_json::json!([
                { "ts": 1000, "asset_id": "T-1", "driver_id": "D-1",
                  "source": "SIMULATOR", "signals": { "coolant_temp": 85.0 } },
                { "ts": 2000, "asset_id": "T-1", "driver_id": "D-1",
                  "source": "SIMULATOR", "signals": { "coolant_temp": 115.0 } },
            ]).to_string();

            let batch_c = CString::new(batch).unwrap();
            let result_ptr = pipeline_process_batch(handle, batch_c.as_ptr());
            assert!(!result_ptr.is_null());

            let result_str = CStr::from_ptr(result_ptr).to_str().unwrap();
            let results: Vec<Vec<serde_json::Value>> = serde_json::from_str(result_str).unwrap();
            assert_eq!(results.len(), 2, "batch of 2 events → 2 result arrays");

            anomedge_free_string(result_ptr);
            pipeline_destroy(handle);
        }
    }

    // ── Test 9: anomedge_version returns valid JSON ─────────────────────────

    #[test]
    fn test_version() {
        let ptr = anomedge_version();
        assert!(!ptr.is_null());
        unsafe {
            let s = CStr::from_ptr(ptr).to_str().unwrap();
            let v: serde_json::Value = serde_json::from_str(s).unwrap();
            assert_eq!(v["name"], "anomedge-core");
            anomedge_free_string(ptr);
        }
    }

    // ── Test 10: free null is safe ──────────────────────────────────────────

    #[test]
    fn test_free_null_is_safe() {
        unsafe {
            anomedge_free_string(std::ptr::null_mut());
            // no crash = pass
        }
    }
}
