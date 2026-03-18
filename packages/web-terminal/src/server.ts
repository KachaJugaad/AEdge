// packages/web-terminal/src/server.ts
// Entry point: runs the full AnomEdge pipeline + WebSocket bridge together.
//
// Usage:
//   ts-node src/server.ts
//   ts-node src/server.ts --scenario overheat_highway
//   ts-node src/server.ts --scenario overheat_highway --port 4200
//
// The server loads policy, wires the pipeline, starts the WS bridge on port
// 4200 (configurable via --port), then optionally replays a scenario with
// real-time delays derived from ts_offset_ms in each frame.

import fs   from 'node:fs';
import path from 'node:path';

import { EventBus }       from '@anomedge/bus';
// @ts-ignore — @anomedge/core has no declaration file (build is --noEmit)
import { createPipeline } from '@anomedge/core';
import type { PolicyPack, SignalEvent } from '@anomedge/contracts';

import { startBridge, stopBridge } from './ws-bridge';

// ─── Paths ────────────────────────────────────────────────────────────────────

const ROOT         = path.resolve(__dirname, '../../../');
const POLICY_FILE  = path.join(ROOT, 'policy', 'policy.yaml');
const SCENARIO_DIR = path.join(ROOT, 'scenarios');

// ─── CLI args ─────────────────────────────────────────────────────────────────

function parseArgs(): { scenarioName: string | null; port: number } {
  const argv = process.argv.slice(2);

  const scenarioIdx = argv.indexOf('--scenario');
  const scenarioName = scenarioIdx >= 0 ? argv[scenarioIdx + 1] ?? null : null;

  const portIdx = argv.indexOf('--port');
  const port = portIdx >= 0 ? parseInt(argv[portIdx + 1] ?? '4200', 10) : 4200;

  return { scenarioName, port };
}

// ─── Minimal YAML → PolicyPack parser (same as demo.ts) ──────────────────────

function parseMinimalYaml(yaml: string): PolicyPack {
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

  return { version: '1.0', vehicle_class: 'SIMULATOR', rules } as PolicyPack;
}

// ─── Scenario types ───────────────────────────────────────────────────────────

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

// ─── Scenario replay ──────────────────────────────────────────────────────────

/**
 * Replay a scenario file with real-time delays.
 *
 * Each frame's ts_offset_ms is used as the absolute publish time relative to
 * the scenario start. Frames are delivered in order; the server waits for the
 * delta between consecutive frame offsets before publishing the next one.
 */
async function replayScenario(scenarioName: string, bus: EventBus): Promise<void> {
  const scenarioFile = path.join(SCENARIO_DIR, `${scenarioName}.json`);

  if (!fs.existsSync(scenarioFile)) {
    console.error(`[server] scenario not found: ${scenarioFile}`);
    console.error(`[server] available: ${fs.readdirSync(SCENARIO_DIR).filter(f => f.endsWith('.json')).join(', ')}`);
    process.exit(1);
  }

  const scenario: Scenario = JSON.parse(fs.readFileSync(scenarioFile, 'utf8'));
  console.log(`[server] replaying scenario: ${scenario.name} (${scenario.frames.length} frames)`);

  const baseTs = Date.now();
  let prevOffset = 0;

  for (const frame of scenario.frames) {
    // Wait for the delta between this frame and the previous one
    const delay = frame.ts_offset_ms - prevOffset;
    if (delay > 0) {
      await sleep(delay);
    }
    prevOffset = frame.ts_offset_ms;

    const signals: Record<string, number> = {};
    for (const [k, v] of Object.entries(frame.signals)) {
      if (typeof v === 'number') signals[k] = v;
    }

    const signalEvent: SignalEvent = {
      ts:        baseTs + frame.ts_offset_ms,
      asset_id:  scenario.asset_id,
      driver_id: 'DRV-SERVER',
      source:    'SIMULATOR',
      signals,
    };

    bus.publish('signals.raw', signalEvent);
  }

  console.log(`[server] scenario complete: ${scenario.name}`);
}

function sleep(ms: number): Promise<void> {
  return new Promise(resolve => setTimeout(resolve, ms));
}

// ─── Entry point ─────────────────────────────────────────────────────────────

async function main(): Promise<void> {
  const { scenarioName, port } = parseArgs();

  // Load policy
  if (!fs.existsSync(POLICY_FILE)) {
    console.error(`[server] policy file not found: ${POLICY_FILE}`);
    process.exit(1);
  }
  const policyYaml = fs.readFileSync(POLICY_FILE, 'utf8');
  const policy = parseMinimalYaml(policyYaml);
  console.log(`[server] loaded policy — ${policy.rules.length} rules`);

  // Create bus and pipeline
  const bus = new EventBus();
  createPipeline(policy, bus);
  console.log('[server] pipeline ready');

  // Start WS bridge
  await startBridge(bus, port);

  // Graceful shutdown
  const shutdown = async () => {
    console.log('[server] shutting down...');
    await stopBridge();
    process.exit(0);
  };
  process.on('SIGINT',  () => { shutdown().catch(console.error); });
  process.on('SIGTERM', () => { shutdown().catch(console.error); });

  // Optionally replay a scenario
  if (scenarioName) {
    console.log(`[server] starting scenario replay in 1s: ${scenarioName}`);
    await sleep(1000); // give clients a moment to connect
    await replayScenario(scenarioName, bus);
  } else {
    console.log('[server] ready — waiting for events (connect a browser or push signals.raw to the bus)');
  }
}

main().catch((err) => {
  console.error('[server] fatal error:', err);
  process.exit(1);
});
