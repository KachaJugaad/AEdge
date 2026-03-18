// gate-tests/phase1.gate.ts
// Phase 1 Gate — 6 tests
// Person C owns this file. Green gate = merge. Red gate = fix.
//
// Blocked tests show in YELLOW and do NOT count as failures.
// Exit code: 0 if all non-blocked pass, 1 if any non-blocked fail.
//
// Pattern mirror: same test runner as phase0.gate.ts.
//   - ANSI colors (no chalk — CJS compat)
//   - TestResult type
//   - resetAll() before every test
//   - X/6 tests passing (Y blocked) summary

import { bus } from '@anomedge/bus';
import type {
  SignalEvent,
  FeatureWindow,
  Decision,
  Action,
  Severity,
  EventEnvelope,
  PolicyPack,
  PolicyRule,
  SignalMap,
} from '@anomedge/contracts';
import * as fs from 'fs';
import * as path from 'path';

// ─── ANSI helpers (no chalk — ESM/CJS compat) ────────────────────────────────

const green  = (s: string) => `\x1b[32m${s}\x1b[0m`;
const red    = (s: string) => `\x1b[31m${s}\x1b[0m`;
const yellow = (s: string) => `\x1b[33m${s}\x1b[0m`;
const bold   = (s: string) => `\x1b[1m${s}\x1b[0m`;
const dim    = (s: string) => `\x1b[2m${s}\x1b[0m`;
// cyan available for future use: const cyan = (s: string) => `\x1b[36m${s}\x1b[0m`;

// ─── TestResult ───────────────────────────────────────────────────────────────

interface TestResult {
  name:    string;
  pass:    boolean;
  blocked: boolean;   // true = YELLOW stub, not a failure
  message: string;
}

// ─── Scenario + Policy loaders (same as phase0) ───────────────────────────────

interface ScenarioFrame {
  ts_offset_ms: number;
  signals: Record<string, number | string | string[]>;
}

interface Scenario {
  name: string;
  vehicle_class: string;
  asset_id: string;
  driver_id: string;
  expected_alerts: string[];
  expected_max_severity: string;
  frames: ScenarioFrame[];
}

function loadScenario(name: string): Scenario {
  const p = path.resolve(__dirname, '..', 'scenarios', `${name}.json`);
  return JSON.parse(fs.readFileSync(p, 'utf-8'));
}

function loadPolicy(): PolicyPack {
  const raw = fs.readFileSync(
    path.resolve(__dirname, '..', 'policy', 'policy.yaml'),
    'utf-8'
  );
  const rules: PolicyRule[] = [];
  let cur: Partial<PolicyRule> | null = null;

  for (const line of raw.split('\n')) {
    const t = line.trim();
    if (t.startsWith('#') || t === '') continue;
    if (t.startsWith('- id:')) {
      if (cur && cur.id) rules.push(cur as PolicyRule);
      cur = { id: t.split(':').slice(1).join(':').trim() };
    } else if (cur) {
      const m = t.match(/^(\w+):\s*(.+)$/);
      if (m) {
        const [, k, v] = m;
        switch (k) {
          case 'group':       cur.group       = v as any; break;
          case 'signal':      cur.signal      = v; break;
          case 'operator':    (cur as any).operator   = v; break;
          case 'threshold':   cur.threshold   = parseFloat(v); break;
          case 'severity':    cur.severity    = v as Severity; break;
          case 'cooldown_ms': cur.cooldown_ms = parseInt(v, 10); break;
          case 'hysteresis':  cur.hysteresis  = parseFloat(v); break;
          case 'description': cur.description = v.replace(/^"(.*)"$/, '$1'); break;
        }
      }
    }
  }
  if (cur && cur.id) rules.push(cur as PolicyRule);
  return { version: '1.0', vehicle_class: 'LIGHT_TRUCK', rules };
}

// ─── StubPipeline (verbatim from phase0 — no changes to shared logic) ─────────

const WINDOW_SECONDS = 30;
const WINDOW_MS = WINDOW_SECONDS * 1000;

class StubFeatureEngine {
  private buffers: Map<string, SignalEvent[]> = new Map();

  ingest(event: SignalEvent): FeatureWindow {
    const { asset_id, ts } = event;
    let buf = this.buffers.get(asset_id);
    if (!buf) { buf = []; this.buffers.set(asset_id, buf); }
    buf.push(event);
    const cutoff = ts - WINDOW_MS;
    while (buf.length > 0 && buf[0].ts < cutoff) buf.shift();
    return this.computeFeatures(asset_id, buf);
  }

  private computeFeatures(asset_id: string, window: SignalEvent[]): FeatureWindow {
    const latest = window[window.length - 1];
    const n = window.length;
    const coolantTemps = window.map(e => (e.signals.coolant_temp  as number) ?? 0);
    const speeds       = window.map(e => (e.signals.vehicle_speed as number) ?? 0);
    const rpms         = window.map(e => (e.signals.engine_rpm    as number) ?? 0);
    const loads        = window.map(e => (e.signals.engine_load   as number) ?? 0);
    const throttles    = window.map(e => (e.signals.throttle_position as number) ?? 0);

    const coolant_slope = this.slope(coolantTemps);
    let brake_spike_count = 0;
    for (let i = 1; i < n; i++) {
      const prev = (window[i-1].signals.brake_pedal as number) ?? 0;
      const curr = (window[i  ].signals.brake_pedal as number) ?? 0;
      if (prev < 0.8 && curr >= 0.8) brake_spike_count++;
    }
    let hydraulic_spike = false;
    for (let i = 1; i < n; i++) {
      const prev = (window[i-1].signals.hydraulic_pressure as number) ?? 0;
      const curr = (window[i  ].signals.hydraulic_pressure as number) ?? 0;
      if (Math.abs(curr - prev) > 500) { hydraulic_spike = true; break; }
    }
    const transmission_heat = window.some(e => ((e.signals.transmission_temp as number) ?? 0) > 110);
    const latestDtc = (latest.signals.dtc_codes as string[]) ?? [];
    let dtc_new: string[];
    if (n <= 1) {
      dtc_new = [...latestDtc];
    } else {
      const prior = new Set<string>();
      for (let i = 0; i < n - 1; i++) {
        for (const c of ((window[i].signals.dtc_codes as string[]) ?? [])) prior.add(c);
      }
      dtc_new = latestDtc.filter(c => !prior.has(c));
    }
    return {
      ts: latest.ts, asset_id, window_seconds: WINDOW_SECONDS,
      coolant_slope, brake_spike_count,
      speed_mean: this.mean(speeds), rpm_mean: this.mean(rpms),
      engine_load_mean: this.mean(loads), throttle_variance: this.variance(throttles),
      hydraulic_spike, transmission_heat, dtc_new,
      signals_snapshot: { ...latest.signals } as Partial<SignalMap>,
    };
  }

  private slope(values: number[]): number {
    const n = values.length;
    if (n <= 1) return 0;
    const mx = (n - 1) / 2, my = this.mean(values);
    let num = 0, den = 0;
    for (let i = 0; i < n; i++) {
      const dx = i - mx;
      num += dx * (values[i] - my);
      den += dx * dx;
    }
    return den === 0 ? 0 : num / den;
  }

  private mean(v: number[]): number {
    if (v.length === 0) return 0;
    let s = 0; for (const x of v) s += x; return s / v.length;
  }

  private variance(v: number[]): number {
    if (v.length <= 1) return 0;
    const m = this.mean(v);
    let s = 0; for (const x of v) { const d = x - m; s += d * d; }
    return s / v.length;
  }
}

function resolveSignal(w: FeatureWindow, signal: string): number | undefined {
  const direct: Record<string, number> = {
    coolant_slope:        w.coolant_slope,
    brake_spike_count:    w.brake_spike_count,
    speed_mean:           w.speed_mean,
    rpm_mean:             w.rpm_mean,
    engine_load_mean:     w.engine_load_mean,
    throttle_variance:    w.throttle_variance,
    hydraulic_spike:      w.hydraulic_spike      ? 1 : 0,
    transmission_heat:    w.transmission_heat    ? 1 : 0,
    dtc_new_count:        w.dtc_new.length,
  };
  if (signal in direct) return direct[signal];
  if (signal.startsWith('signals_snapshot.')) {
    const field = signal.slice('signals_snapshot.'.length);
    const val = (w.signals_snapshot as any)[field];
    return typeof val === 'number' ? val : undefined;
  }
  return undefined;
}

function checkOp(value: number, op: string, threshold: number): boolean {
  switch (op) {
    case 'gt':       return value > threshold;
    case 'lt':       return value < threshold;
    case 'gte':      return value >= threshold;
    case 'lte':      return value <= threshold;
    case 'eq':       return value === threshold;
    case 'contains': return value >= threshold;
    default:         return false;
  }
}

interface TrustEntry { lastFiredTs: number }

class StubPipeline {
  private fe = new StubFeatureEngine();
  private trustState = new Map<string, TrustEntry>();
  private actionSeq = 0;
  private policy: PolicyPack;

  constructor(policy: PolicyPack) { this.policy = policy; }

  wire(): void {
    bus.subscribe<SignalEvent>('signals.raw', (env: EventEnvelope<SignalEvent>) => {
      const event = env.payload;
      const window = this.fe.ingest(event);
      bus.publish('signals.features', window);

      const rawDecisions: Decision[] = [];
      for (const rule of this.policy.rules) {
        const rawValue = resolveSignal(window, rule.signal);
        if (rawValue === undefined) continue;
        if (checkOp(rawValue, rule.operator, rule.threshold)) {
          rawDecisions.push({
            ts: window.ts, asset_id: window.asset_id,
            severity: rule.severity, rule_id: rule.id, rule_group: rule.group,
            confidence: 1.0, triggered_by: [rule.signal],
            raw_value: rawValue, threshold: rule.threshold,
            decision_source: 'RULE_ENGINE', context: window,
          });
        }
      }
      for (const d of rawDecisions) bus.publish('decisions', d);

      const gated: Decision[] = [];
      for (const d of rawDecisions) {
        const rule = this.policy.rules.find(r => r.id === d.rule_id);
        const cooldownMs = rule?.cooldown_ms ?? 0;
        const hysteresis = rule?.hysteresis  ?? 0;
        const key = `${d.asset_id}::${d.rule_id}`;
        const entry = this.trustState.get(key);
        if (entry !== undefined) {
          const elapsed = d.ts - entry.lastFiredTs;
          if (elapsed < cooldownMs) continue;
          if (d.raw_value < d.threshold + hysteresis) continue;
        }
        this.trustState.set(key, { lastFiredTs: d.ts });
        gated.push(d);
      }
      for (const d of gated) bus.publish('decisions.gated', d);

      for (const d of gated) {
        this.actionSeq++;
        const action: Action = {
          seq: this.actionSeq, ts: d.ts, asset_id: d.asset_id,
          severity: d.severity,
          title:    `${d.rule_group}: ${d.rule_id}`,
          guidance: `Reduce load and check ${d.triggered_by.join(', ')}. Current value ${d.raw_value.toFixed(1)} exceeds threshold ${d.threshold.toFixed(1)}.`,
          rule_id: d.rule_id,
          speak:   d.severity === 'HIGH' || d.severity === 'CRITICAL',
          acknowledged: false,
          source:  'TEMPLATE',
          decision_source: d.decision_source,
        };
        bus.publish('actions', action);
      }
    });
  }

  reset(): void {
    this.fe = new StubFeatureEngine();
    this.trustState.clear();
    this.actionSeq = 0;
  }
}

// ─── Scenario replay ──────────────────────────────────────────────────────────

function replayScenario(scenario: Scenario, speedMultiplier = 1): void {
  // speedMultiplier compresses ts_offset_ms (100 = 100x speed, offsets divided by 100)
  const baseTs = Date.now();
  for (const frame of scenario.frames) {
    const compressedOffset = Math.floor(frame.ts_offset_ms / speedMultiplier);
    const event: SignalEvent = {
      ts:        baseTs + compressedOffset,
      asset_id:  scenario.asset_id,
      driver_id: scenario.driver_id,
      source:    'SIMULATOR',
      signals:   frame.signals as any,
    };
    bus.publish('signals.raw', event);
  }
}

// ─── Shared state ─────────────────────────────────────────────────────────────

const policy = loadPolicy();
let pipeline = new StubPipeline(policy);

function resetAll(): void {
  bus.reset();
  pipeline.reset();
  pipeline = new StubPipeline(policy);
  pipeline.wire();
}

// ═══════════════════════════════════════════════════════════════════════════════
// TEST 1 — TrustEngine cooldown (Person A contract check)
// ─────────────────────────────────────────────────────────────────────────────
// Publishes a decision for the same rule_id at three timestamps:
//   t=0      → first fire (allowed — no prior state)
//   t=500ms  → within cooldown_ms → must be suppressed by TrustEngine
//   t=cooldown_ms + 1ms → past cooldown → must re-fire
//
// Uses coolant_overheat_critical: cooldown_ms=15000, threshold=108, hysteresis=1.0
// We drive coolant_temp to 112 (> 108 + 1.0) so hysteresis is never the blocker.
// ═══════════════════════════════════════════════════════════════════════════════

function test1_trustEngineCooldown(): TestResult {
  const name = '1. TrustEngine cooldown suppresses duplicates within cooldown_ms';
  try {
    resetAll();

    const gated: Decision[] = [];
    bus.subscribe<Decision>('decisions.gated', env => gated.push(env.payload));

    // Pick the rule with the clearest cooldown to test
    const ruleId = 'coolant_overheat_critical';
    const rule = policy.rules.find(r => r.id === ruleId);
    if (!rule) {
      return { name, pass: false, blocked: false, message: `Policy rule '${ruleId}' not found — check policy/policy.yaml` };
    }
    const { cooldown_ms, threshold, hysteresis = 0 } = rule;
    // Value well above threshold + hysteresis so that hysteresis never blocks re-fire
    const safeValue = threshold + hysteresis + 5;

    const baseTs = Date.now();
    const asset_id = 'TEST-COOLDOWN-001';

    // Publish three raw signals at carefully chosen timestamps.
    // We bypass the FeatureEngine here by publishing directly to signals.raw
    // with a coolant_temp that will cross the threshold immediately.

    function publishSignalAt(ts: number): void {
      const event: SignalEvent = {
        ts, asset_id, driver_id: 'DRV-TEST', source: 'SIMULATOR',
        signals: {
          coolant_temp:       safeValue,    // above threshold + hysteresis
          engine_rpm:         2200,
          vehicle_speed:      100,
          throttle_position:  45,
          engine_load:        55,
        },
      };
      bus.publish('signals.raw', event);
    }

    // Fire 1: t=0 → expect pass-through
    publishSignalAt(baseTs);

    // Fire 2: t = cooldown_ms/2 → expect suppressed
    publishSignalAt(baseTs + Math.floor(cooldown_ms / 2));

    // Fire 3: t = cooldown_ms + 1 → expect pass-through
    publishSignalAt(baseTs + cooldown_ms + 1);

    // Filter to only the target rule
    const targeted = gated.filter(d => d.rule_id === ruleId && d.asset_id === asset_id);

    if (targeted.length === 0) {
      return {
        name, pass: false, blocked: false,
        message: `No '${ruleId}' decisions reached decisions.gated at all. ` +
                 `Check StubPipeline or policy signal mapping for this rule. ` +
                 `Total gated events: ${gated.length}`,
      };
    }

    if (targeted.length === 1) {
      return {
        name, pass: false, blocked: false,
        message: `Only 1 firing — expected first fire at t=0 AND re-fire after cooldown. ` +
                 `The third publish at t=${cooldown_ms + 1}ms should have passed through. ` +
                 `Timestamps received: [${targeted.map(d => d.ts - baseTs).join(', ')}]ms`,
      };
    }

    if (targeted.length > 2) {
      // The mid-window publish should have been suppressed
      return {
        name, pass: false, blocked: false,
        message: `Expected exactly 2 firings (first + re-fire after cooldown). Got ${targeted.length}. ` +
                 `The publish at t=${Math.floor(cooldown_ms / 2)}ms was NOT suppressed. ` +
                 `Offsets from base: [${targeted.map(d => d.ts - baseTs).join(', ')}]ms`,
      };
    }

    // exactly 2: verify timing
    const [first, second] = targeted;
    const gap = second.ts - first.ts;

    if (gap < cooldown_ms) {
      return {
        name, pass: false, blocked: false,
        message: `Two firings but gap=${gap}ms is less than cooldown_ms=${cooldown_ms}ms. ` +
                 `Cooldown not enforced correctly.`,
      };
    }

    return {
      name, pass: true, blocked: false,
      message: `rule=${ruleId}: fired at t+0, mid-window suppressed, re-fired after ${gap}ms gap (cooldown_ms=${cooldown_ms})`,
    };
  } catch (e: any) {
    return { name, pass: false, blocked: false, message: `Error: ${e.message}` };
  }
}

// ═══════════════════════════════════════════════════════════════════════════════
// TEST 2 — Flutter LocalQueue simulation (STUB — BLOCKED on Person B)
// ─────────────────────────────────────────────────────────────────────────────
// Validates the SQLite queue contract Person B must implement.
// When Person B ships LocalQueueAdapter, this test wires to it instead.
// ═══════════════════════════════════════════════════════════════════════════════

function test2_flutterLocalQueue(): TestResult {
  const name = '2. Flutter LocalQueue: readBatch + markSynced contract';
  return {
    name,
    pass:    false,
    blocked: true,
    message: 'BLOCKED — waiting for Person B Flutter LocalQueue (SQLite readBatch/markSynced)',
  };
}

// ═══════════════════════════════════════════════════════════════════════════════
// TEST 3 — TTS flag correctness (per-severity granularity)
// ─────────────────────────────────────────────────────────────────────────────
// Validates speak flag logic at the Action level across two scenarios:
//   harsh_brake_city  → expected WARN actions  (speak must be false)
//   overheat_highway  → expected HIGH+CRITICAL  (speak must be true)
//
// This is more granular than phase0 test5: it checks every action individually
// and reports the first mismatching rule_id + severity combo.
// ═══════════════════════════════════════════════════════════════════════════════

function test3_ttsFlagPerSeverity(): TestResult {
  const name = '3. TTS speak flag: WARN/WATCH/NORMAL=false, HIGH/CRITICAL=true (per-severity)';
  try {
    const allActions: Action[] = [];

    const scenarios = [
      'harsh_brake_city',   // produces WARN actions
      'overheat_highway',   // produces HIGH + CRITICAL
    ];

    for (const s of scenarios) {
      resetAll();
      bus.subscribe<Action>('actions', env => allActions.push(env.payload));
      replayScenario(loadScenario(s));
    }

    if (allActions.length === 0) {
      return {
        name, pass: false, blocked: false,
        message: 'No actions produced from harsh_brake_city or overheat_highway. ' +
                 'Cannot verify TTS flag. Check StubPipeline rule evaluation.',
      };
    }

    const speakTrue:  Severity[] = ['HIGH', 'CRITICAL'];
    const speakFalse: Severity[] = ['NORMAL', 'WATCH', 'WARN'];

    const errors: string[] = [];
    for (const a of allActions) {
      const shouldSpeak = speakTrue.includes(a.severity);
      if (a.speak !== shouldSpeak) {
        errors.push(`${a.rule_id}(${a.severity}): speak=${a.speak}, expected=${shouldSpeak}`);
      }
    }

    if (errors.length === 0) {
      const bySeverity = new Map<Severity, number>();
      for (const a of allActions) {
        bySeverity.set(a.severity, (bySeverity.get(a.severity) ?? 0) + 1);
      }
      const summary = [...bySeverity.entries()]
        .map(([s, n]) => `${s}×${n}`)
        .join(', ');
      return {
        name, pass: true, blocked: false,
        message: `${allActions.length} actions, all speak flags correct. Distribution: ${summary}`,
      };
    }

    return {
      name, pass: false, blocked: false,
      message: `${errors.length}/${allActions.length} speak-flag mismatches: ${errors.slice(0, 3).join('; ')}`,
    };
  } catch (e: any) {
    return { name, pass: false, blocked: false, message: `Error: ${e.message}` };
  }
}

// ═══════════════════════════════════════════════════════════════════════════════
// TEST 4 — Offline banner / decision_source field (STUB — BLOCKED on Person B)
// ─────────────────────────────────────────────────────────────────────────────
// Will verify Flutter shows offline banner when network is down.
// Requires Person B to expose an offline-mode event or hook.
// ═══════════════════════════════════════════════════════════════════════════════

function test4_offlineBanner(): TestResult {
  const name = '4. Offline banner shown when network unavailable (Flutter)';
  return {
    name,
    pass:    false,
    blocked: true,
    message: 'BLOCKED — waiting for Person B offline mode / network-state hook',
  };
}

// ═══════════════════════════════════════════════════════════════════════════════
// TEST 5 — SyncAgent mock upload (STUB — BLOCKED on Person B + cloud-api)
// ─────────────────────────────────────────────────────────────────────────────
// Will verify SyncAgent batches decisions.gated events and POSTs to cloud-api.
// Requires Person B SyncAgent implementation and cloud-api /api/v1/sync endpoint.
// ═══════════════════════════════════════════════════════════════════════════════

function test5_syncAgentUpload(): TestResult {
  const name = '5. SyncAgent batches events and POSTs to cloud-api /api/v1/sync';
  return {
    name,
    pass:    false,
    blocked: true,
    message: 'BLOCKED — waiting for Person B SyncAgent + cloud-api /api/v1/sync wiring',
  };
}

// ═══════════════════════════════════════════════════════════════════════════════
// TEST 6 — p95 end-to-end latency < 200ms (stricter than phase0's 500ms)
// ─────────────────────────────────────────────────────────────────────────────
// Measures the latency recorded by EventBus for the 'actions' topic.
// In Phase 1 all processing is synchronous + in-process, so p95 should be
// in the sub-millisecond range. When Person A adds ONNX inference this will
// be re-validated — the 200ms target leaves headroom for async inference.
// ═══════════════════════════════════════════════════════════════════════════════

function test6_p95Latency200ms(): TestResult {
  const name = '6. p95 actions latency < 200ms (Tier-3 only; re-validate after ONNX)';
  try {
    resetAll();

    const actions: Action[] = [];
    bus.subscribe<Action>('actions', env => actions.push(env.payload));

    // Run all available scenarios to get a representative latency distribution
    const scenarioNames = [
      'overheat_highway',
      'harsh_brake_city',
      'cold_start_normal',
      'oscillating_fault',
      'heavy_equipment_hydraulic',
    ];

    const scenarioLatencies: { name: string; p95: number; count: number }[] = [];

    for (const sName of scenarioNames) {
      // Fresh bus metrics per scenario so we measure each independently
      bus.reset();
      pipeline.reset();
      pipeline = new StubPipeline(policy);
      pipeline.wire();

      const perScenarioActions: Action[] = [];
      bus.subscribe<Action>('actions', env => {
        perScenarioActions.push(env.payload);
        actions.push(env.payload);
      });

      replayScenario(loadScenario(sName), 100);

      const metrics = bus.getMetrics();
      const m = metrics['actions'];
      const p95 = m?.p95 ?? 0;
      const count = m?.count ?? 0;
      scenarioLatencies.push({ name: sName, p95, count });
    }

    // Log per-scenario latencies for debuggability
    console.log(dim('           Scenario latencies (actions topic):'));
    for (const s of scenarioLatencies) {
      const marker = s.count === 0 ? ' (no actions)' : '';
      const latencyStr = s.p95 >= 200 ? yellow(`p95=${s.p95.toFixed(3)}ms`) : `p95=${s.p95.toFixed(3)}ms`;
      console.log(dim(`             ${s.name.padEnd(30)} ${latencyStr}  n=${s.count}${marker}`));
    }

    // Only assert on scenarios that actually produced actions
    const withActions = scenarioLatencies.filter(s => s.count > 0);

    if (withActions.length === 0) {
      return {
        name, pass: false, blocked: false,
        message: 'No action events produced by any scenario — cannot measure latency. ' +
                 'Check StubPipeline wiring.',
      };
    }

    const violations = withActions.filter(s => s.p95 >= 200);
    if (violations.length === 0) {
      const maxP95 = Math.max(...withActions.map(s => s.p95));
      return {
        name, pass: true, blocked: false,
        message: `All ${withActions.length} scenarios under 200ms. Max p95=${maxP95.toFixed(3)}ms`,
      };
    }

    const detail = violations.map(s => `${s.name}:p95=${s.p95.toFixed(3)}ms`).join(', ');
    return {
      name, pass: false, blocked: false,
      message: `${violations.length} scenario(s) exceeded 200ms p95: ${detail}`,
    };
  } catch (e: any) {
    return { name, pass: false, blocked: false, message: `Error: ${e.message}` };
  }
}

// ─── Gate runner ─────────────────────────────────────────────────────────────

async function runGate(): Promise<void> {
  console.log(bold('\n  Phase 1 Gate — 6 tests\n'));
  console.log(dim('  Legend:') + '  ' + green('✓ PASS') + '  ' + red('✗ FAIL') + '  ' + yellow('⊘ BLOCKED') + '\n');

  const results: TestResult[] = [
    test1_trustEngineCooldown(),
    test2_flutterLocalQueue(),
    test3_ttsFlagPerSeverity(),
    test4_offlineBanner(),
    test5_syncAgentUpload(),
    test6_p95Latency200ms(),
  ];

  for (const r of results) {
    if (r.blocked) {
      // YELLOW — blocked stub, not a failure
      console.log(`  ${yellow('⊘')} ${yellow('BLOCKED')}  ${r.name}`);
      console.log(`           ${yellow(r.message)}`);
    } else if (r.pass) {
      console.log(`  ${green('✓')} ${green('PASS')}     ${r.name}`);
      console.log(`           ${dim(r.message)}`);
    } else {
      console.log(`  ${red('✗')} ${red('FAIL')}     ${r.name}`);
      console.log(`           ${yellow(r.message)}`);
    }
    console.log('');
  }

  const blocked    = results.filter(r => r.blocked).length;
  const nonBlocked = results.filter(r => !r.blocked);
  const passed     = nonBlocked.filter(r => r.pass).length;
  const failed     = nonBlocked.filter(r => !r.pass).length;
  const total      = results.length;

  // Summary line
  const passingStr = `${passed}/${total - blocked} non-blocked tests passing`;
  const blockedStr = blocked > 0 ? ` (${blocked} blocked)` : '';

  if (failed === 0) {
    console.log(green(bold(`  ${passingStr}${blockedStr} — GATE GREEN\n`)));
  } else {
    console.log(red(bold(`  ${passingStr}${blockedStr} — GATE RED (${failed} failing)\n`)));
  }

  // Exit 0 only if no non-blocked tests are failing
  process.exit(failed === 0 ? 0 : 1);
}

runGate();
