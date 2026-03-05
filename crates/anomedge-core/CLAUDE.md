# anomedge-core — Rust Crate

## Purpose
This crate is the intelligence engine. It runs on every platform.
Compiles to: native binary (IoT), .so/.dylib (Android/iOS FFI), .wasm (web).

## Modules to Build (in order)
1. types.rs        — mirrors packages/contracts/src/index.ts exactly
2. adapters/       — telematics normalizers (OBD2, FordF450, Cat, JD139)
3. feature.rs      — FeatureEngine (30-second rolling window)
4. rules.rs        — RuleEngine (loads policy YAML via serde)
5. trust.rs        — TrustEngine (cooldown + hysteresis per asset)
6. inference.rs    — InferenceChain (3-tier fallback orchestrator)
7. pipeline.rs     — wires all above together

## Key Dependencies (add to Cargo.toml)
[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
serde_yaml = "0.9"
ort = "2"                    # ONNX Runtime (INT8 inference)
ndarray = "0.15"             # Array ops for spectrograms
uuid = { version = "1", features = ["v4"] }
log = "0.4"
thiserror = "1"
tokio = { version = "1", features = ["full"] }   # async runtime

[dev-dependencies]
approx = "0.5"               # floating point assertions in tests

## Test Strategy
- Unit tests in each module (mod tests {})
- Integration tests in tests/ folder
- Run: cargo test
- Run with output: cargo test -- --nocapture
- Benchmark: cargo bench (add when optimizing)

## INT8 Inference
- Model path: ../../models/anomedge_int8.onnx
- Use ort::Session::builder().commit_from_file()
- Single thread for IoT: .with_intra_threads(1)
- Timeout: run in tokio::time::timeout(Duration::from_millis(50), ...)
- Fallback: if Err(_) or confidence < 0.65 → return None → caller tries Tier 2

## Time Complexity Targets
- FeatureEngine.ingest(): O(n) where n = window size. Max window = 300 samples.
  Use VecDeque not Vec — O(1) push_front/pop_back.
- RuleEngine.evaluate(): O(r) where r = rules count (~20). Use match, not loop+if.
- TrustEngine.evaluate(): O(1) — HashMap lookup by rule_id.
