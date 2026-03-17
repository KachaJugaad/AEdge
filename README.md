# AnomEdge

**Edge AI for Vehicle Anomaly Detection**

> Your vehicle knows something is wrong. Before you do.

[![Live Site](https://img.shields.io/badge/Live%20Site-kachajugaad.github.io%2FAEdge-58a6ff?style=flat-square&logo=github)](https://kachajugaad.github.io/AEdge/)
[![Tests](https://img.shields.io/badge/Tests-145%20passing-3fb950?style=flat-square)](#tests)
[![Rust](https://img.shields.io/badge/Rust-edge%20core-e3783e?style=flat-square&logo=rust)](#architecture)
[![TypeScript](https://img.shields.io/badge/TypeScript-contracts%20%26%20web-3178c6?style=flat-square&logo=typescript)](#architecture)

---

**[View the full project site with interactive deep dives, architecture diagrams, and demos](https://kachajugaad.github.io/AEdge/)**

---

## What is this?

AnomEdge runs anomaly detection **directly on the vehicle** — no cloud, no latency, no dead zones. It watches hundreds of sensor readings every second and tells the driver exactly what to do when something goes wrong.

```
Engine overheating? → "Pull over within 5 minutes. Turn on the heater to draw heat."
Harsh braking?     → "Increase following distance. Check brakes at next stop."
Battery dying?     → "Drive directly to nearest mechanic. Don't turn off engine."
```

**Under 100ms** from raw sensor data to driver alert. Works offline. Runs on phones, tablets, IoT devices, or in the browser via WASM.

## Why edge, not cloud?

| | Cloud Telematics | AnomEdge |
|---|---|---|
| **Latency** | 2-30 seconds round trip | < 100ms on-device |
| **Offline** | No signal = no alerts | Always works |
| **Privacy** | Raw telemetry uploaded | Data stays on vehicle |
| **Cost** | Per-vehicle cloud fees | Zero runtime cost |
| **Reliability** | Depends on server uptime | Self-contained |

## Architecture

```
┌─────────────┐    ┌────────────────┐    ┌─────────────────┐    ┌──────────────┐    ┌──────────────┐
│  Telematics  │───>│ Feature Engine │───>│ Inference Chain  │───>│ Trust Engine │───>│ Driver Alert │
│   Adapter    │    │ (30s window)   │    │ ONNX→ML→Rules   │    │ (cooldown +  │    │ (title +     │
│              │    │                │    │                  │    │  hysteresis)  │    │  guidance)   │
└─────────────┘    └────────────────┘    └─────────────────┘    └──────────────┘    └──────────────┘
     < 1ms              < 2ms                 < 50ms                 < 1ms            = < 100ms p99
```

**3-Tier Inference Chain** (never misses):
1. **Tier 1 — Edge AI**: INT8 ONNX neural network on mel-spectrograms (50ms timeout)
2. **Tier 2 — ML Statistical**: Isolation Forest for unsupervised outlier detection
3. **Tier 3 — Rule Engine**: Policy YAML thresholds — always fires, the safety net

**Bus Topics** (EventBus pub/sub):
```
signals.raw → signals.features → decisions → decisions.gated → actions
```

## Project Structure

```
anomEdge/
├── crates/anomedge-core/src/    # Rust core (Person A)
│   ├── types.rs                 # Shared types (frozen)
│   ├── adapters/                # OBD2, Ford, Cat, JD adapters
│   ├── feature.rs               # 30s rolling window FeatureEngine
│   ├── rules.rs                 # Policy YAML RuleEngine
│   ├── trust.rs                 # Cooldown + hysteresis TrustEngine
│   ├── inference.rs             # 3-tier InferenceChain
│   ├── pipeline.rs              # Full wiring
│   ├── ffi.rs                   # C FFI exports (iOS/Android)
│   └── wasm.rs                  # WASM exports (browser)
│
├── packages/contracts/src/      # TypeScript types (frozen)
├── packages/bus/src/            # EventBus with metrics
├── packages/core/src/           # TS pipeline (FeatureEngine, RuleEngine, etc.)
├── packages/web-terminal/src/   # React dashboard + driver app preview
│
├── scenarios/                   # JSON test scenarios
├── policy/policy.yaml           # All detection thresholds
├── gate-tests/                  # Integration gate tests
│
└── docs/                        # GitHub Pages site
    ├── index.html               # Project site
    └── videos/                  # Drop demo videos here
```

## Quick Start

```bash
# Install dependencies
pnpm install

# Run TypeScript tests
pnpm --filter @anomedge/contracts test

# Run Rust tests
cd crates/anomedge-core && cargo test

# Launch the web dashboard
pnpm --filter @anomedge/web-terminal dev
# → Open http://localhost:5173
```

## Web Dashboard

The web terminal runs the entire pipeline in-browser (no backend needed):

- **Scenario Mode**: Replay 5 pre-built scenarios (overheat, harsh brake, cold start, oscillating fault, hydraulic)
- **Live Mode**: Real-time simulator generating multi-fault telemetry at 1 Hz
- **Driver App Tab**: Phone-frame preview showing exactly what the mobile app displays
- **Bus Metrics**: p50/p95/p99 latencies per topic
- **Pipeline View**: Stage-by-stage message counts

## Detection Rules

18 rules across 9 categories, all configurable via `policy/policy.yaml`:

| Category | Rules | Example |
|---|---|---|
| Thermal | coolant_high_temp, coolant_overheat_critical, coolant_rising_fast, intake_air_hot | "Coolant > 108°C → CRITICAL: Pull over immediately" |
| Braking | harsh_brake_event, excessive_braking | "5+ hard brakes in 30s → HIGH: Slow down" |
| Speed | speed_over_limit_watch, speed_over_limit_high | "> 130 km/h → HIGH: Logged for fleet review" |
| Hydraulic | hydraulic_spike_rule | "Pressure spike → HIGH: Visit mechanic" |
| Transmission | transmission_overheat, transmission_heat_flag | "> 110°C → HIGH: Stop towing" |
| Electrical | low_battery_voltage, critical_battery_voltage | "< 10V → CRITICAL: Don't turn off engine" |
| DTC | dtc_new_codes | "New fault code → WARN: Schedule diagnostic" |
| Fuel | fuel_level_low, fuel_level_critical | "< 5% → HIGH: Refuel immediately" |
| Engine | engine_overload, high_idle_rpm | "Load > 90% → WARN: Check payload" |

## Tests

**145 tests total** — all passing:
- 61 adapter tests (OBD2, Ford F-450, Cat, JD + registry)
- 12 FeatureEngine tests (rolling window, slopes, spikes)
- 12 RuleEngine tests (all operators, severity, groups)
- 8 TrustEngine tests (cooldown, hysteresis)
- 10 InferenceChain tests (3-tier fallback)
- 9 Pipeline integration tests
- 5 Scenario tests (JSON replay)
- 10 FFI tests + 8 WASM tests
- 8 Type compatibility tests (TS ↔ Rust)
- 13 Contract tests

## Driver Actions

Every alert maps to a specific driver-facing action:

| Severity | Urgency | Voice | Example Guidance |
|---|---|---|---|
| CRITICAL | PULL OVER | Yes | "Pull over IMMEDIATELY. Turn off engine. Call roadside assistance." |
| HIGH | ACTION NEEDED | Yes | "Reduce speed. Visit mechanic within 24 hours." |
| WARN | CAUTION | No | "Increase following distance. Schedule a check." |
| WATCH | MONITOR | No | "Performance may drop slightly. Awareness only." |

## Videos

Drop demo videos (MP4, WebM, MOV) into `docs/videos/` and add them to the manifest in `docs/index.html`:

```javascript
const VIDEO_MANIFEST = [
  { file: 'demo-overheat.mp4', title: 'Overheat Scenario', description: 'Live detection of coolant overheat' },
  { file: 'live-mode.mp4', title: 'Live Simulation', description: 'Multi-fault real-time simulation' },
];
```

They'll appear on the [project site](https://kachajugaad.github.io/AEdge/) automatically.

## Team

| Person | Domain | Owns |
|---|---|---|
| **A** | Rust Core | Contracts, adapters, engines, inference, FFI/WASM |
| **B** | Flutter App | Mobile driver app, decisions.gated subscriber |
| **C** | TypeScript | EventBus, gate tests, CI, web terminal |

## License

MIT

---

**[View the full interactive project site →](https://kachajugaad.github.io/AEdge/)**
