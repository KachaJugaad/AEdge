# AnomEdge — Next Steps: Architecture Decisions & Why

> **For:** All three engineers + anyone reading this cold  
> **Written by:** Person A perspective  
> **Date:** March 2026 | Vancouver BC  
> **Status:** Decision document — read before writing a single line of code

---

## The 5 Questions We're Answering Here

1. Which language runs fast on every platform without draining battery?
2. How does Edge AI fall back to ML, then to rule-based, automatically?
3. How do Persons B and C read what Person A produces — exactly?
4. What is the complete pub/sub event flow from signal to operator alert?
5. How do we shrink models to INT8 for edge without losing accuracy?

---

## Decision 1: Language Architecture — Why Rust Core + Flutter Shell

### The Problem With A Single Language

| Language | Web | Android | iOS | IoT | Battery | Speed | Verdict |
|----------|-----|---------|-----|-----|---------|-------|---------|
| TypeScript/Node | ✅ | ⚠️ via RN | ⚠️ via RN | ❌ | ❌ GC pauses | ❌ JIT | Too slow for inference |
| Python | ❌ | ❌ | ❌ | ⚠️ | ❌ | ❌ | Only for training |
| Kotlin | ❌ | ✅ | ❌ | ❌ | ✅ | ✅ | Android only |
| Swift | ❌ | ❌ | ✅ | ❌ | ✅ | ✅ | iOS only |
| **Rust** | ✅ WASM | ✅ FFI | ✅ FFI | ✅ bare metal | ✅ zero GC | ✅ native | **Core engine** |
| **Dart/Flutter** | ✅ | ✅ | ✅ | ❌ | ✅ AOT | ✅ | **UI + app layer** |

### The Answer: Two-Layer Architecture

```
┌─────────────────────────────────────────────────────────┐
│  LAYER A: Flutter / Dart  (Person B owns)               │
│  UI, alerts, TTS, SQLite queue, sync agent              │
│  Compiles to: Android APK / iOS IPA / Web PWA           │
│  Communicates via: FFI bindings (mobile) / WASM (web)   │
├─────────────────────────────────────────────────────────┤
│  LAYER B: Rust Core  (Person A owns the logic)          │
│  FeatureEngine, RuleEngine, TrustEngine, InferenceChain │
│  INT8 ONNX runtime, fallback orchestrator               │
│  Compiles to: .so/.dylib (Android/iOS) / .wasm (Web)    │
│              / binary (IoT — Raspberry Pi, ESP32-S3)    │
├─────────────────────────────────────────────────────────┤
│  LAYER C: Cloud / TypeScript  (Person C owns)           │
│  REST API, fine-tuning pipeline, dashboard              │
│  Receives sync batches from Flutter layer               │
└─────────────────────────────────────────────────────────┘
```

### Why Rust for the Core?

- **Zero garbage collection.** GC pauses kill real-time audio inference. Rust has none.
- **Compiles to WebAssembly.** Same Rust code runs in Chrome via WASM — no rewrite for web.
- **FFI to Dart.** Flutter calls Rust via `dart:ffi` with zero-copy buffer sharing.
- **IoT native.** Raspberry Pi 4, ESP32-S3 run Rust binaries directly. No JVM, no Python overhead.
- **INT8 ONNX Runtime.** `ort` crate (ONNX Runtime Rust bindings) supports INT8 quantized models natively.
- **Battery.** Zero-GC + no runtime overhead = 30–60% less CPU time = 30–60% less battery.

### Time Complexity Impact of Rust vs TypeScript for Inference

```
TypeScript (V8 JIT):
  Feature computation:  O(n) but GC pauses add 2–15ms unpredictably
  Rule evaluation:      O(r) where r = rules. JS object access ~10ns/field
  Total per frame:      8–25ms (with GC jitter)

Rust (compiled):
  Feature computation:  O(n) — same algorithm, ~0.3ns/op cache-friendly
  Rule evaluation:      O(r) — match arms compile to jump tables
  Total per frame:      0.5–2ms (deterministic, no jitter)

Difference: 10–12x faster per frame, zero latency spikes
For INT8 inference on 30-second audio window: Rust = ~4ms, TypeScript = ~40ms+
```

---

## Decision 2: The Fallback Chain — Edge AI → ML → Rule Engine

### The Core Problem

Edge AI models can be:
- Too slow for the current device (old IoT hardware)
- Unavailable (model not loaded yet, corrupted)
- Uncertain (confidence below threshold)
- Timing out (inference > configured deadline)

**We never leave the operator without a decision.** So we build three layers that cascade automatically.

### The Three-Tier Decision Chain

```
Signal arrives
      │
      ▼
┌─────────────────────────────────────────────────────────────┐
│  TIER 1: Edge AI Inference  (target: < 50ms)               │
│  - INT8 ONNX model (audio spectrogram → anomaly class)      │
│  - Runs: if model loaded AND compute budget available       │
│  - Output: { class, confidence: 0.0–1.0 }                   │
│  - Falls through if: timeout > 50ms OR confidence < 0.65    │
└─────────────────────────────────────────────────────────────┘
      │ (fallthrough if slow/uncertain)
      ▼
┌─────────────────────────────────────────────────────────────┐
│  TIER 2: ML-Based Statistical Detection  (target: < 10ms)  │
│  - Isolation Forest (pre-computed, in-memory)               │
│  - Statistical thresholds: z-score, IQR on feature window  │
│  - No model file needed — runs on computed FeatureWindow    │
│  - Falls through if: feature window < 5 samples            │
└─────────────────────────────────────────────────────────────┘
      │ (fallthrough if insufficient data)
      ▼
┌─────────────────────────────────────────────────────────────┐
│  TIER 3: Rule Engine  (target: < 1ms, ALWAYS fires)        │
│  - Policy YAML thresholds (coolant > 108°C = HIGH)         │
│  - Deterministic, zero uncertainty                          │
│  - Always produces a decision — even with 1 data point     │
│  - This tier NEVER falls through                           │
└─────────────────────────────────────────────────────────────┘
      │
      ▼
TrustEngine (cooldown + hysteresis — same regardless of tier)
      │
      ▼
Decision published to bus → GuidanceEngine → Operator
```

### Why This Ordering Matters

The decision to put Edge AI first (not Rule Engine first) is deliberate:

- **Edge AI catches patterns rules miss.** A failing bearing sounds unusual at 2,800 RPM even when coolant temp is normal. Rules can't catch this. Edge AI can.
- **Rule Engine is the safety net, not the first line.** Rules are conservative (threshold tuned for 99th percentile). Edge AI catches 80th percentile — earlier warnings.
- **ML middle tier bridges the gap.** On devices where the ONNX model is too large, statistical detection still catches obvious anomalies without needing the full model.

### Rust Implementation — InferenceChain

```rust
// packages/core-rust/src/inference_chain.rs

use std::time::{Duration, Instant};

pub const EDGE_AI_TIMEOUT_MS: u64 = 50;
pub const EDGE_AI_MIN_CONFIDENCE: f32 = 0.65;

pub enum DecisionSource {
    EdgeAI { confidence: f32, model_version: String },
    MLStatistical { method: &'static str },
    RuleEngine { rule_id: String },
}

pub struct ChainResult {
    pub severity:   Severity,
    pub source:     DecisionSource,
    pub latency_ms: u64,
    pub skipped:    Vec<&'static str>,  // which tiers were skipped and why
}

pub fn evaluate(window: &FeatureWindow, context: &InferenceContext) -> ChainResult {
    let start = Instant::now();
    let mut skipped = Vec::new();

    // ── TIER 1: Edge AI ─────────────────────────────────────────────────────
    if context.model_available {
        let timeout = Duration::from_millis(EDGE_AI_TIMEOUT_MS);
        match run_onnx_with_timeout(&window, timeout) {
            Ok(result) if result.confidence >= EDGE_AI_MIN_CONFIDENCE => {
                return ChainResult {
                    severity:   result.severity,
                    source:     DecisionSource::EdgeAI {
                        confidence:    result.confidence,
                        model_version: context.model_version.clone(),
                    },
                    latency_ms: start.elapsed().as_millis() as u64,
                    skipped,
                };
            }
            Ok(result) => {
                skipped.push("edge_ai: confidence below threshold");
                log::debug!("EdgeAI confidence {:.2} < {:.2}, falling through", 
                    result.confidence, EDGE_AI_MIN_CONFIDENCE);
            }
            Err(e) => {
                skipped.push("edge_ai: timeout or error");
                log::warn!("EdgeAI inference failed: {}", e);
            }
        }
    } else {
        skipped.push("edge_ai: model not loaded");
    }

    // ── TIER 2: ML Statistical ───────────────────────────────────────────────
    if window.sample_count >= 5 {
        if let Some(result) = run_isolation_forest(window) {
            return ChainResult {
                severity:   result.severity,
                source:     DecisionSource::MLStatistical { method: "isolation_forest" },
                latency_ms: start.elapsed().as_millis() as u64,
                skipped,
            };
        }
    } else {
        skipped.push("ml_statistical: insufficient samples");
    }

    // ── TIER 3: Rule Engine (ALWAYS fires) ───────────────────────────────────
    let rule_decision = run_rule_engine(window, &context.policy);
    ChainResult {
        severity:   rule_decision.severity,
        source:     DecisionSource::RuleEngine { rule_id: rule_decision.rule_id },
        latency_ms: start.elapsed().as_millis() as u64,
        skipped,
    }
}
```

### Config: How the Timeout Is Configured

**File: `policy/inference-config.yaml`**

```yaml
# Inference chain configuration — tuned per hardware class

inference:
  edge_ai:
    enabled: true
    timeout_ms: 50          # If inference takes longer, fall through
    min_confidence: 0.65    # If AI is unsure, fall through
    model_path: "models/anomedge_v1_int8.onnx"
    compute_budget_pct: 25  # Max % of CPU for inference (battery protection)

  ml_statistical:
    enabled: true
    method: "isolation_forest"
    min_samples: 5          # Minimum window samples before ML fires
    contamination: 0.05     # Expected anomaly rate (5%)

  rule_engine:
    enabled: true           # CANNOT be disabled — safety net
    policy_path: "policy/policy.yaml"

  fallback_log: true        # Log which tier fired and why — for debugging

# Per hardware class overrides
hardware_profiles:
  high_power:               # Phone flagship, Raspberry Pi 4
    edge_ai.timeout_ms: 50
    edge_ai.enabled: true

  mid_range:                # Mid-tier phones, Pi 3B+
    edge_ai.timeout_ms: 80
    edge_ai.min_confidence: 0.70  # Stricter — more falls through to ML

  low_power:                # IoT MCU, ESP32-S3, old phones
    edge_ai.enabled: false  # Skip AI tier entirely
    ml_statistical.enabled: true
```

---

## Decision 3: Contracts — The Single Document Every Person Reads

### What "Contracts" Means in Practice

Person B (Flutter) and Person C (TypeScript/Cloud) need to know:
- **What data arrives on each bus topic**
- **What fields are guaranteed vs optional**
- **What the allowed values are**

This means contracts must be:
1. Written in a language all three can read
2. The **JSON schema** form — not TypeScript types alone
3. Published as a human-readable reference page

### The Contracts as JSON Schema (Language-Neutral)

**File: `packages/contracts/schema/signal_event.schema.json`**

```json
{
  "$schema": "http://json-schema.org/draft-07/schema#",
  "$id": "https://anomedge.dev/schemas/SignalEvent",
  "title": "SignalEvent",
  "description": "A single telemetry frame from any vehicle source. Published on: signals.raw",
  "type": "object",
  "required": ["ts", "asset_id", "driver_id", "source", "signals"],
  "properties": {
    "ts":        { "type": "integer", "description": "Unix timestamp in milliseconds" },
    "asset_id":  { "type": "string",  "description": "Vehicle identifier e.g. TRUCK-001" },
    "driver_id": { "type": "string",  "description": "Driver identifier e.g. DRV-042" },
    "source": {
      "type": "string",
      "enum": ["SIMULATOR", "OBD2_GENERIC", "FORD_F450", "CAT_HEAVY", "JOHN_DEERE_139", "CUSTOM"],
      "description": "Which telematics adapter produced this frame"
    },
    "signals": { "$ref": "#/definitions/SignalMap" }
  },
  "definitions": {
    "SignalMap": {
      "type": "object",
      "description": "Key-value of all sensor readings. All fields optional — not every vehicle reports every signal.",
      "properties": {
        "coolant_temp":       { "type": "number", "description": "Engine coolant temperature in °C" },
        "engine_rpm":         { "type": "number", "description": "Engine speed in RPM" },
        "vehicle_speed":      { "type": "number", "description": "Vehicle speed in km/h" },
        "throttle_position":  { "type": "number", "description": "Throttle position 0–100%" },
        "engine_load":        { "type": "number", "description": "Engine load 0–100%" },
        "fuel_level":         { "type": "number", "description": "Fuel level 0–100%" },
        "battery_voltage":    { "type": "number", "description": "Battery voltage in V" },
        "brake_pedal":        { "type": "number", "description": "Brake pedal 0.0=off 1.0=full" },
        "oil_pressure":       { "type": "number", "description": "Oil pressure in kPa" },
        "hydraulic_pressure": { "type": "number", "description": "Heavy equipment hydraulic pressure kPa" },
        "transmission_temp":  { "type": "number", "description": "Transmission fluid temp °C" },
        "def_level":          { "type": "number", "description": "DEF/AdBlue level 0–100%" },
        "boom_position":      { "type": "number", "description": "Excavator boom angle degrees" },
        "load_weight":        { "type": "number", "description": "Payload weight kg" },
        "dtc_codes": {
          "type": "array",
          "items": { "type": "string" },
          "description": "Diagnostic trouble codes e.g. ['P0300', 'CAT-F1234']"
        }
      },
      "additionalProperties": { "type": ["number", "string"] }
    }
  }
}
```

**File: `packages/contracts/schema/action.schema.json`**

```json
{
  "$id": "https://anomedge.dev/schemas/Action",
  "title": "Action",
  "description": "Final output to operator. Published on: actions. This is what Person B reads.",
  "type": "object",
  "required": ["seq", "ts", "asset_id", "severity", "title", "guidance", "rule_id", "speak"],
  "properties": {
    "seq":      { "type": "integer", "description": "Monotonically increasing. If you see a gap, an event was dropped." },
    "ts":       { "type": "integer", "description": "Unix ms" },
    "asset_id": { "type": "string" },
    "severity": {
      "type": "string",
      "enum": ["NORMAL", "WATCH", "WARN", "HIGH", "CRITICAL"],
      "description": "NORMAL=no action. WATCH=monitor. WARN=address soon. HIGH=address now. CRITICAL=stop operating."
    },
    "title":         { "type": "string", "description": "Short label for alert card e.g. 'Coolant Overheating'" },
    "guidance":      { "type": "string", "description": "Full operator instruction from GuidanceEngine" },
    "rule_id":       { "type": "string", "description": "Which rule or AI class triggered this" },
    "speak":         { "type": "boolean", "description": "If true, Person B must invoke TTS. Always true for HIGH and CRITICAL." },
    "acknowledged":  { "type": "boolean", "description": "Set to true when operator taps acknowledge button" },
    "source":        { "type": "string", "enum": ["TEMPLATE", "LLM"], "description": "How guidance text was generated" },
    "decision_source": {
      "type": "string",
      "enum": ["EDGE_AI", "ML_STATISTICAL", "RULE_ENGINE"],
      "description": "Which inference tier produced this action. For diagnostics."
    }
  }
}
```

### The Human-Readable Bus Contract Table

This table is the **one document all three persons bookmark.** It answers: *"I am subscribing to topic X — what do I get?"*

```
╔══════════════════════╦════════════════════╦═══════════════════╦══════════════════╗
║ TOPIC                ║ PUBLISHED BY       ║ SUBSCRIBED BY     ║ SCHEMA           ║
╠══════════════════════╬════════════════════╬═══════════════════╬══════════════════╣
║ signals.raw          ║ Simulator (A)      ║ FeatureEngine (A) ║ SignalEvent       ║
║                      ║ Telematics Adapters║                   ║                  ║
╠══════════════════════╬════════════════════╬═══════════════════╬══════════════════╣
║ signals.features     ║ FeatureEngine (A)  ║ InferenceChain(A) ║ FeatureWindow    ║
║                      ║                   ║ Cloud dashboard(C)║                  ║
╠══════════════════════╬════════════════════╬═══════════════════╬══════════════════╣
║ decisions            ║ InferenceChain (A) ║ TrustEngine (A)   ║ Decision         ║
╠══════════════════════╬════════════════════╬═══════════════════╬══════════════════╣
║ decisions.gated      ║ TrustEngine (A)    ║ GuidanceEngine(B) ║ Decision         ║
║                      ║                   ║ Cloud ingestion(C)║                  ║
╠══════════════════════╬════════════════════╬═══════════════════╬══════════════════╣
║ actions              ║ GuidanceEngine (B) ║ AlertScreen (B)   ║ Action           ║
║                      ║                   ║ TTS service (B)   ║                  ║
║                      ║                   ║ LocalQueue (B)    ║                  ║
╠══════════════════════╬════════════════════╬═══════════════════╬══════════════════╣
║ telemetry.sync       ║ SyncAgent (B)      ║ Cloud API (C)     ║ EventEnvelope[]  ║
╠══════════════════════╬════════════════════╬═══════════════════╬══════════════════╣
║ model.ota            ║ Cloud pipeline (C) ║ ModelLoader (A)   ║ OTAUpdate        ║
╠══════════════════════╬════════════════════╬═══════════════════╬══════════════════╣
║ system.heartbeat     ║ All engines (A)    ║ Monitoring (C)    ║ Heartbeat        ║
╠══════════════════════╬════════════════════╬═══════════════════╬══════════════════╣
║ system.error         ║ Any engine         ║ Person C CI       ║ SystemError      ║
╚══════════════════════╩════════════════════╩═══════════════════╩══════════════════╝

Legend:
(A) = Person A owns the publisher and the schema definition
(B) = Person B consumes this topic — never changes the schema
(C) = Person C consumes for cloud — never changes the schema
```

**Rule:** Only the person who PUBLISHES a topic can change its schema. Schema changes require all-team sign-off.

---

## Decision 4: Complete Pub/Sub Flow — Signal to Operator Alert

### The Full Event Journey (Every Step Named)

```
VEHICLE / SIMULATOR
        │
        │  Raw telemetry frame
        │  (OBD2 PIDs / J1939 SPNs / JSON from cloud API)
        ▼
┌───────────────────────────────────┐
│  Telematics Adapter               │  ← Person A owns
│  (OBD2 / FordF450 / Cat / JD139) │
│  Normalizes → SignalEvent         │
└───────────────────────────────────┘
        │
        │  PUBLISH: signals.raw
        │  Payload: SignalEvent
        ▼
┌───────────────────────────────────┐
│  EventBus (in-process)            │  ← Person C owns the bus code
│  Typed, topic-based, p99 < 1ms    │
└───────────────────────────────────┘
        │
        │  SUBSCRIBE: signals.raw
        ▼
┌───────────────────────────────────┐
│  FeatureEngine (Rust)             │  ← Person A owns
│  30-second rolling window         │
│  Computes: slopes, spikes, means  │
└───────────────────────────────────┘
        │
        │  PUBLISH: signals.features
        │  Payload: FeatureWindow
        ▼
┌───────────────────────────────────┐
│  InferenceChain (Rust)            │  ← Person A owns
│  Tier 1: INT8 ONNX model          │
│  Tier 2: Isolation Forest         │
│  Tier 3: Rule Engine (YAML)       │
│  Selects winning decision         │
└───────────────────────────────────┘
        │
        │  PUBLISH: decisions
        │  Payload: Decision (includes decision_source tier)
        ▼
┌───────────────────────────────────┐
│  TrustEngine (Rust)               │  ← Person A owns
│  Cooldown per rule per asset      │
│  Hysteresis filter                │
│  Suppresses alert spam            │
└───────────────────────────────────┘
        │
        │  PUBLISH: decisions.gated  ← THIS IS THE HANDOFF TO PERSON B
        │  Payload: Decision (only meaningful decisions pass)
        ▼
        ├─────────────────────────────────────────────────┐
        │                                                 │
        ▼                                                 ▼
┌──────────────────────────┐               ┌──────────────────────────┐
│  GuidanceEngine (Dart/B) │               │  Cloud Ingestion (C)     │
│  Template or LLM lookup  │               │  Stores for analytics    │
│  Formats human text      │               │  Fine-tuning pipeline    │
└──────────────────────────┘               └──────────────────────────┘
        │
        │  PUBLISH: actions
        │  Payload: Action (final operator-facing output)
        ▼
        ├──────────────────┬──────────────────┐
        ▼                  ▼                  ▼
┌─────────────┐  ┌─────────────────┐  ┌──────────────┐
│ AlertScreen │  │  TTS Service    │  │ LocalQueue   │
│ (Flutter/B) │  │  (flutter_tts)  │  │ (SQLite/B)   │
│ Shows card  │  │  Speaks if HIGH │  │ Persists for │
│ + badge     │  │  or CRITICAL    │  │ cloud sync   │
└─────────────┘  └─────────────────┘  └──────────────┘
                                              │
                                   When online│
                                              ▼
                                   ┌──────────────────┐
                                   │  SyncAgent (B)   │
                                   │  Batches events  │
                                   │  POST /api/sync  │
                                   └──────────────────┘
                                              │
                                   PUBLISH: telemetry.sync
                                              ▼
                                   ┌──────────────────┐
                                   │  Cloud API (C)   │
                                   │  Fleet dashboard │
                                   │  LLM fine-tuning │
                                   └──────────────────┘
```

### What Person B Needs From Person A — Exactly

Person B subscribes to **exactly one topic: `decisions.gated`**. Nothing else.

Person B's contract is: "Give me a `Decision` object. I will turn it into an `Action` with human-readable text. I don't care how the decision was made."

```dart
// Person B reads this from the bus — Dart representation
// Generated from contracts/schema/decision.schema.json

class Decision {
  final int    ts;
  final String assetId;
  final String severity;        // 'NORMAL' | 'WATCH' | 'WARN' | 'HIGH' | 'CRITICAL'
  final String ruleId;          // e.g. 'coolant_overheat_critical'
  final String ruleGroup;       // e.g. 'thermal'
  final double confidence;      // 0.0–1.0
  final double rawValue;        // the actual sensor value that triggered this
  final double threshold;       // the threshold it crossed
  final String decisionSource;  // 'EDGE_AI' | 'ML_STATISTICAL' | 'RULE_ENGINE'
  // ... context FeatureWindow snapshot
}
```

### What Person C Needs From Person A — Exactly

Person C subscribes to **`signals.features`** (for dashboards) and **`decisions.gated`** (for cloud storage). The cloud layer is read-only for Person A's data.

---

## Decision 5: INT8 Quantization Path — Shrinking the Model for Edge

### Why INT8

| Precision | Model Size | Inference Speed | Battery Use | Accuracy Loss |
|-----------|-----------|----------------|-------------|---------------|
| FP32      | 100%       | 1×              | 100%        | 0%            |
| FP16      | 50%        | 1.5–2×          | 60%         | < 0.5%        |
| INT8      | 25%        | 3–4×            | 35%         | 1–2%          |
| INT4      | 12.5%      | 5–8×            | 20%         | 3–5%          |

**INT8 is the sweet spot.** 4× smaller, 3–4× faster, acceptable accuracy loss for our use case (we have the rule engine as safety net for edge cases the model misses).

### The Quantization Pipeline (Person A runs this once when model is ready)

```
STEP 1: Train in FP32
   Python / PyTorch
   Input: Audio spectrograms (mel-spectrogram, 128 bins, 3-second windows)
   Output: Anomaly class probabilities
   Dataset: Labeled engine sounds (MIMII dataset + AnomEdge collected data)
   ↓

STEP 2: Export to ONNX (FP32)
   torch.onnx.export(model, dummy_input, "anomedge_fp32.onnx",
     input_names=['spectrogram'],
     output_names=['anomaly_probs'],
     dynamic_axes={'spectrogram': {0: 'batch_size'}}
   )
   ↓

STEP 3: Calibrate (collect representative inputs)
   onnxruntime.quantization.CalibrationDataReader
   Feed 100–500 real engine audio samples
   Collect activation statistics (min/max per layer)
   ↓

STEP 4: Static INT8 Quantization
   from onnxruntime.quantization import quantize_static, QuantType
   quantize_static(
     "anomedge_fp32.onnx",
     "anomedge_int8.onnx",
     calibration_data_reader,
     quant_format=QuantFormat.QOperator,
     per_channel=True,          # Better accuracy than per-tensor
     weight_type=QuantType.QInt8,
     activation_type=QuantType.QUInt8
   )
   ↓

STEP 5: Validate accuracy didn't fall below threshold
   Compare FP32 vs INT8 on test set:
   - F1-score delta must be < 2%
   - No precision regression on CRITICAL class
   - If delta > 2%, try QAT (Quantization-Aware Training)
   ↓

STEP 6: Rust ONNX Runtime loads INT8 model
   let model = ort::Session::builder()
     .with_optimization_level(GraphOptimizationLevel::All)
     .with_intra_threads(1)      // Single thread for IoT battery saving
     .commit_from_file("anomedge_int8.onnx")?;
```

### Model Size Targets Per Platform

```
Desktop/Server:   FP32, full model, no limit
Android flagship: FP16, < 50MB
Android mid:      INT8, < 15MB
Raspberry Pi 4:   INT8, < 15MB, single thread
Raspberry Pi 3B+: INT8, < 8MB  (if too large → fall back to ML tier)
ESP32-S3:         INT4 or rule-only (< 2MB flash constraint)
Web (WASM):       INT8, < 15MB (ONNX.js or ort-web)
```

---

## The Immediate Next Steps — In Order

### Week 1: Foundation (All Three Persons in Parallel)

```
Person A (You):
  Day 1:  Set up Rust crate structure (anomedge-core)
          Write contracts as BOTH TypeScript AND JSON Schema
          Commit contracts — tag v1 — this is the freeze point
  Day 2:  Build telematics adapters (OBD2, Ford, Cat, JD) in Rust
          Write adapter tests with 5 real-world sample frames each
  Day 3:  FeatureEngine in Rust with 6 unit tests
  Day 4:  RuleEngine in Rust, loads policy.yaml
  Day 5:  TrustEngine + InferenceChain fallback orchestrator
          Write 5 scenario JSON files

Person B:
  Day 1:  Read contracts/schema/ folder — ask questions before building
  Day 2:  Flutter project scaffold + EventBus bridge (Dart FFI to Rust)
  Day 3+: Build against bus — subscribe decisions.gated, publish actions

Person C:
  Day 1:  Implement EventBus package in TypeScript
          All 9 topics wired, tested with vitest
  Day 2:  Phase 0 gate test scaffolding
  Day 3+: CI/CD, cloud API skeleton
```

### Week 2: Integration

```
Day 8:   Person A + C: First cross-package integration test
         Simulator publishes signals.raw → full pipeline → actions
         Must complete 100x speed scenario in < 5 seconds

Day 9:   Person B: Flutter app subscribes decisions.gated via FFI
         First alert displays on screen from simulated data

Day 10:  Person C: Phase 0 gate runs all 5 scenarios
         Target: 7/8 tests green (TrustEngine hysteresis is hard — OK to take until Day 12)

Day 12:  Phase 0 complete. All 8 gate tests green.
         pnpm demo shows colour-coded terminal output
         Flutter app shows alert for overheat_highway scenario
```

### Week 3–4: INT8 Model + Offline Proof

```
Day 14:  Integrate ONNX Runtime (ort crate) into Rust core
         Plug into InferenceChain Tier 1
         Use dummy model first (random weights) to prove pipeline works

Day 16:  Run INT8 quantization pipeline on MIMII dataset (public)
         Target: model < 15MB, F1 > 0.80 on test set

Day 18:  InferenceChain live: Edge AI fires on real audio anomalies
         Fallback tested: kill model file → ML tier fires automatically
         Kill model + thin window → Rule Engine fires automatically

Day 21:  Phase 1 gate: offline mode proven
         Flutter airplane mode test passes
         SQLite queue survives app restart
```

### Week 5–7: Live Intelligence + Demo

```
Day 28:  packages/chaos: ChaosToPolicy algorithm
         Chaos engine varies thresholds across 10,000 simulated scenarios
         Best-performing thresholds promoted to policy.yaml v2

Day 32:  Qwen2.5-0.5B-Q4 integrated (GuidanceEngine Phase 2)
         LLM generates human guidance text, replaces templates for HIGH/CRITICAL

Day 40:  Executive dashboard live on Canadian cloud infrastructure
         Fleet heatmap, driver scores, maintenance predictions

Day 42:  Demo video. Pitch ready.
```

---

## Summary Table — Decisions Made and Why

| Decision | What We Chose | Why |
|----------|---------------|-----|
| Core language | Rust | Zero GC, compiles everywhere, INT8 ONNX native |
| UI language | Dart/Flutter | AOT compiled, single codebase for Android/iOS/Web |
| Bus | In-process (Phase 0) → WebSocket (Phase 1) | No broker needed for single device |
| Model format | INT8 ONNX | 4× smaller, 3–4× faster, ONNX Runtime runs on all targets |
| Fallback chain | Edge AI → ML → Rules | Progressive degradation — never leaves operator blind |
| Contract format | TypeScript types + JSON Schema | TypeScript for compile-time safety, JSON Schema for Person B/C to read |
| Policy format | YAML | Human-editable, version-controlled, no code deploy needed for threshold change |
| Cloud hosting | Canadian (Hetzner CA or AWS ca-central-1) | Data sovereignty — $925M sovereign AI fund requirement |

---

*AnomEdge | Architecture Decisions v1.0 | Vancouver BC | March 2026*  
*Read before writing code. Change requires all-team sign-off.*
