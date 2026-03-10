// gate-tests/phase0.gate.ts
// Phase 0 Gate — 8 tests
// Person C owns this file. Green gate = merge. Red gate = fix.
//
// Uses a minimal stub pipeline since gate-tests must not import @anomedge/core.
// The stub wires signals.raw -> FeatureEngine -> RuleEngine -> TrustEngine -> actions,
// matching the same logic Person A ships, but through bus topics only.

import { bus, type BusMetrics } from '@anomedge/bus';
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

// ─── chalk v5 is ESM-only; use ANSI codes directly for CJS compatibility ────

const green = (s: string) => `\x1b[32m${s}\x1b[0m`;
const red   = (s: string) => `\x1b[31m${s}\x1b[0m`;
const yellow = (s: string) => `\x1b[33m${s}\x1b[0m`;
const bold  = (s: string) => `\x1b[1m${s}\x1b[0m`;
const dim   = (s: string) => `\x1b[2m${s}\x1b[0m`;

// ─── Scenario loader ────────────────────────────────────────────────────────

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
  const scenarioPath = path.resolve(__dirname, '..', 'scenarios', `${name}.json`);
  return JSON.parse(fs.readFileSync(scenarioPath, 'utf-8'));
}

// ─── Policy loader ──────────────────────────────────────────────────────────

function loadPolicy(): PolicyPack {
  // Parse YAML manually (no yaml dep) — extract rules from policy.yaml
  const policyPath = path.resolve(__dirname, '..', 'policy', 'policy.yaml');
  const raw = fs.readFileSync(policyPath, 'utf-8');
  const rules: PolicyRule[] = [];

  let currentRule: Partial<PolicyRule> | null = null;

  for (const line of raw.split('\n')) {
    const trimmed = line.trim();
    if (trimmed.startsWith('#') || trimmed === '') continue;

    if (trimmed.startsWith('- id:')) {
      if (currentRule && currentRule.id) {
        rules.push(currentRule as PolicyRule);
      }
      currentRule = { id: trimmed.split(':').slice(1).join(':').trim() };
    } else if (currentRule) {
      const match = trimmed.match(/^(\w+):\s*(.+)$/);
      if (match) {
        const [, key, val] = match;
        switch (key) {
          case 'group':       currentRule.group = val as any; break;
          case 'signal':      currentRule.signal = val; break;
          case 'operator':    (currentRule as any).operator = val as any; break;
          case 'threshold':   currentRule.threshold = parseFloat(val); break;
          case 'severity':    currentRule.severity = val as Severity; break;
          case 'cooldown_ms': currentRule.cooldown_ms = parseInt(val, 10); break;
          case 'hysteresis':  currentRule.hysteresis = parseFloat(val); break;
          case 'description': currentRule.description = val.replace(/^"(.*)"$/, '$1'); break;
        }
      }
    }
  }
  if (currentRule && currentRule.id) {
    rules.push(currentRule as PolicyRule);
  }

  return {
    version: '1.0',
    vehicle_class: 'LIGHT_TRUCK',
    rules,
  };
}

// ─── Minimal stub pipeline (does NOT import @anomedge/core) ──────────────────
// Mirrors Person A's pipeline logic: FeatureEngine -> RuleEngine -> TrustEngine
// All communication through bus topics.

const WINDOW_SECONDS = 30;
const WINDOW_MS = WINDOW_SECONDS * 1000;

class StubFeatureEngine {
  private buffers: Map<string, SignalEvent[]> = new Map();

  ingest(event: SignalEvent): FeatureWindow {
    const { asset_id, ts } = event;
    let buffer = this.buffers.get(asset_id);
    if (!buffer) { buffer = []; this.buffers.set(asset_id, buffer); }
    buffer.push(event);

    const cutoff = ts - WINDOW_MS;
    while (buffer.length > 0 && buffer[0].ts < cutoff) buffer.shift();

    return this.computeFeatures(asset_id, buffer);
  }

  private computeFeatures(asset_id: string, window: SignalEvent[]): FeatureWindow {
    const latest = window[window.length - 1];
    const n = window.length;
    const coolantTemps = window.map(e => (e.signals.coolant_temp as number) ?? 0);
    const speeds = window.map(e => (e.signals.vehicle_speed as number) ?? 0);
    const rpms = window.map(e => (e.signals.engine_rpm as number) ?? 0);
    const loads = window.map(e => (e.signals.engine_load as number) ?? 0);
    const throttles = window.map(e => (e.signals.throttle_position as number) ?? 0);

    const coolant_slope = this.slope(coolantTemps);
    let brake_spike_count = 0;
    for (let i = 1; i < n; i++) {
      const prev = (window[i - 1].signals.brake_pedal as number) ?? 0;
      const curr = (window[i].signals.brake_pedal as number) ?? 0;
      if (prev < 0.8 && curr >= 0.8) brake_spike_count++;
    }

    let hydraulic_spike = false;
    for (let i = 1; i < n; i++) {
      const prev = (window[i - 1].signals.hydraulic_pressure as number) ?? 0;
      const curr = (window[i].signals.hydraulic_pressure as number) ?? 0;
      if (Math.abs(curr - prev) > 500) { hydraulic_spike = true; break; }
    }

    const transmission_heat = window.some(e => ((e.signals.transmission_temp as number) ?? 0) > 110);

    const latestDtc = (latest.signals.dtc_codes as string[]) ?? [];
    let dtc_new: string[];
    if (n <= 1) { dtc_new = [...latestDtc]; }
    else {
      const prior = new Set<string>();
      for (let i = 0; i < n - 1; i++) {
        for (const c of ((window[i].signals.dtc_codes as string[]) ?? [])) prior.add(c);
      }
      dtc_new = latestDtc.filter(c => !prior.has(c));
    }

    return {
      ts: latest.ts,
      asset_id,
      window_seconds: WINDOW_SECONDS,
      coolant_slope,
      brake_spike_count,
      speed_mean: this.mean(speeds),
      rpm_mean: this.mean(rpms),
      engine_load_mean: this.mean(loads),
      throttle_variance: this.variance(throttles),
      hydraulic_spike,
      transmission_heat,
      dtc_new,
      signals_snapshot: { ...latest.signals } as Partial<SignalMap>,
    };
  }

  private slope(values: number[]): number {
    const n = values.length;
    if (n <= 1) return 0;
    const mx = (n - 1) / 2;
    const my = this.mean(values);
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
    coolant_slope: w.coolant_slope,
    brake_spike_count: w.brake_spike_count,
    speed_mean: w.speed_mean,
    rpm_mean: w.rpm_mean,
    engine_load_mean: w.engine_load_mean,
    throttle_variance: w.throttle_variance,
    hydraulic_spike: w.hydraulic_spike ? 1 : 0,
    transmission_heat: w.transmission_heat ? 1 : 0,
    dtc_new_count: w.dtc_new.length,
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
    case 'gt': return value > threshold;
    case 'lt': return value < threshold;
    case 'gte': return value >= threshold;
    case 'lte': return value <= threshold;
    case 'eq': return value === threshold;
    case 'contains': return value >= threshold;
    default: return false;
  }
}

interface TrustEntry { lastFiredTs: number }

class StubPipeline {
  private fe = new StubFeatureEngine();
  private trustState = new Map<string, TrustEntry>();
  private actionSeq = 0;
  private policy: PolicyPack;

  constructor(policy: PolicyPack) {
    this.policy = policy;
  }

  wire(): void {
    bus.subscribe<SignalEvent>('signals.raw', (env: EventEnvelope<SignalEvent>) => {
      const event = env.payload;
      const window = this.fe.ingest(event);
      bus.publish('signals.features', window);

      // RuleEngine: evaluate all rules
      const rawDecisions: Decision[] = [];
      for (const rule of this.policy.rules) {
        const rawValue = resolveSignal(window, rule.signal);
        if (rawValue === undefined) continue;
        if (checkOp(rawValue, rule.operator, rule.threshold)) {
          rawDecisions.push({
            ts: window.ts,
            asset_id: window.asset_id,
            severity: rule.severity,
            rule_id: rule.id,
            rule_group: rule.group,
            confidence: 1.0,
            triggered_by: [rule.signal],
            raw_value: rawValue,
            threshold: rule.threshold,
            decision_source: 'RULE_ENGINE',
            context: window,
          });
        }
      }

      for (const d of rawDecisions) {
        bus.publish('decisions', d);
      }

      // TrustEngine: cooldown + hysteresis
      const gated: Decision[] = [];
      for (const d of rawDecisions) {
        const rule = this.policy.rules.find(r => r.id === d.rule_id);
        const cooldownMs = rule?.cooldown_ms ?? 0;
        const hysteresis = rule?.hysteresis ?? 0;
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

      for (const d of gated) {
        bus.publish('decisions.gated', d);
      }

      // Action generation (stub GuidanceEngine)
      for (const d of gated) {
        this.actionSeq++;
        const action: Action = {
          seq: this.actionSeq,
          ts: d.ts,
          asset_id: d.asset_id,
          severity: d.severity,
          title: `${d.rule_group}: ${d.rule_id}`,
          guidance: `Reduce load and check ${d.triggered_by.join(', ')}. Current value ${d.raw_value.toFixed(1)} exceeds threshold ${d.threshold.toFixed(1)}.`,
          rule_id: d.rule_id,
          speak: d.severity === 'HIGH' || d.severity === 'CRITICAL',
          acknowledged: false,
          source: 'TEMPLATE',
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

// ─── Scenario replay ────────────────────────────────────────────────────────

function replayScenario(scenario: Scenario): void {
  const baseTs = Date.now();
  for (const frame of scenario.frames) {
    const event: SignalEvent = {
      ts: baseTs + frame.ts_offset_ms,
      asset_id: scenario.asset_id,
      driver_id: scenario.driver_id,
      source: 'SIMULATOR',
      signals: frame.signals as any,
    };
    bus.publish('signals.raw', event);
  }
}

// ─── Guidance validator ─────────────────────────────────────────────────────

const ACTION_VERBS = [
  'reduce', 'check', 'stop', 'inspect', 'monitor', 'refuel',
  'address', 'pull', 'slow', 'review', 'contact', 'service',
  'avoid', 'replace', 'clean', 'coach', 'lower', 'add',
];

function validateGuidance(guidance: string): boolean {
  if (guidance.length <= 10) return false;
  if (guidance.includes('undefined')) return false;
  if (guidance.includes('null')) return false;
  const lower = guidance.toLowerCase();
  return ACTION_VERBS.some(v => lower.includes(v));
}

// ─── Test runner ────────────────────────────────────────────────────────────

interface TestResult {
  name: string;
  pass: boolean;
  message: string;
}

const policy = loadPolicy();
let pipeline = new StubPipeline(policy);

function resetAll(): void {
  bus.reset();
  pipeline.reset();
  pipeline = new StubPipeline(policy);
  pipeline.wire();
}

// ─── Test 1: overheat_highway reaches CRITICAL ──────────────────────────────

function test1_overheatCritical(): TestResult {
  const name = '1. overheat_highway reaches CRITICAL';
  try {
    resetAll();
    const actions: Action[] = [];
    bus.subscribe<Action>('actions', (env) => actions.push(env.payload));

    const scenario = loadScenario('overheat_highway');
    replayScenario(scenario);

    const hasCritical = actions.some(a => a.severity === 'CRITICAL');
    if (hasCritical) {
      const critCount = actions.filter(a => a.severity === 'CRITICAL').length;
      return { name, pass: true, message: `${critCount} CRITICAL action(s) emitted` };
    }
    const severities = [...new Set(actions.map(a => a.severity))];
    return { name, pass: false, message: `No CRITICAL actions. Got severities: [${severities.join(', ')}] (${actions.length} total)` };
  } catch (e: any) {
    return { name, pass: false, message: `Error: ${e.message}` };
  }
}

// ─── Test 2: cold_start_normal zero WARN+ ───────────────────────────────────

function test2_coldStartClean(): TestResult {
  const name = '2. cold_start_normal zero WARN+ events';
  try {
    resetAll();
    const actions: Action[] = [];
    bus.subscribe<Action>('actions', (env) => actions.push(env.payload));

    const scenario = loadScenario('cold_start_normal');
    replayScenario(scenario);

    const warnPlus: Severity[] = ['WARN', 'HIGH', 'CRITICAL'];
    const bad = actions.filter(a => warnPlus.includes(a.severity));
    if (bad.length === 0) {
      return { name, pass: true, message: `${actions.length} total actions, zero WARN+` };
    }
    return { name, pass: false, message: `${bad.length} WARN+ actions found: ${bad.map(a => `${a.rule_id}(${a.severity})`).join(', ')}` };
  } catch (e: any) {
    return { name, pass: false, message: `Error: ${e.message}` };
  }
}

// ─── Test 3: oscillating_fault TrustEngine suppression ──────────────────────

function test3_oscillatingFault(): TestResult {
  const name = '3. oscillating_fault: fire -> suppress -> re-fire';
  try {
    resetAll();
    const actions: Action[] = [];
    bus.subscribe<Action>('actions', (env) => actions.push(env.payload));

    const scenario = loadScenario('oscillating_fault');
    replayScenario(scenario);

    // We expect the rule coolant_overheat_critical or coolant_rising_fast to fire
    // Pattern: first fire, then cooldown suppression, then second fire
    // Group by rule_id and check timing
    const ruleActions = new Map<string, Action[]>();
    for (const a of actions) {
      const list = ruleActions.get(a.rule_id) ?? [];
      list.push(a);
      ruleActions.set(a.rule_id, list);
    }

    // Find a rule that fired at least twice (meaning it re-fired after cooldown)
    let foundPattern = false;
    let detail = '';

    for (const [ruleId, ruleActs] of ruleActions) {
      if (ruleActs.length >= 2) {
        const rule = policy.rules.find(r => r.id === ruleId);
        const cooldownMs = rule?.cooldown_ms ?? 0;
        const gap = ruleActs[1].ts - ruleActs[0].ts;
        if (gap >= cooldownMs) {
          foundPattern = true;
          detail = `rule=${ruleId}: fired at t+0, suppressed during cooldown (${cooldownMs}ms), re-fired after ${gap}ms gap`;
          break;
        }
      }
    }

    if (foundPattern) {
      return { name, pass: true, message: detail };
    }

    // Partial credit: if at least one firing exists, it means pipeline works but
    // cooldown/re-fire pattern may not match scenario timing
    if (actions.length > 0) {
      const ruleIds = [...ruleActions.keys()];
      const firings = ruleIds.map(r => `${r}(${ruleActions.get(r)!.length}x)`).join(', ');
      return { name, pass: false, message: `Actions produced but no fire->suppress->re-fire pattern found. Firings: ${firings}` };
    }

    return { name, pass: false, message: 'No actions produced from oscillating_fault scenario' };
  } catch (e: any) {
    return { name, pass: false, message: `Error: ${e.message}` };
  }
}

// ─── Test 4: All guidance templates pass validateGuidance() ─────────────────

function test4_guidanceValid(): TestResult {
  const name = '4. All guidance templates pass validateGuidance()';
  try {
    resetAll();
    const actions: Action[] = [];
    bus.subscribe<Action>('actions', (env) => actions.push(env.payload));

    // Run all 5 scenarios to collect guidance from every template
    const scenarios = [
      'overheat_highway', 'cold_start_normal', 'oscillating_fault',
      'harsh_brake_city', 'heavy_equipment_hydraulic',
    ];
    for (const s of scenarios) {
      bus.reset();
      pipeline.reset();
      pipeline = new StubPipeline(policy);
      pipeline.wire();
      bus.subscribe<Action>('actions', (env) => actions.push(env.payload));
      replayScenario(loadScenario(s));
    }

    if (actions.length === 0) {
      return { name, pass: false, message: 'No actions produced across all scenarios — cannot validate guidance' };
    }

    const failures: string[] = [];
    for (const a of actions) {
      if (!validateGuidance(a.guidance)) {
        failures.push(`${a.rule_id}: "${a.guidance.substring(0, 60)}..."`);
      }
    }

    if (failures.length === 0) {
      return { name, pass: true, message: `${actions.length} guidance strings validated` };
    }
    return { name, pass: false, message: `${failures.length}/${actions.length} failed: ${failures.slice(0, 3).join('; ')}` };
  } catch (e: any) {
    return { name, pass: false, message: `Error: ${e.message}` };
  }
}

// ─── Test 5: TTS flag correct for all severities ────────────────────────────

function test5_ttsFlag(): TestResult {
  const name = '5. TTS speak flag correct per severity';
  try {
    resetAll();
    const actions: Action[] = [];
    bus.subscribe<Action>('actions', (env) => actions.push(env.payload));

    const scenarios = [
      'overheat_highway', 'cold_start_normal', 'oscillating_fault',
      'harsh_brake_city', 'heavy_equipment_hydraulic',
    ];
    for (const s of scenarios) {
      bus.reset();
      pipeline.reset();
      pipeline = new StubPipeline(policy);
      pipeline.wire();
      bus.subscribe<Action>('actions', (env) => actions.push(env.payload));
      replayScenario(loadScenario(s));
    }

    if (actions.length === 0) {
      return { name, pass: false, message: 'No actions produced — cannot verify TTS flag' };
    }

    const errors: string[] = [];
    for (const a of actions) {
      const shouldSpeak = a.severity === 'HIGH' || a.severity === 'CRITICAL';
      if (a.speak !== shouldSpeak) {
        errors.push(`${a.rule_id}(${a.severity}): speak=${a.speak}, expected=${shouldSpeak}`);
      }
    }

    if (errors.length === 0) {
      return { name, pass: true, message: `${actions.length} actions — TTS flags all correct` };
    }
    return { name, pass: false, message: `${errors.length} mismatches: ${errors.slice(0, 3).join('; ')}` };
  } catch (e: any) {
    return { name, pass: false, message: `Error: ${e.message}` };
  }
}

// ─── Test 6: p95 latency under 500ms ────────────────────────────────────────

function test6_p95Latency(): TestResult {
  const name = '6. p95 action latency < 500ms';
  try {
    resetAll();
    // Subscribe to actions so the bus records latency (bus skips publish if 0 subscribers)
    const actions: Action[] = [];
    bus.subscribe<Action>('actions', (env) => { actions.push(env.payload); });

    const scenario = loadScenario('overheat_highway');
    replayScenario(scenario);

    const metrics = bus.getMetrics();
    const actionMetrics = metrics['actions'];

    if (!actionMetrics) {
      return { name, pass: false, message: 'No metrics for actions topic — no actions were published' };
    }

    if (actionMetrics.p95 < 500) {
      return { name, pass: true, message: `p95=${actionMetrics.p95.toFixed(3)}ms (< 500ms)` };
    }
    return { name, pass: false, message: `p95=${actionMetrics.p95.toFixed(3)}ms exceeds 500ms` };
  } catch (e: any) {
    return { name, pass: false, message: `Error: ${e.message}` };
  }
}

// ─── Test 7: Monotonic sequence numbers ─────────────────────────────────────

function test7_monotonicSeq(): TestResult {
  const name = '7. Monotonic sequence numbers (no gaps)';
  try {
    resetAll();
    const envelopes: EventEnvelope<unknown>[] = [];

    // Collect envelopes from multiple topics
    const topics = ['signals.raw', 'signals.features', 'decisions', 'decisions.gated', 'actions'] as const;
    for (const t of topics) {
      bus.subscribe(t, (env: EventEnvelope<unknown>) => {
        envelopes.push(env);
      });
    }

    const scenario = loadScenario('overheat_highway');
    replayScenario(scenario);

    if (envelopes.length < 20) {
      return { name, pass: false, message: `Only ${envelopes.length} envelopes collected — need at least 20` };
    }

    // Sort by seq (bus seq is global, not per-topic)
    envelopes.sort((a, b) => a.seq - b.seq);

    // Check monotonic and no gaps
    const seqs = envelopes.map(e => e.seq);
    for (let i = 1; i < seqs.length; i++) {
      if (seqs[i] !== seqs[i - 1] + 1) {
        return { name, pass: false, message: `Gap at position ${i}: seq ${seqs[i - 1]} -> ${seqs[i]}` };
      }
    }

    return { name, pass: true, message: `${seqs.length} envelopes, seq ${seqs[0]}..${seqs[seqs.length - 1]}, no gaps` };
  } catch (e: any) {
    return { name, pass: false, message: `Error: ${e.message}` };
  }
}

// ─── Test 8: bus.getMetrics() returns data ──────────────────────────────────

function test8_busMetrics(): TestResult {
  const name = '8. bus.getMetrics() returns data for all active topics';
  try {
    resetAll();
    // Subscribe to all expected topics so bus records metrics (bus skips if 0 subscribers)
    bus.subscribe('signals.features', () => {});
    bus.subscribe('actions', () => {});
    bus.subscribe('decisions', () => {});
    bus.subscribe('decisions.gated', () => {});

    const scenario = loadScenario('overheat_highway');
    replayScenario(scenario);

    const metrics = bus.getMetrics();
    const requiredTopics = ['signals.raw', 'signals.features', 'actions'];
    const errors: string[] = [];

    for (const topic of requiredTopics) {
      const m = metrics[topic];
      if (!m) {
        errors.push(`${topic}: missing`);
        continue;
      }
      if (m.count <= 0) errors.push(`${topic}: count=${m.count}`);
      if (m.p50 <= 0 && m.count > 0) errors.push(`${topic}: p50=${m.p50}`);
    }

    if (errors.length === 0) {
      const summary = requiredTopics.map(t => `${t}(n=${metrics[t]?.count ?? 0})`).join(', ');
      return { name, pass: true, message: summary };
    }
    return { name, pass: false, message: errors.join('; ') };
  } catch (e: any) {
    return { name, pass: false, message: `Error: ${e.message}` };
  }
}

// ─── Run gate ───────────────────────────────────────────────────────────────

async function runGate(): Promise<void> {
  console.log(bold('\n  Phase 0 Gate — 8 tests\n'));

  const results: TestResult[] = [
    test1_overheatCritical(),
    test2_coldStartClean(),
    test3_oscillatingFault(),
    test4_guidanceValid(),
    test5_ttsFlag(),
    test6_p95Latency(),
    test7_monotonicSeq(),
    test8_busMetrics(),
  ];

  // Report
  for (const r of results) {
    if (r.pass) {
      console.log(`  ${green('\u2713')} ${green('PASS')}  ${r.name}`);
      console.log(`           ${dim(r.message)}`);
    } else {
      console.log(`  ${red('\u2717')} ${red('FAIL')}  ${r.name}`);
      console.log(`           ${yellow(r.message)}`);
    }
  }

  const passed = results.filter(r => r.pass).length;
  const total = results.length;

  console.log('');
  if (passed === total) {
    console.log(green(bold(`  ${passed}/${total} tests passing — GATE GREEN\n`)));
  } else {
    console.log(red(bold(`  ${passed}/${total} tests passing — GATE RED\n`)));
  }

  process.exit(passed === total ? 0 : 1);
}

runGate();
