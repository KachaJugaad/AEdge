# packages/contracts — Shared Types

## This Package Is Special
It is written FIRST. It is FROZEN after Day 1 merge.
Person B (Flutter/Dart) and Person C (TypeScript) both depend on it.
No changes after merge without ALL THREE persons signing off.

## What To Generate
1. src/index.ts         — TypeScript types (compile-time safety for Person C)
2. schema/              — JSON Schema files (Person B reads these for Dart code)
   - signal_event.schema.json
   - feature_window.schema.json
   - decision.schema.json
   - action.schema.json
   - event_envelope.schema.json

## Key Types
- Severity: 'NORMAL' | 'WATCH' | 'WARN' | 'HIGH' | 'CRITICAL'
- SignalSource: 'SIMULATOR' | 'OBD2_GENERIC' | 'FORD_F450' | 'CAT_HEAVY' | 'JOHN_DEERE_139' | 'CUSTOM'
- SignalEvent: ts, asset_id, driver_id, source, signals (SignalMap), raw_frame?
- FeatureWindow: ts, asset_id, coolant_slope, brake_spike_count, hydraulic_spike, transmission_heat, dtc_new[]
- Decision: ts, asset_id, severity, rule_id, rule_group, confidence, raw_value, threshold, decision_source
- Action: seq, ts, asset_id, severity, title, guidance, speak bool, source 'TEMPLATE'|'LLM'
- BusTopic: 9 topics as string union
- EventEnvelope<T>: id, topic, seq, ts, payload

## Tests Required
- Import every type, assert all required fields exist
- JSON Schema validates a sample object for each type
- Run: pnpm test
