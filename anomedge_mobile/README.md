# AnomEdge Mobile — Person B (Interface Engineer)

Flutter Android app. Offline-first. Receives decisions from the engine, speaks them, stores them, syncs them.

---

## Architecture

```
EventBus (Person C)
    │
    ├── decisions.gated ──► GuidanceEngine ──► actions ──► AlertScreen (UI)
    │                                                   └── actions.spoken ──► TTS
    │
    └── signals.raw ──► SimulatorScreen (dev only)

LocalQueue (SQLite) ◄── AlertScreen writes envelopes
    │
    └── SyncAgent ──► Cloud REST API (when online)
```

## Package Structure

```
lib/
├── main.dart                     # Entry point, routing, offline banner
├── contracts/
│   └── contracts.dart            # Dart mirror of TypeScript contracts (Person A)
├── bus/
│   └── event_bus.dart            # Dart EventBus via StreamController
├── guidance/
│   ├── validate_guidance.dart    # Output validator (3–15 words, verb, no banned terms)
│   ├── templates.dart            # DEFAULT_TEMPLATES for all DecisionType × Severity
│   └── guidance_engine.dart      # Subscribes to decisions.gated → publishes actions
├── queue/
│   └── local_queue.dart          # SQLite offline store (sqflite)
├── sync/
│   └── sync_agent.dart           # Connectivity detection + cloud batch upload
└── screens/
    ├── alert_screen.dart         # PRIMARY operator UI — severity badge, guidance, TTS
    ├── history_screen.dart       # Alert history list
    ├── simulator_screen.dart     # Scenario replay for dev/demo (no hardware needed)
    └── settings_screen.dart      # TTS toggle, driver mode, asset/driver ID
```

## Phase Deliverables

### Phase 0 ✅ (this codebase)
- [x] `validateGuidance()` + tests
- [x] `DEFAULT_TEMPLATES` + tests (all 25 combinations)
- [x] `GuidanceEngine` (template mode) 
- [x] Flutter app skeleton — 4 screens
- [x] Dart contracts (mirrors Person A's TypeScript types)
- [x] Dart EventBus via StreamController
- [x] `AlertScreen` widget (severity badge, guidance, acknowledge)
- [x] `SimulatorScreen` (scenario JSON replay, Phase 0 hardcoded)

### Phase 1 (next)
- [ ] `LocalQueue` (SQLite) + tests ← code written, needs sqflite device test
- [ ] TTS integration (flutter_tts) + tests
- [ ] Offline banner (connectivity_plus) ← UI done, needs real device
- [ ] `SyncAgent` + integration tests (mock server)
- [ ] Airplane mode integration test

### Phase 2 (later)
- [ ] Live LLM: Qwen2.5-0.5B-Q4 via llama.cpp FFI
- [ ] "Why did this fire?" detail modal
- [ ] OTA model update receiver
- [ ] Full fleet sync to production cloud API

---

## Setup

```bash
# Install Flutter (if not already)
# https://docs.flutter.dev/get-started/install

# Get dependencies
flutter pub get

# Run tests (guidance + templates — no device needed)
flutter test test/guidance/

# Run on Android emulator
flutter run
```

## Rules of Engagement (your contract)

1. **Never import from Person A or C's packages directly** — only use EventBus + contracts
2. **Templates are mandatory, LLM is optional** — if LLM >400ms or fails, template fires
3. **Offline-first always wins** — if a feature needs internet, it belongs in cloud layer
4. **Tests before implementation** — every feature starts as a failing test
5. **Only Person C merges to main** — submit PRs, don't self-merge
6. **`pnpm demo` must run at all times** — if main breaks, all work stops

## Waiting On

| Dependency | From | Status |
|---|---|---|
| `contracts/index.ts` (TypeScript types) | Person A | **BLOCKING** |
| Dart test vectors (parity check) | Person A | Phase 0 close |
| `packages/bus` EventBus merged to main | Person C | **BLOCKING for integration** |
| GitHub repo + CI pipeline | Person C | **BLOCKING for PRs** |

---

*AnomEdge | Edge AI for Vehicle Operations | Vancouver, BC | 2026*
