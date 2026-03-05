# AnomEdge — Root Project Context

## What This Project Is
Edge AI platform for vehicle anomaly detection.
Runs on mobile (Flutter), web (WASM), IoT (Rust binary), and cloud (Node.js).

## Three-Person Team
- Person A (YOU): Rust crates — contracts, adapters, FeatureEngine, RuleEngine, TrustEngine, InferenceChain
- Person B: Flutter app — subscribes decisions.gated, publishes actions
- Person C: TypeScript — EventBus package, cloud API, gate tests, CI

## YOUR DOMAIN (Person A)
You own everything in:
- crates/anomedge-core/
- packages/contracts/  (shared types — define first, then FREEZE)
- scenarios/           (JSON test scenarios)
- policy/              (YAML thresholds)

## RULES — READ BEFORE EVERY TASK
1. Never import another crate's internals. Only @anomedge/contracts and @anomedge/bus cross-crate.
2. Contracts are FROZEN after Day 1 commit. No changes without all-team sign-off.
3. Tests first. Write failing tests before any implementation. No exceptions.
4. Policy YAML drives all thresholds. Never hardcode a number in Rust.
5. Telematics adapters are the only real-world bridge. FeatureEngine sees SignalEvent only.

## INFERENCE CHAIN (most important logic)
Three tiers in order:
1. Edge AI — INT8 ONNX model via ort crate — timeout 50ms, min confidence 0.65
2. ML Statistical — Isolation Forest on FeatureWindow — needs >= 5 samples
3. Rule Engine — Policy YAML thresholds — ALWAYS fires, never skips

## BUS TOPICS (Person A publishes these)
- signals.raw      → FeatureEngine subscribes
- signals.features → InferenceChain subscribes
- decisions        → TrustEngine subscribes
- decisions.gated  → Person B subscribes (THE HANDOFF — do not break this contract)

## LANGUAGE
- Rust for all inference logic (zero GC, compiles to WASM/iOS/Android/IoT)
- TypeScript only for packages/contracts (JSON schema generation)
- Python only for model training scripts (not in codebase)

## TARGET PERFORMANCE
- Feature computation: < 2ms per frame
- Rule engine: < 1ms per evaluation
- INT8 ONNX inference: < 50ms (fallback if exceeded)
- Total pipeline signal.raw → decisions.gated: < 100ms p99

## INT8 MODEL TARGET
- Format: ONNX with INT8 static quantization
- Size: < 15MB for mobile, < 8MB for IoT
- F1 delta vs FP32: must be < 2%
- Input: mel-spectrogram 128 bins, 3-second window
