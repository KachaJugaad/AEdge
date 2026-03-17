# AnomEdge

**Edge AI for Vehicle Anomaly Detection**

> Your vehicle knows something is wrong. Before you do.

[![Live Site](https://img.shields.io/badge/Live%20Site-kachajugaad.github.io%2FAEdge-58a6ff?style=flat-square&logo=github)](https://kachajugaad.github.io/AEdge/)
[![Tests](https://img.shields.io/badge/Tests-145%20passing-3fb950?style=flat-square)](#testing)
[![Rust](https://img.shields.io/badge/Rust-edge%20core-e3783e?style=flat-square&logo=rust)](#tech-stack)
[![TypeScript](https://img.shields.io/badge/TypeScript-contracts%20%26%20web-3178c6?style=flat-square&logo=typescript)](#tech-stack)

---

**[View the full project site with interactive deep dives and demos &rarr;](https://kachajugaad.github.io/AEdge/)**

---

## What is this?

AnomEdge runs anomaly detection **directly on the vehicle** — no cloud, no latency, no dead zones. It watches hundreds of sensor readings every second and tells the driver exactly what to do when something goes wrong.

```
Engine overheating? → "Pull over within 5 minutes. Turn on the heater to draw heat."
Harsh braking?     → "Increase following distance. Check brakes at next stop."
Battery dying?     → "Drive directly to nearest mechanic. Don't turn off engine."
```

**Under 100ms** from raw sensor data to driver alert. Works offline. Runs on phones, tablets, IoT devices, or in the browser.

## Why edge, not cloud?

| | Cloud Telematics | AnomEdge |
|---|---|---|
| **Latency** | 2-30 seconds round trip | < 100ms on-device |
| **Offline** | No signal = no alerts | Always works |
| **Privacy** | Raw telemetry uploaded | Data stays on vehicle |
| **Cost** | Per-vehicle cloud fees | Zero runtime cost |
| **Reliability** | Depends on server uptime | Self-contained |

## How It Works

```
┌──────────────────┐          ┌────────────────────────┐          ┌──────────────────┐
│  Vehicle Sensors  │────────>│   AnomEdge AI Engine    │────────>│   Driver Alert    │
│                  │          │                        │          │                  │
│  100s of signals │          │  Multi-layer inference  │          │  Plain English    │
│  per second      │          │  with smart filtering   │          │  + voice alerts   │
└──────────────────┘          └────────────────────────┘          └──────────────────┘
                                   < 100ms total
```

### 3-Tier AI — Why It Never Misses

AnomEdge uses a **cascading fallback** strategy. If the most advanced AI is unavailable or uncertain, the next tier catches it. The system **always** produces a result.

| Tier | Method | Role |
|---|---|---|
| **1** | Deep Learning | Catches subtle, complex patterns humans can't see |
| **2** | Statistical ML | Learns what "normal" looks like — no training labels needed |
| **3** | Expert Rules | Human-defined safety thresholds — the backstop that never fails |

## What It Detects

18 detection rules across 9 categories — all configurable by fleet managers without code changes:

| Category | What It Catches | Example Action |
|---|---|---|
| **Thermal** | Overheating, rapid temperature rise | "Pull over immediately. Turn off engine." |
| **Braking** | Harsh braking, brake abuse | "Slow down. Increase following distance." |
| **Speed** | Speeding, dangerous velocity | "Reduce speed. Event logged for review." |
| **Hydraulic** | Pressure spikes | "Visit mechanic. Possible seal issue." |
| **Transmission** | Overheating, heat flags | "Stop towing. Let it cool." |
| **Electrical** | Low/critical battery | "Don't turn off engine. Head to mechanic." |
| **Fuel** | Low/critical fuel | "Refuel immediately." |
| **Engine** | Overload, over-rev | "Check payload. Shift to higher gear." |
| **Diagnostics** | New fault codes | "Schedule diagnostic scan." |

## Driver Actions

Every alert comes with specific, actionable guidance:

| Severity | Urgency | Voice Alert | What the driver sees |
|---|---|---|---|
| **CRITICAL** | PULL OVER | Yes | "Pull over IMMEDIATELY. Turn off engine. Call roadside assistance." |
| **HIGH** | ACTION NEEDED | Yes | "Reduce speed. Visit mechanic within 24 hours." |
| **WARN** | CAUTION | No | "Increase following distance. Schedule a check." |
| **WATCH** | MONITOR | No | "Performance may drop slightly. Awareness only." |

## Key Capabilities

| Capability | Status |
|---|---|
| Multi-vehicle support (trucks, heavy equipment, construction) | Done |
| Real-time sensor analysis (100s of signals/sec) | Done |
| 3-tier AI with cascading fallback | Done |
| Smart alert filtering (no alert fatigue) | Done |
| Cross-platform (browser, iOS, Android, IoT, cloud) | Done |
| Live simulation dashboard | Done |
| Plain-English driver guidance with voice | Done |
| Advanced unsupervised anomaly scoring | In Progress |
| Native mobile driver app | Next |
| Fleet management dashboard | Planned |
| LLM-powered contextual guidance | Future |

## Tech Stack

| Technology | Why |
|---|---|
| **Rust** | Zero GC, memory-safe, compiles to every target |
| **WebAssembly** | Same logic in the browser |
| **React + Vite** | Dashboard and simulation |
| **Flutter** | Native driver mobile app (planned) |

## Web Dashboard

The dashboard runs the entire AI pipeline **in-browser** — no backend needed:

- **Scenario Mode**: Replay pre-built fault scenarios
- **Live Mode**: Real-time multi-fault simulation at 1 Hz
- **Driver App Tab**: Phone-shaped preview of what the driver sees
- **Pipeline Metrics**: Real-time performance monitoring

## Testing

**145 tests** covering unit, integration, and scenario-based testing. CI via GitHub Actions.

## Videos & Demos

Demo videos are available on the [project site](https://kachajugaad.github.io/AEdge/#videos).

## License

MIT

---

**[View the full interactive project site &rarr;](https://kachajugaad.github.io/AEdge/)**
