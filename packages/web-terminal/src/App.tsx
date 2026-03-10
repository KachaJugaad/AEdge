// packages/web-terminal/src/App.tsx
// AnomEdge — Decision Monitor
// Dark-themed real-time dashboard. No external CSS framework.
// Pipeline runs entirely in-browser: EventBus is in-process, no WebSocket needed.

import React, { useState, useEffect, useRef, useCallback } from 'react';
import type { Decision, EventEnvelope, PolicyPack, SignalEvent } from '@anomedge/contracts';
import { EventBus } from '@anomedge/bus';
// @ts-ignore — @anomedge/core has no declaration file (build is --noEmit)
import { createPipeline } from '@anomedge/core';
import type { BusMetrics } from '@anomedge/bus';

// ─── Types ────────────────────────────────────────────────────────────────────

interface ScenarioFrame {
  ts_offset_ms: number;
  signals: Record<string, number | boolean>;
}

interface ScenarioFile {
  name: string;
  asset_id: string;
  expected_alerts: string[];
  expected_max_severity: string;
  frames: ScenarioFrame[];
}

type ActiveTab = 'feed' | 'metrics' | 'pipeline';

// ─── Severity colour map ──────────────────────────────────────────────────────

const SEVERITY_STYLES: Record<string, { bg: string; color: string; border: string }> = {
  NORMAL:   { bg: '#21262d', color: '#8b949e', border: '#30363d' },
  WATCH:    { bg: '#0d2847', color: '#58a6ff', border: '#1f6feb' },
  WARN:     { bg: '#2d2000', color: '#e3b341', border: '#9e6a03' },
  HIGH:     { bg: '#3d0f0f', color: '#f97583', border: '#da3633' },
  CRITICAL: { bg: '#58001a', color: '#ffffff', border: '#f85149' },
};

const SEVERITY_ORDER: Record<string, number> = {
  NORMAL: 0, WATCH: 1, WARN: 2, HIGH: 3, CRITICAL: 4,
};

function getSeverityStyle(sev: string) {
  return SEVERITY_STYLES[sev] ?? SEVERITY_STYLES.NORMAL;
}

// ─── Minimal YAML parser (same logic as demo.ts) ──────────────────────────────

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

// ─── Sub-components ───────────────────────────────────────────────────────────

// Severity badge
function SeverityBadge({ severity }: { severity: string }) {
  const s = getSeverityStyle(severity);
  const isPulsing = severity === 'CRITICAL';
  return (
    <span
      style={{
        display: 'inline-block',
        padding: '2px 8px',
        borderRadius: '4px',
        fontSize: '11px',
        fontWeight: 700,
        fontFamily: 'monospace',
        background: s.bg,
        color: s.color,
        border: `1px solid ${s.border}`,
        letterSpacing: '0.05em',
        animation: isPulsing ? 'pulse 1.2s ease-in-out infinite' : 'none',
      }}
    >
      {severity}
    </span>
  );
}

// Live event feed
interface FeedEvent {
  envelope: EventEnvelope<Decision>;
  receivedAt: number;
}

function EventFeed({ events }: { events: FeedEvent[] }) {
  const bottomRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [events.length]);

  const last20 = events.slice(-20);

  if (last20.length === 0) {
    return (
      <div style={{ color: '#8b949e', textAlign: 'center', padding: '40px', fontFamily: 'monospace', fontSize: '13px' }}>
        No events yet. Select a scenario and click Run.
      </div>
    );
  }

  return (
    <div style={{ overflowX: 'auto' }}>
      <table style={{ width: '100%', borderCollapse: 'collapse', fontFamily: 'monospace', fontSize: '12px' }}>
        <thead>
          <tr style={{ borderBottom: '1px solid #30363d', color: '#8b949e', textAlign: 'left' }}>
            <th style={thStyle}>SEQ</th>
            <th style={thStyle}>TIME</th>
            <th style={thStyle}>ASSET</th>
            <th style={thStyle}>SEVERITY</th>
            <th style={thStyle}>RULE</th>
            <th style={thStyle}>RAW / THRESH</th>
            <th style={thStyle}>SOURCE</th>
          </tr>
        </thead>
        <tbody>
          {last20.map((item) => {
            const d = item.envelope.payload;
            const s = getSeverityStyle(d.severity);
            const time = new Date(d.ts).toLocaleTimeString('en-US', { hour12: false });
            return (
              <tr
                key={item.envelope.id}
                style={{
                  borderBottom: '1px solid #21262d',
                  background: 'transparent',
                  transition: 'background 0.2s',
                }}
                onMouseEnter={e => (e.currentTarget.style.background = '#161b22')}
                onMouseLeave={e => (e.currentTarget.style.background = 'transparent')}
              >
                <td style={{ ...tdStyle, color: '#8b949e' }}>{item.envelope.seq}</td>
                <td style={{ ...tdStyle, color: '#8b949e' }}>{time}</td>
                <td style={{ ...tdStyle, color: '#e6edf3', fontWeight: 600 }}>{d.asset_id}</td>
                <td style={tdStyle}><SeverityBadge severity={d.severity} /></td>
                <td style={{ ...tdStyle, color: s.color }}>{d.rule_id}</td>
                <td style={{ ...tdStyle, color: '#8b949e' }}>
                  <span style={{ color: s.color, fontWeight: 600 }}>{d.raw_value.toFixed(2)}</span>
                  <span style={{ color: '#484f58' }}> / </span>
                  <span>{d.threshold.toFixed(2)}</span>
                </td>
                <td style={{ ...tdStyle, color: '#6e7681', fontSize: '10px' }}>{d.decision_source}</td>
              </tr>
            );
          })}
        </tbody>
      </table>
      <div ref={bottomRef} />
    </div>
  );
}

const thStyle: React.CSSProperties = {
  padding: '8px 12px',
  fontWeight: 600,
  fontSize: '10px',
  letterSpacing: '0.08em',
  textTransform: 'uppercase',
};

const tdStyle: React.CSSProperties = {
  padding: '7px 12px',
  verticalAlign: 'middle',
};

// Bus metrics table
function MetricsTable({ metrics }: { metrics: BusMetrics }) {
  const topics = Object.keys(metrics);

  if (topics.length === 0) {
    return (
      <div style={{ color: '#8b949e', textAlign: 'center', padding: '40px', fontFamily: 'monospace', fontSize: '13px' }}>
        No metrics yet. Run a scenario first.
      </div>
    );
  }

  return (
    <div style={{ overflowX: 'auto' }}>
      <table style={{ width: '100%', borderCollapse: 'collapse', fontFamily: 'monospace', fontSize: '12px' }}>
        <thead>
          <tr style={{ borderBottom: '1px solid #30363d', color: '#8b949e', textAlign: 'left' }}>
            <th style={thStyle}>TOPIC</th>
            <th style={{ ...thStyle, textAlign: 'right' }}>COUNT</th>
            <th style={{ ...thStyle, textAlign: 'right' }}>p50 ms</th>
            <th style={{ ...thStyle, textAlign: 'right' }}>p95 ms</th>
            <th style={{ ...thStyle, textAlign: 'right' }}>p99 ms</th>
          </tr>
        </thead>
        <tbody>
          {topics.map((topic) => {
            const m = metrics[topic];
            return (
              <tr
                key={topic}
                style={{ borderBottom: '1px solid #21262d' }}
                onMouseEnter={e => (e.currentTarget.style.background = '#161b22')}
                onMouseLeave={e => (e.currentTarget.style.background = 'transparent')}
              >
                <td style={{ ...tdStyle, color: '#58a6ff' }}>{topic}</td>
                <td style={{ ...tdStyle, color: '#e6edf3', textAlign: 'right' }}>{m.count}</td>
                <td style={{ ...tdStyle, color: '#3fb950', textAlign: 'right' }}>{m.p50.toFixed(3)}</td>
                <td style={{ ...tdStyle, color: '#e3b341', textAlign: 'right' }}>{m.p95.toFixed(3)}</td>
                <td style={{ ...tdStyle, color: '#f97583', textAlign: 'right' }}>{m.p99.toFixed(3)}</td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}

// Pipeline stage indicators
interface PipelineCounts {
  'signals.raw': number;
  'signals.features': number;
  'decisions': number;
  'decisions.gated': number;
}

function PipelinePanel({ counts }: { counts: PipelineCounts }) {
  const stages = [
    { label: 'signals.raw', key: 'signals.raw' as const, color: '#58a6ff' },
    { label: 'signals.features', key: 'signals.features' as const, color: '#3fb950' },
    { label: 'decisions', key: 'decisions' as const, color: '#e3b341' },
    { label: 'decisions.gated', key: 'decisions.gated' as const, color: '#f97583' },
  ];

  return (
    <div style={{ display: 'flex', gap: '16px', flexWrap: 'wrap', padding: '16px 0' }}>
      {stages.map((stage, i) => (
        <React.Fragment key={stage.key}>
          <div style={{
            flex: '1 1 160px',
            background: '#161b22',
            border: '1px solid #30363d',
            borderRadius: '8px',
            padding: '16px',
            textAlign: 'center',
          }}>
            <div style={{ fontFamily: 'monospace', fontSize: '10px', color: '#6e7681', letterSpacing: '0.06em', marginBottom: '8px', textTransform: 'uppercase' }}>
              {stage.label}
            </div>
            <div style={{ fontFamily: 'monospace', fontSize: '32px', fontWeight: 700, color: stage.color }}>
              {counts[stage.key]}
            </div>
            <div style={{ fontFamily: 'monospace', fontSize: '10px', color: '#484f58', marginTop: '4px' }}>
              messages
            </div>
          </div>
          {i < stages.length - 1 && (
            <div style={{ display: 'flex', alignItems: 'center', color: '#30363d', fontSize: '20px', flexShrink: 0 }}>
              →
            </div>
          )}
        </React.Fragment>
      ))}
    </div>
  );
}

// ─── Main App ─────────────────────────────────────────────────────────────────

const SCENARIOS = [
  'overheat_highway',
  'harsh_brake_city',
  'cold_start_normal',
  'oscillating_fault',
  'heavy_equipment_hydraulic',
] as const;

type ScenarioName = typeof SCENARIOS[number];

export default function App() {
  const [clock, setClock] = useState(() => new Date().toLocaleTimeString('en-US', { hour12: false }));
  const [scenario, setScenario] = useState<ScenarioName>('overheat_highway');
  const [activeTab, setActiveTab] = useState<ActiveTab>('feed');
  const [running, setRunning] = useState(false);
  const [status, setStatus] = useState<string>('Ready');
  const [events, setEvents] = useState<FeedEvent[]>([]);
  const [metrics, setMetrics] = useState<BusMetrics>({});
  const [pipelineCounts, setPipelineCounts] = useState<PipelineCounts>({
    'signals.raw': 0,
    'signals.features': 0,
    'decisions': 0,
    'decisions.gated': 0,
  });
  const [maxSeverity, setMaxSeverity] = useState<string>('—');
  const [totalEvents, setTotalEvents] = useState(0);
  const [rulesFired, setRulesFired] = useState<Set<string>>(new Set());

  const busRef = useRef<EventBus | null>(null);
  const metricsTimerRef = useRef<ReturnType<typeof setInterval> | null>(null);

  // Live clock
  useEffect(() => {
    const t = setInterval(() => {
      setClock(new Date().toLocaleTimeString('en-US', { hour12: false }));
    }, 1000);
    return () => clearInterval(t);
  }, []);

  // Refresh metrics every 2 seconds when bus is active
  useEffect(() => {
    if (metricsTimerRef.current) clearInterval(metricsTimerRef.current);
    metricsTimerRef.current = setInterval(() => {
      if (busRef.current) {
        setMetrics(busRef.current.getMetrics());
      }
    }, 2000);
    return () => {
      if (metricsTimerRef.current) clearInterval(metricsTimerRef.current);
    };
  }, []);

  const runScenario = useCallback(async () => {
    if (running) return;
    setRunning(true);
    setStatus(`Loading scenario: ${scenario}…`);

    // Reset state
    setEvents([]);
    setMaxSeverity('—');
    setTotalEvents(0);
    setRulesFired(new Set());
    setPipelineCounts({ 'signals.raw': 0, 'signals.features': 0, 'decisions': 0, 'decisions.gated': 0 });

    try {
      // Fetch scenario JSON (served from public/ = scenarios/ root)
      const [scenarioRes, policyRes] = await Promise.all([
        fetch(`/scenarios/${scenario}.json`),
        fetch('/policy/policy.yaml'),
      ]);

      if (!scenarioRes.ok) throw new Error(`Scenario not found: scenarios/${scenario}.json (status ${scenarioRes.status})`);
      if (!policyRes.ok) throw new Error(`Policy not found: policy/policy.yaml (status ${policyRes.status})`);

      const scenarioData: ScenarioFile = await scenarioRes.json();
      const policyYaml: string = await policyRes.text();
      const policy = parseMinimalYaml(policyYaml);

      setStatus(`Running: ${scenarioData.name} — ${scenarioData.frames.length} frames…`);

      // Fresh bus instance for each run
      const bus = new EventBus();
      busRef.current = bus;
      createPipeline(policy, bus);

      // Local counters (avoid stale closure issues with React state)
      const localCounts = { 'signals.raw': 0, 'signals.features': 0, 'decisions': 0, 'decisions.gated': 0 };
      let localMaxRank = -1;
      let localMaxSev = '—';
      const localRules = new Set<string>();
      let localTotal = 0;

      // Subscribe to all four pipeline topics for count tracking
      bus.subscribe('signals.raw', () => {
        localCounts['signals.raw']++;
        setPipelineCounts(prev => ({ ...prev, 'signals.raw': localCounts['signals.raw'] }));
      });

      bus.subscribe('signals.features', () => {
        localCounts['signals.features']++;
        setPipelineCounts(prev => ({ ...prev, 'signals.features': localCounts['signals.features'] }));
      });

      bus.subscribe('decisions', () => {
        localCounts['decisions']++;
        setPipelineCounts(prev => ({ ...prev, 'decisions': localCounts['decisions'] }));
      });

      bus.subscribe<Decision>('decisions.gated', (envelope: EventEnvelope<Decision>) => {
        localCounts['decisions.gated']++;
        localTotal++;
        setPipelineCounts(prev => ({ ...prev, 'decisions.gated': localCounts['decisions.gated'] }));

        const d = envelope.payload;
        const rank = SEVERITY_ORDER[d.severity] ?? 0;
        if (rank > localMaxRank) {
          localMaxRank = rank;
          localMaxSev = d.severity;
          setMaxSeverity(d.severity);
        }
        localRules.add(d.rule_id);
        setRulesFired(new Set(localRules));
        setTotalEvents(localTotal);
        setEvents(prev => [...prev, { envelope, receivedAt: Date.now() }]);
      });

      const baseTs = Date.now();

      // Publish all frames synchronously (the bus is in-process synchronous)
      for (const frame of scenarioData.frames) {
        const signals: Record<string, number> = {};
        for (const [k, v] of Object.entries(frame.signals)) {
          if (typeof v === 'number') signals[k] = v;
        }
        const signalEvent: SignalEvent = {
          ts: baseTs + frame.ts_offset_ms,
          asset_id: scenarioData.asset_id,
          driver_id: 'DRV-DEMO',
          source: 'SIMULATOR',
          signals,
        };
        bus.publish('signals.raw', signalEvent);
      }

      // Final metrics snapshot
      setMetrics(bus.getMetrics());
      setStatus(`Complete — ${scenarioData.name} (${scenarioData.frames.length} frames, max severity: ${localMaxSev})`);
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      setStatus(`Error: ${msg}`);
    } finally {
      setRunning(false);
    }
  }, [scenario, running]);

  const maxSevStyle = getSeverityStyle(maxSeverity === '—' ? 'NORMAL' : maxSeverity);

  return (
    <>
      <style>{`
        @keyframes pulse {
          0%, 100% { opacity: 1; }
          50% { opacity: 0.55; }
        }
        ::-webkit-scrollbar { width: 6px; height: 6px; }
        ::-webkit-scrollbar-track { background: #161b22; }
        ::-webkit-scrollbar-thumb { background: #30363d; border-radius: 3px; }
        ::-webkit-scrollbar-thumb:hover { background: #484f58; }
        * { scrollbar-width: thin; scrollbar-color: #30363d #161b22; }
      `}</style>

      <div style={{ minHeight: '100vh', background: '#0d1117', color: '#e6edf3', fontFamily: '-apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif' }}>

        {/* Header */}
        <header style={{
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'space-between',
          padding: '12px 24px',
          borderBottom: '1px solid #21262d',
          background: '#161b22',
          position: 'sticky',
          top: 0,
          zIndex: 100,
        }}>
          <div style={{ display: 'flex', alignItems: 'center', gap: '12px' }}>
            <div style={{
              width: '28px', height: '28px', borderRadius: '6px',
              background: 'linear-gradient(135deg, #58a6ff, #388bfd)',
              display: 'flex', alignItems: 'center', justifyContent: 'center',
              fontSize: '14px', fontWeight: 700, color: '#0d1117',
            }}>
              A
            </div>
            <span style={{ fontSize: '16px', fontWeight: 600, letterSpacing: '-0.02em' }}>
              AnomEdge
            </span>
            <span style={{ color: '#484f58', fontSize: '14px' }}>—</span>
            <span style={{ color: '#8b949e', fontSize: '14px' }}>Decision Monitor</span>
          </div>
          <div style={{ fontFamily: 'monospace', fontSize: '13px', color: '#8b949e' }}>
            {clock}
          </div>
        </header>

        {/* Control bar */}
        <div style={{
          display: 'flex',
          alignItems: 'center',
          gap: '12px',
          padding: '12px 24px',
          borderBottom: '1px solid #21262d',
          background: '#0d1117',
          flexWrap: 'wrap',
        }}>
          <label style={{ fontSize: '13px', color: '#8b949e', whiteSpace: 'nowrap' }}>Scenario:</label>
          <select
            value={scenario}
            onChange={e => setScenario(e.target.value as ScenarioName)}
            disabled={running}
            style={{
              background: '#161b22',
              color: '#e6edf3',
              border: '1px solid #30363d',
              borderRadius: '6px',
              padding: '6px 10px',
              fontSize: '13px',
              fontFamily: 'monospace',
              cursor: 'pointer',
              outline: 'none',
            }}
          >
            {SCENARIOS.map(s => (
              <option key={s} value={s}>{s}</option>
            ))}
          </select>

          <button
            onClick={runScenario}
            disabled={running}
            style={{
              background: running ? '#21262d' : 'linear-gradient(135deg, #238636, #2ea043)',
              color: running ? '#484f58' : '#fff',
              border: 'none',
              borderRadius: '6px',
              padding: '6px 16px',
              fontSize: '13px',
              fontWeight: 600,
              cursor: running ? 'not-allowed' : 'pointer',
              transition: 'all 0.15s',
              whiteSpace: 'nowrap',
            }}
          >
            {running ? 'Running…' : 'Run Scenario'}
          </button>

          <div style={{
            fontFamily: 'monospace',
            fontSize: '12px',
            color: status.startsWith('Error') ? '#f97583' : '#8b949e',
            flex: 1,
            minWidth: 0,
            overflow: 'hidden',
            textOverflow: 'ellipsis',
            whiteSpace: 'nowrap',
          }}>
            {status}
          </div>
        </div>

        {/* Main content */}
        <div style={{ padding: '20px 24px' }}>

          {/* Pipeline stage indicators */}
          <div style={{ marginBottom: '20px' }}>
            <div style={{ fontSize: '11px', color: '#484f58', letterSpacing: '0.08em', textTransform: 'uppercase', marginBottom: '10px', fontFamily: 'monospace' }}>
              Pipeline Stages
            </div>
            <PipelinePanel counts={pipelineCounts} />
          </div>

          {/* Tabs */}
          <div style={{ borderBottom: '1px solid #21262d', marginBottom: '0', display: 'flex', gap: '0' }}>
            {(['feed', 'metrics', 'pipeline'] as const).map(tab => (
              <button
                key={tab}
                onClick={() => setActiveTab(tab)}
                style={{
                  background: 'none',
                  border: 'none',
                  borderBottom: activeTab === tab ? '2px solid #58a6ff' : '2px solid transparent',
                  color: activeTab === tab ? '#e6edf3' : '#8b949e',
                  padding: '8px 16px',
                  fontSize: '13px',
                  fontWeight: activeTab === tab ? 600 : 400,
                  cursor: 'pointer',
                  transition: 'color 0.15s',
                  letterSpacing: '0.01em',
                }}
              >
                {tab === 'feed' ? 'Live Event Feed' : tab === 'metrics' ? 'Bus Metrics' : 'Pipeline Info'}
              </button>
            ))}
          </div>

          {/* Tab content */}
          <div style={{ background: '#161b22', border: '1px solid #21262d', borderTop: 'none', borderRadius: '0 0 8px 8px', minHeight: '320px' }}>
            {activeTab === 'feed' && <EventFeed events={events} />}
            {activeTab === 'metrics' && <MetricsTable metrics={metrics} />}
            {activeTab === 'pipeline' && <PipelineInfoTab />}
          </div>
        </div>

        {/* Summary bar */}
        <footer style={{
          position: 'sticky',
          bottom: 0,
          background: '#161b22',
          borderTop: '1px solid #21262d',
          padding: '10px 24px',
          display: 'flex',
          gap: '32px',
          flexWrap: 'wrap',
          alignItems: 'center',
        }}>
          <SummaryItem label="Total Events" value={String(totalEvents)} color="#e6edf3" />
          <SummaryItem
            label="Max Severity"
            value={maxSeverity}
            color={maxSeverity === '—' ? '#8b949e' : maxSevStyle.color}
          />
          <SummaryItem
            label="Rules Fired"
            value={rulesFired.size > 0 ? [...rulesFired].join(', ') : '—'}
            color="#8b949e"
            mono
          />
          <div style={{ marginLeft: 'auto', fontSize: '11px', color: '#484f58', fontFamily: 'monospace' }}>
            @anomedge/web-terminal v1.0
          </div>
        </footer>
      </div>
    </>
  );
}

function SummaryItem({
  label, value, color, mono,
}: {
  label: string;
  value: string;
  color: string;
  mono?: boolean;
}) {
  return (
    <div style={{ display: 'flex', alignItems: 'baseline', gap: '8px' }}>
      <span style={{ fontSize: '11px', color: '#484f58', letterSpacing: '0.06em', textTransform: 'uppercase' }}>
        {label}
      </span>
      <span style={{
        fontSize: '13px',
        color,
        fontFamily: mono ? 'monospace' : 'inherit',
        fontWeight: 600,
        maxWidth: '400px',
        overflow: 'hidden',
        textOverflow: 'ellipsis',
        whiteSpace: 'nowrap',
      }}>
        {value}
      </span>
    </div>
  );
}

function PipelineInfoTab() {
  return (
    <div style={{ padding: '20px', fontFamily: 'monospace', fontSize: '12px', lineHeight: '1.7', color: '#8b949e' }}>
      <div style={{ marginBottom: '16px' }}>
        <span style={{ color: '#58a6ff', fontWeight: 600 }}>Inference Chain</span>{' '}
        (Phase 0 — Rule Engine only)
      </div>
      {[
        { label: 'Tier 1', name: 'Edge AI', desc: 'INT8 ONNX model via ort crate — timeout 50ms, min confidence 0.65', active: false },
        { label: 'Tier 2', name: 'ML Statistical', desc: 'Isolation Forest on FeatureWindow — needs >= 5 samples', active: false },
        { label: 'Tier 3', name: 'Rule Engine', desc: 'Policy YAML thresholds — ALWAYS fires, never skips', active: true },
      ].map(t => (
        <div key={t.label} style={{ display: 'flex', gap: '12px', marginBottom: '10px', alignItems: 'flex-start' }}>
          <span style={{ color: '#484f58', minWidth: '48px' }}>{t.label}</span>
          <span style={{ color: t.active ? '#3fb950' : '#484f58', fontWeight: 600, minWidth: '100px' }}>{t.name}</span>
          <span style={{ color: t.active ? '#8b949e' : '#484f58' }}>{t.desc}</span>
          {t.active && (
            <span style={{
              background: '#0f2a14', color: '#3fb950', border: '1px solid #2ea043',
              padding: '1px 6px', borderRadius: '4px', fontSize: '10px', fontWeight: 700,
              whiteSpace: 'nowrap',
            }}>
              ACTIVE
            </span>
          )}
        </div>
      ))}

      <div style={{ marginTop: '24px', marginBottom: '8px', color: '#58a6ff', fontWeight: 600 }}>
        Bus Topics
      </div>
      {[
        { topic: 'signals.raw', dir: '→', desc: 'Raw telematics frames from adapter/simulator' },
        { topic: 'signals.features', dir: '→', desc: 'Computed 30s rolling window features' },
        { topic: 'decisions', dir: '→', desc: 'Raw RuleEngine decisions (all tiers)' },
        { topic: 'decisions.gated', dir: '→', desc: 'TrustEngine-filtered decisions (Person B handoff)' },
      ].map(t => (
        <div key={t.topic} style={{ display: 'flex', gap: '12px', marginBottom: '6px' }}>
          <span style={{ color: '#58a6ff', minWidth: '160px' }}>{t.topic}</span>
          <span style={{ color: '#484f58' }}>{t.dir}</span>
          <span>{t.desc}</span>
        </div>
      ))}

      <div style={{ marginTop: '24px', color: '#484f58', fontSize: '11px' }}>
        Performance targets: Feature computation &lt; 2ms · Rule engine &lt; 1ms · Total pipeline &lt; 100ms p99
      </div>
    </div>
  );
}
