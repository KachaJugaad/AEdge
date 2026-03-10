# gate-tests — Phase Gate Tests

## What Gate Tests Do
Each gate test replays ALL 5 scenarios at 100x speed and makes assertions
about event sequences, severity escalations, latency, and output quality.
Gate = merge approval. Green = merge. Red = block and send fix prompt.

## gate test structure (every gate file):
1. bus.reset() — always first line of each test
2. Load scenario JSON and replay via SimulatorService at 100x speed
3. Subscribe to relevant bus topics and collect events
4. Assert on collected events
5. Report pass/fail with colour output (chalk)

## Phase 0 Gate (phase0.gate.ts) — 8 tests
1. overheat_highway reaches severity CRITICAL
2. cold_start_normal produces zero WARN+ events
3. oscillating_fault: alert fires, then is suppressed by TrustEngine, then re-fires after cooldown
4. All 5 guidance templates pass validateGuidance() output validator
5. TTS flag is true for HIGH and CRITICAL, false for NORMAL/WATCH/WARN
6. p95 action latency under 500ms (signals.raw → actions)
7. EventEnvelope seq numbers are monotonically increasing (no gaps)
8. bus.getMetrics() returns data for all active topics

## Important
- bus.reset() before EVERY test — shared bus state causes test pollution
- Run at 100x speed — 60-second overheat scenario completes in ~600ms
- All output to terminal must be colour-coded (chalk: green=pass, red=fail)
- Exit code 0 if all pass, exit code 1 if any fail (CI reads this)
