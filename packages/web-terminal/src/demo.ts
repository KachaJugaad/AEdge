//! packages/web-terminal/src/demo.ts
//! Colored CLI scenario runner for AnomEdge Phase 0.
//!
//! Usage:
//!   pnpm --filter @anomedge/web-terminal demo
//!   pnpm --filter @anomedge/web-terminal demo -- --scenario overheat_highway
//!   pnpm --filter @anomedge/web-terminal demo -- --scenario harsh_brake_city

import fs   from 'node:fs';
import path from 'node:path';

import type { Decision, EventEnvelope, PolicyPack, SignalEvent } from '@anomedge/contracts';
import { EventBus }        from '@anomedge/bus';
// @ts-ignore — @anomedge/core has no declaration file (build is --noEmit)
import { createPipeline }  from '@anomedge/core';

// ─── ANSI colours ─────────────────────────────────────────────────────────────

const C = {
  reset:    '\x1b[0m',
  bold:     '\x1b[1m',
  dim:      '\x1b[2m',
  red:      '\x1b[31m',
  yellow:   '\x1b[33m',
  cyan:     '\x1b[36m',
  green:    '\x1b[32m',
  magenta:  '\x1b[35m',
  white:    '\x1b[37m',
  bgRed:    '\x1b[41m',
  bgYellow: '\x1b[43m',
};

function severityColor(sev: string): string {
  switch (sev) {
    case 'CRITICAL': return C.bgRed + C.bold + C.white;
    case 'HIGH':     return C.red + C.bold;
    case 'WARN':     return C.yellow + C.bold;
    case 'LOW':      return C.cyan;
    case 'WATCH':    return C.dim;
    default:         return C.white;
  }
}

// ─── Scenario JSON shape ──────────────────────────────────────────────────────

interface ScenarioFrame {
  ts_offset_ms: number;
  signals: Record<string, number | boolean>;
}

interface Scenario {
  name:                  string;
  asset_id:              string;
  expected_alerts:       string[];
  expected_max_severity: string;
  frames:                ScenarioFrame[];
}

// ─── Main ────────────────────────────────────────────────────────────────────

const ROOT       = path.resolve(__dirname, '../../../');
const POLICY_DIR = path.join(ROOT, 'policy');
const SCENARIO_DIR = path.join(ROOT, 'scenarios');

function parseArgs(): { scenarioName: string; policyFile: string } {
  const args = process.argv.slice(2);
  const flagIdx = args.indexOf('--scenario');
  const scenarioName = flagIdx >= 0 ? args[flagIdx + 1] : 'overheat_highway';
  const policyFlagIdx = args.indexOf('--policy');
  const policyFile = policyFlagIdx >= 0
    ? args[policyFlagIdx + 1]
    : path.join(POLICY_DIR, 'policy.yaml');
  return { scenarioName, policyFile };
}

// Minimal YAML → PolicyPack parser for demo use.
// Full parsing is delegated to the Rust pipeline; here we just need PolicyPack
// for TypeScript pipeline construction.
function loadPolicy(yamlPath: string): PolicyPack {
  // We read it raw and use a tiny inline parser for the demo.
  // For production use serde_yaml on the Rust side.
  // TypeScript demo uses a bundled policy object instead.
  const raw = fs.readFileSync(yamlPath, 'utf8');
  return parseMinimalYaml(raw);
}

/** Parse just enough policy.yaml to build a PolicyPack. */
function parseMinimalYaml(yaml: string): PolicyPack {
  // Very minimal line-by-line state machine — not a general YAML parser.
  const rules: PolicyPack['rules'] = [];
  let current: Partial<PolicyPack['rules'][number]> | null = null;

  for (const line of yaml.split('\n')) {
    const trimmed = line.trim();
    if (trimmed.startsWith('- id:')) {
      if (current?.id) rules.push(current as PolicyPack['rules'][number]);
      current = { id: trimmed.slice(5).trim().replace(/['"]/g, '') };
    } else if (current && trimmed.startsWith('signal:')) {
      current.signal = trimmed.slice(7).trim().replace(/['"]/g, '');
    } else if (current && trimmed.startsWith('operator:')) {
      current.operator = trimmed.slice(9).trim().replace(/['"]/g, '') as never;
    } else if (current && trimmed.startsWith('threshold:')) {
      current.threshold = parseFloat(trimmed.slice(10).trim());
    } else if (current && trimmed.startsWith('severity:')) {
      current.severity = trimmed.slice(9).trim().replace(/['"]/g, '') as never;
    } else if (current && trimmed.startsWith('cooldown_ms:')) {
      current.cooldown_ms = parseInt(trimmed.slice(12).trim(), 10);
    } else if (current && trimmed.startsWith('hysteresis:')) {
      current.hysteresis = parseFloat(trimmed.slice(11).trim());
    } else if (current && trimmed.startsWith('group:')) {
      current.group = trimmed.slice(6).trim().replace(/['"]/g, '') as never;
    } else if (current && trimmed.startsWith('description:')) {
      current.description = trimmed.slice(12).trim().replace(/['"]/g, '');
    }
  }
  if (current?.id) rules.push(current as PolicyPack['rules'][number]);

  return {
    version:       '1.0',
    vehicle_class: 'SIMULATOR',
    rules,
  } as PolicyPack;
}

function buildSignalEvent(
  assetId: string,
  baseTs:  number,
  frame:   ScenarioFrame,
): SignalEvent {
  const s = frame.signals;
  const extra: Record<string, number> = {};
  for (const [k, v] of Object.entries(s)) {
    if (typeof v === 'number') extra[k] = v;
  }
  return {
    ts:        baseTs + frame.ts_offset_ms,
    asset_id:  assetId,
    driver_id: 'DRV-DEMO',
    source:    'SIMULATOR',
    signals:   extra,
  } as SignalEvent;
}

function severityRank(sev: string): number {
  return { WATCH: 0, LOW: 1, WARN: 2, HIGH: 3, CRITICAL: 4 }[sev] ?? -1;
}

async function runScenario(scenarioName: string, policyFile: string): Promise<boolean> {
  const scenarioFile = path.join(SCENARIO_DIR, `${scenarioName}.json`);
  const scenario: Scenario = JSON.parse(fs.readFileSync(scenarioFile, 'utf8'));
  const policy = loadPolicy(policyFile);

  const bus = new EventBus();
  createPipeline(policy, bus);

  const baseTs = Date.now();
  const firedRuleIds = new Set<string>();
  let maxSeverityRank = -1;
  let maxSeverity = '';

  // Collect gated decisions
  bus.subscribe<Decision>('decisions.gated', (envelope: EventEnvelope<Decision>) => {
    const d = envelope.payload;
    const color = severityColor(d.severity);
    const rank  = severityRank(d.severity);
    if (rank > maxSeverityRank) {
      maxSeverityRank = rank;
      maxSeverity     = d.severity;
    }
    firedRuleIds.add(d.rule_id);
    process.stdout.write(
      `  ${color}[ALERT]${C.reset} ` +
      `rule=${C.bold}${d.rule_id.padEnd(30)}${C.reset} ` +
      `sev=${color}${d.severity.padEnd(8)}${C.reset} ` +
      `raw=${d.raw_value.toFixed(2)}\n`,
    );
  });

  console.log(`\n${C.bold}=== AnomEdge Demo: ${scenario.name} ===${C.reset}`);
  console.log(`${C.dim}Policy:   ${policyFile}${C.reset}`);
  console.log(`${C.dim}Scenario: ${scenarioFile}${C.reset}`);
  console.log(`${C.dim}Frames:   ${scenario.frames.length}${C.reset}\n`);

  for (const frame of scenario.frames) {
    const event = buildSignalEvent(scenario.asset_id, baseTs, frame);
    bus.publish('signals.raw', event);
  }

  console.log(`\n${C.bold}--- Results ---${C.reset}`);
  console.log(`Fired rules:  ${C.cyan}${[...firedRuleIds].join(', ') || '(none)'}${C.reset}`);
  console.log(`Max severity: ${severityColor(maxSeverity)}${maxSeverity || '(none)'}${C.reset}`);

  console.log(`\n${C.bold}--- Expectations ---${C.reset}`);
  console.log(`Expected rules: ${C.dim}${scenario.expected_alerts.join(', ')}${C.reset}`);
  console.log(`Expected max:   ${C.dim}${scenario.expected_max_severity}${C.reset}`);

  let passed = true;
  for (const expected of scenario.expected_alerts) {
    if (!firedRuleIds.has(expected)) {
      console.log(`${C.red}FAIL: rule '${expected}' did not fire${C.reset}`);
      passed = false;
    }
  }
  if (scenario.expected_max_severity && maxSeverity !== scenario.expected_max_severity) {
    console.log(
      `${C.red}FAIL: expected max severity '${scenario.expected_max_severity}', ` +
      `got '${maxSeverity}'${C.reset}`,
    );
    passed = false;
  }

  if (passed) {
    console.log(`\n${C.green}${C.bold}PASS — all expectations met${C.reset}`);
  } else {
    console.log(`\n${C.red}${C.bold}FAIL — see above${C.reset}`);
  }

  return passed;
}

// ─── Entry point ──────────────────────────────────────────────────────────────

(async () => {
  const { scenarioName, policyFile } = parseArgs();

  // If --all flag given, run every scenario
  if (process.argv.includes('--all')) {
    const scenarios = [
      'overheat_highway',
      'harsh_brake_city',
      'cold_start_normal',
      'oscillating_fault',
      'heavy_equipment_hydraulic',
    ];
    let allPassed = true;
    for (const name of scenarios) {
      const ok = await runScenario(name, policyFile);
      if (!ok) allPassed = false;
    }
    process.exit(allPassed ? 0 : 1);
  } else {
    const ok = await runScenario(scenarioName, policyFile);
    process.exit(ok ? 0 : 1);
  }
})();
