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

type ActiveTab = 'feed' | 'metrics' | 'pipeline' | 'mobile';
type SimMode = 'scenario' | 'live';

// ─── Action Templates — what the mobile app tells the driver ─────────────────

interface ActionTemplate {
  icon: string;
  title: string;
  guidance: string;
  speak: boolean;
}

const ACTION_TEMPLATES: Record<string, ActionTemplate> = {
  // Thermal
  coolant_high_temp: {
    icon: '\u{1F321}',   // thermometer
    title: 'Engine Running Hot',
    guidance: 'Reduce speed and avoid heavy acceleration. Pull over within 5 minutes if temperature keeps rising. Check coolant level at next safe stop.',
    speak: true,
  },
  coolant_overheat_critical: {
    icon: '\u{1F6A8}',   // rotating light
    title: 'CRITICAL: Engine Overheating',
    guidance: 'Pull over IMMEDIATELY and turn off the engine. Do NOT open the hood until it cools. Call roadside assistance or your fleet manager.',
    speak: true,
  },
  coolant_rising_fast: {
    icon: '\u{26A0}',    // warning
    title: 'Rapid Temperature Rise',
    guidance: 'Temperature climbing fast. Turn off A/C, turn on heater to full to draw heat from engine. Reduce speed and prepare to pull over.',
    speak: true,
  },
  intake_air_hot: {
    icon: '\u{1F32C}',   // wind
    title: 'Intake Air Temperature High',
    guidance: 'Air filter may be clogged. Performance may drop slightly. Schedule a check at your next maintenance window.',
    speak: false,
  },
  // Braking
  harsh_brake_event: {
    icon: '\u{1F6D1}',   // stop sign
    title: 'Harsh Braking Detected',
    guidance: 'Multiple hard stops detected. Increase following distance and anticipate stops. Smoother braking protects your brakes and cargo.',
    speak: false,
  },
  excessive_braking: {
    icon: '\u{26D4}',    // no entry
    title: 'Excessive Braking - Slow Down',
    guidance: 'Frequent hard braking is dangerous. SLOW DOWN and increase your following distance. If brakes feel spongy, pull over and inspect.',
    speak: true,
  },
  // Speed
  speed_over_limit_watch: {
    icon: '\u{1F3CE}',   // racing car
    title: 'Speed Advisory',
    guidance: 'You are exceeding 110 km/h. Reduce speed to stay within fleet policy. High speed increases fuel consumption and risk.',
    speak: false,
  },
  speed_over_limit_high: {
    icon: '\u{1F6A8}',   // rotating light
    title: 'Dangerously High Speed',
    guidance: 'SLOW DOWN IMMEDIATELY. Speed above 130 km/h is a critical safety violation. This event has been logged for fleet review.',
    speak: true,
  },
  // Hydraulic
  hydraulic_spike_rule: {
    icon: '\u{1F527}',   // wrench
    title: 'Hydraulic Pressure Spike',
    guidance: 'Abnormal hydraulic pressure detected. Reduce load on hydraulic systems. Visit a mechanic if this recurs — possible seal or pump issue.',
    speak: true,
  },
  // Transmission
  transmission_overheat: {
    icon: '\u{2699}',    // gear
    title: 'Transmission Overheating',
    guidance: 'Transmission fluid is too hot. Reduce speed, downshift if towing. Pull over and let it cool if warning persists. Mechanic visit recommended.',
    speak: true,
  },
  transmission_heat_flag: {
    icon: '\u{2699}',    // gear
    title: 'Transmission Heat Warning',
    guidance: 'Sustained high transmission temp. Avoid towing or heavy loads. Schedule transmission fluid check with your mechanic.',
    speak: true,
  },
  // Electrical
  low_battery_voltage: {
    icon: '\u{1F50B}',   // battery
    title: 'Low Battery Voltage',
    guidance: 'Battery at 11.5V or below. Check if headlights are dim. Alternator may be failing — visit mechanic within 24 hours to avoid breakdown.',
    speak: false,
  },
  critical_battery_voltage: {
    icon: '\u{1F6A8}',   // rotating light
    title: 'CRITICAL: Battery Dying',
    guidance: 'Battery critically low. Vehicle may stall at any moment. Drive directly to the nearest mechanic or safe stop. Do NOT turn off the engine.',
    speak: true,
  },
  // DTC
  dtc_new_codes: {
    icon: '\u{1F4CB}',   // clipboard
    title: 'New Diagnostic Code',
    guidance: 'A new fault code was detected by the onboard computer. Schedule a diagnostic scan with your mechanic at the earliest convenience.',
    speak: false,
  },
  // Fuel
  fuel_level_low: {
    icon: '\u{26FD}',    // fuel pump
    title: 'Fuel Getting Low',
    guidance: 'Fuel below 15%. Plan a refueling stop soon. Nearest fuel stations shown on your map.',
    speak: false,
  },
  fuel_level_critical: {
    icon: '\u{1F6A8}',   // rotating light
    title: 'CRITICAL: Refuel Now',
    guidance: 'Fuel below 5%. You risk running out. Head to the nearest fuel station IMMEDIATELY. Running dry can damage the fuel pump.',
    speak: true,
  },
  // Engine
  engine_overload: {
    icon: '\u{1F4A8}',   // dashing away
    title: 'Engine Overloaded',
    guidance: 'Engine under extreme load. If towing, reduce payload or shift to lower gear. Sustained overload causes accelerated wear.',
    speak: false,
  },
  high_idle_rpm: {
    icon: '\u{1F504}',   // counterclockwise arrows
    title: 'Engine Over-Revving',
    guidance: 'RPM sustained above 4500. Shift to a higher gear or ease off the throttle. Prolonged over-rev damages the engine.',
    speak: false,
  },
};

function getActionForDecision(d: Decision): ActionTemplate {
  return ACTION_TEMPLATES[d.rule_id] ?? {
    icon: '\u{2139}',
    title: d.rule_id.replace(/_/g, ' '),
    guidance: `${d.rule_id}: value ${d.raw_value.toFixed(1)} crossed threshold ${d.threshold.toFixed(1)}. Monitor the situation.`,
    speak: d.severity === 'HIGH' || d.severity === 'CRITICAL',
  };
}

// ─── Mobile Action (what the phone app displays) ────────────────────────────

interface MobileAction {
  id: string;
  ts: number;
  severity: string;
  asset_id: string;
  rule_id: string;
  icon: string;
  title: string;
  guidance: string;
  speak: boolean;
  raw_value: number;
  threshold: number;
  acknowledged: boolean;
}

// ─── Live Simulator — generates continuous telemetry at ~1 Hz ─────────────────

interface AnomalySlot {
  type: string;
  startTick: number;
  duration: number; // how long it lasts
  drift: Record<string, number>;
}

interface VehicleSim {
  asset_id: string;
  coolant_temp: number;
  engine_rpm: number;
  vehicle_speed: number;
  throttle_position: number;
  engine_load: number;
  brake_pedal: number;
  fuel_level: number;
  battery_voltage: number;
  hydraulic_pressure: number;
  transmission_temp: number;
  oil_pressure: number;
  // active anomaly slots (can overlap)
  activeAnomalies: AnomalySlot[];
  tickCount: number;
  nextAnomalyAt: number; // tick when next anomaly triggers
}

function createVehicleSim(asset_id: string): VehicleSim {
  return {
    asset_id,
    coolant_temp: 82 + Math.random() * 8,
    engine_rpm: 1400 + Math.random() * 400,
    vehicle_speed: 60 + Math.random() * 30,
    throttle_position: 30 + Math.random() * 20,
    engine_load: 40 + Math.random() * 20,
    brake_pedal: 0,
    fuel_level: 50 + Math.random() * 40,
    battery_voltage: 13.2 + Math.random() * 0.8,
    hydraulic_pressure: 1800 + Math.random() * 400,
    transmission_temp: 75 + Math.random() * 15,
    oil_pressure: 350 + Math.random() * 100,
    activeAnomalies: [],
    tickCount: 0,
    nextAnomalyAt: 3 + Math.floor(Math.random() * 4), // first anomaly in 3-6s
  };
}

// Anomaly factory — creates drift profiles for each fault type
function spawnAnomaly(type: string, tick: number): AnomalySlot {
  const base: AnomalySlot = { type, startTick: tick, duration: 8 + Math.floor(Math.random() * 12), drift: {} };
  switch (type) {
    case 'overheat':
      base.drift['coolant_temp'] = 3.0 + Math.random() * 2.0; // aggressive rise
      break;
    case 'brake_storm':
      base.drift['brake_pedal'] = 1;
      base.duration = 6 + Math.floor(Math.random() * 6);
      break;
    case 'hydraulic_spike':
      base.drift['hydraulic_pressure'] = 100 + Math.random() * 60;
      break;
    case 'battery_drop':
      base.drift['battery_voltage'] = -0.4 - Math.random() * 0.2;
      break;
    case 'transmission_heat':
      base.drift['transmission_temp'] = 2.5 + Math.random() * 1.5;
      break;
    case 'speed_surge':
      base.drift['vehicle_speed'] = 8 + Math.random() * 5;
      base.drift['engine_rpm'] = 200 + Math.random() * 100;
      break;
    case 'engine_overload':
      base.drift['engine_load'] = 5 + Math.random() * 3;
      base.drift['engine_rpm'] = 150 + Math.random() * 100;
      break;
    case 'fuel_drain':
      base.drift['fuel_level'] = -2 - Math.random(); // rapid fuel loss
      base.duration = 15 + Math.floor(Math.random() * 10);
      break;
  }
  return base;
}

const ANOMALY_TYPES = [
  'overheat', 'brake_storm', 'hydraulic_spike', 'battery_drop',
  'transmission_heat', 'speed_surge', 'engine_overload', 'fuel_drain',
];

function tickVehicle(v: VehicleSim): Record<string, number> {
  v.tickCount++;

  // Spawn new anomalies frequently — every 5-10 seconds, can overlap
  if (v.tickCount >= v.nextAnomalyAt) {
    // Pick 1-2 anomaly types that aren't already active
    const activeTypes = new Set(v.activeAnomalies.map(a => a.type));
    const available = ANOMALY_TYPES.filter(t => !activeTypes.has(t));
    if (available.length > 0) {
      const count = Math.random() < 0.3 ? 2 : 1; // 30% chance of double fault
      for (let i = 0; i < count && i < available.length; i++) {
        const picked = available[Math.floor(Math.random() * available.length)];
        v.activeAnomalies.push(spawnAnomaly(picked, v.tickCount));
      }
    }
    v.nextAnomalyAt = v.tickCount + 5 + Math.floor(Math.random() * 6); // next in 5-10s
  }

  // Remove expired anomalies
  v.activeAnomalies = v.activeAnomalies.filter(a => v.tickCount - a.startTick < a.duration);

  // Merge all active drifts
  const mergedDrift: Record<string, number> = {};
  const hasBrakeStorm = v.activeAnomalies.some(a => a.type === 'brake_storm');
  for (const a of v.activeAnomalies) {
    for (const [k, val] of Object.entries(a.drift)) {
      mergedDrift[k] = (mergedDrift[k] ?? 0) + val;
    }
  }

  // Apply drifts + noise
  const noise = () => (Math.random() - 0.5) * 2;

  v.coolant_temp += (mergedDrift['coolant_temp'] ?? 0) + noise() * 0.3;
  v.engine_rpm += (mergedDrift['engine_rpm'] ?? 0) + noise() * 50;
  v.vehicle_speed += (mergedDrift['vehicle_speed'] ?? 0) + noise() * 2;
  v.throttle_position += noise() * 3;
  v.engine_load += (mergedDrift['engine_load'] ?? 0) + noise() * 2;
  v.oil_pressure += noise() * 5;
  v.hydraulic_pressure += (mergedDrift['hydraulic_pressure'] ?? 0) + noise() * 10;
  v.transmission_temp += (mergedDrift['transmission_temp'] ?? 0) + noise() * 0.5;
  v.battery_voltage += (mergedDrift['battery_voltage'] ?? 0) + noise() * 0.02;
  v.fuel_level += (mergedDrift['fuel_level'] ?? 0) - 0.01 - Math.random() * 0.01;

  // Brake pedal — oscillate during brake_storm, else mostly off
  if (hasBrakeStorm) {
    v.brake_pedal = v.tickCount % 2 === 0 ? 0.9 + Math.random() * 0.1 : 0.1 * Math.random();
  } else {
    v.brake_pedal = Math.random() < 0.05 ? 0.3 + Math.random() * 0.5 : Math.max(0, v.brake_pedal - 0.1);
  }

  // Recovery toward normal when no anomaly is pushing a signal
  if (!mergedDrift['coolant_temp'])       v.coolant_temp       += (88 - v.coolant_temp) * 0.05;
  if (!mergedDrift['hydraulic_pressure']) v.hydraulic_pressure += (2000 - v.hydraulic_pressure) * 0.03;
  if (!mergedDrift['transmission_temp'])  v.transmission_temp  += (82 - v.transmission_temp) * 0.04;
  if (!mergedDrift['battery_voltage'])    v.battery_voltage    += (13.5 - v.battery_voltage) * 0.05;
  if (!mergedDrift['vehicle_speed'])      v.vehicle_speed      += (75 - v.vehicle_speed) * 0.02;
  if (!mergedDrift['engine_load'])        v.engine_load        += (50 - v.engine_load) * 0.03;
  if (!mergedDrift['engine_rpm'])         v.engine_rpm         += (1600 - v.engine_rpm) * 0.03;

  // Clamp to realistic ranges
  v.coolant_temp = Math.max(60, Math.min(140, v.coolant_temp));
  v.engine_rpm = Math.max(700, Math.min(5500, v.engine_rpm));
  v.vehicle_speed = Math.max(0, Math.min(180, v.vehicle_speed));
  v.throttle_position = Math.max(0, Math.min(100, v.throttle_position));
  v.engine_load = Math.max(0, Math.min(100, v.engine_load));
  v.brake_pedal = Math.max(0, Math.min(1, v.brake_pedal));
  v.fuel_level = Math.max(0, Math.min(100, v.fuel_level));
  v.battery_voltage = Math.max(8, Math.min(15, v.battery_voltage));
  v.hydraulic_pressure = Math.max(500, Math.min(3500, v.hydraulic_pressure));
  v.transmission_temp = Math.max(50, Math.min(150, v.transmission_temp));
  v.oil_pressure = Math.max(100, Math.min(600, v.oil_pressure));

  return {
    coolant_temp: Math.round(v.coolant_temp * 100) / 100,
    engine_rpm: Math.round(v.engine_rpm),
    vehicle_speed: Math.round(v.vehicle_speed * 10) / 10,
    throttle_position: Math.round(v.throttle_position * 10) / 10,
    engine_load: Math.round(v.engine_load * 10) / 10,
    brake_pedal: Math.round(v.brake_pedal * 100) / 100,
    fuel_level: Math.round(v.fuel_level * 10) / 10,
    battery_voltage: Math.round(v.battery_voltage * 100) / 100,
    hydraulic_pressure: Math.round(v.hydraulic_pressure),
    transmission_temp: Math.round(v.transmission_temp * 10) / 10,
    oil_pressure: Math.round(v.oil_pressure),
  };
}

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
        No events yet. Run a scenario or start Live Mode.
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
  const [simMode, setSimMode] = useState<SimMode>('scenario');
  const [mobileActions, setMobileActions] = useState<MobileAction[]>([]);

  const busRef = useRef<EventBus | null>(null);
  const actionSeqRef = useRef(0);
  const metricsTimerRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const liveTimerRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const vehicleSimRef = useRef<VehicleSim | null>(null);
  const liveCountersRef = useRef({ maxRank: -1, maxSev: '—', rules: new Set<string>(), total: 0 });

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
    setMobileActions([]);
    setMaxSeverity('—');
    setTotalEvents(0);
    setRulesFired(new Set());
    setPipelineCounts({ 'signals.raw': 0, 'signals.features': 0, 'decisions': 0, 'decisions.gated': 0 });
    actionSeqRef.current = 0;

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

        // Generate mobile action for driver
        const tmpl = getActionForDecision(d);
        actionSeqRef.current++;
        const action: MobileAction = {
          id: `act-${actionSeqRef.current}`,
          ts: d.ts,
          severity: d.severity,
          asset_id: d.asset_id,
          rule_id: d.rule_id,
          icon: tmpl.icon,
          title: tmpl.title,
          guidance: tmpl.guidance,
          speak: tmpl.speak,
          raw_value: d.raw_value,
          threshold: d.threshold,
          acknowledged: false,
        };
        setMobileActions(prev => {
          const next = [...prev, action];
          return next.length > 50 ? next.slice(-50) : next;
        });
        // Publish action on bus for Person B
        bus.publish('actions', action);
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

  const stopLive = useCallback(() => {
    if (liveTimerRef.current) {
      clearInterval(liveTimerRef.current);
      liveTimerRef.current = null;
    }
    vehicleSimRef.current = null;
    setSimMode('scenario');
    setRunning(false);
    setStatus('Live stopped');
    if (busRef.current) setMetrics(busRef.current.getMetrics());
  }, []);

  const startLive = useCallback(async () => {
    if (running) return;
    setRunning(true);
    setSimMode('live');
    setStatus('Starting live simulator…');

    // Reset state
    setEvents([]);
    setMobileActions([]);
    setMaxSeverity('—');
    setTotalEvents(0);
    setRulesFired(new Set());
    setPipelineCounts({ 'signals.raw': 0, 'signals.features': 0, 'decisions': 0, 'decisions.gated': 0 });
    actionSeqRef.current = 0;

    try {
      const policyRes = await fetch('/policy/policy.yaml');
      if (!policyRes.ok) throw new Error(`Policy not found (status ${policyRes.status})`);
      const policyYaml = await policyRes.text();
      const policy = parseMinimalYaml(policyYaml);

      const bus = new EventBus();
      busRef.current = bus;
      createPipeline(policy, bus);

      // Local counters
      const localCounts = { 'signals.raw': 0, 'signals.features': 0, 'decisions': 0, 'decisions.gated': 0 };
      const lc = liveCountersRef.current = { maxRank: -1, maxSev: '—', rules: new Set<string>(), total: 0 };

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
        lc.total++;
        setPipelineCounts(prev => ({ ...prev, 'decisions.gated': localCounts['decisions.gated'] }));

        const d = envelope.payload;
        const rank = SEVERITY_ORDER[d.severity] ?? 0;
        if (rank > lc.maxRank) {
          lc.maxRank = rank;
          lc.maxSev = d.severity;
          setMaxSeverity(d.severity);
        }
        lc.rules.add(d.rule_id);
        setRulesFired(new Set(lc.rules));
        setTotalEvents(lc.total);
        setEvents(prev => {
          const next = [...prev, { envelope, receivedAt: Date.now() }];
          return next.length > 200 ? next.slice(-200) : next;
        });

        // Generate mobile action for driver
        const tmpl = getActionForDecision(d);
        actionSeqRef.current++;
        const action: MobileAction = {
          id: `act-${actionSeqRef.current}`,
          ts: d.ts,
          severity: d.severity,
          asset_id: d.asset_id,
          rule_id: d.rule_id,
          icon: tmpl.icon,
          title: tmpl.title,
          guidance: tmpl.guidance,
          speak: tmpl.speak,
          raw_value: d.raw_value,
          threshold: d.threshold,
          acknowledged: false,
        };
        setMobileActions(prev => {
          const next = [...prev, action];
          return next.length > 50 ? next.slice(-50) : next;
        });
        bus.publish('actions', action);
      });

      // Create vehicle sim
      const vehicle = createVehicleSim('VH-LIVE-001');
      vehicleSimRef.current = vehicle;

      setStatus('Live — streaming telemetry at ~1 Hz');

      // Start 1 Hz interval
      liveTimerRef.current = setInterval(() => {
        if (!vehicleSimRef.current) return;
        const signals = tickVehicle(vehicleSimRef.current);
        const signalEvent: SignalEvent = {
          ts: Date.now(),
          asset_id: vehicleSimRef.current.asset_id,
          driver_id: 'DRV-LIVE',
          source: 'SIMULATOR',
          signals,
        };
        bus.publish('signals.raw', signalEvent);
      }, 1000);
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      setStatus(`Error: ${msg}`);
      setRunning(false);
      setSimMode('scenario');
    }
  }, [running]);

  // Cleanup on unmount
  useEffect(() => {
    return () => {
      if (liveTimerRef.current) clearInterval(liveTimerRef.current);
    };
  }, []);

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
            {running && simMode === 'scenario' ? 'Running…' : 'Run Scenario'}
          </button>

          <button
            onClick={simMode === 'live' ? stopLive : startLive}
            disabled={running && simMode !== 'live'}
            style={{
              background: simMode === 'live'
                ? 'linear-gradient(135deg, #da3633, #f85149)'
                : (running ? '#21262d' : 'linear-gradient(135deg, #1f6feb, #388bfd)'),
              color: (running && simMode !== 'live') ? '#484f58' : '#fff',
              border: 'none',
              borderRadius: '6px',
              padding: '6px 16px',
              fontSize: '13px',
              fontWeight: 600,
              cursor: (running && simMode !== 'live') ? 'not-allowed' : 'pointer',
              transition: 'all 0.15s',
              whiteSpace: 'nowrap',
            }}
          >
            {simMode === 'live' ? 'Stop Live' : 'Live Mode'}
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
            {(['feed', 'mobile', 'metrics', 'pipeline'] as const).map(tab => (
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
                  position: 'relative',
                }}
              >
                {tab === 'feed' ? 'Live Event Feed' : tab === 'mobile' ? 'Driver App' : tab === 'metrics' ? 'Bus Metrics' : 'Pipeline Info'}
                {tab === 'mobile' && mobileActions.filter(a => !a.acknowledged).length > 0 && (
                  <span style={{
                    position: 'absolute', top: '2px', right: '2px',
                    background: '#f85149', color: '#fff', borderRadius: '10px',
                    padding: '0 5px', fontSize: '10px', fontWeight: 700, minWidth: '16px', textAlign: 'center',
                  }}>
                    {mobileActions.filter(a => !a.acknowledged).length}
                  </span>
                )}
              </button>
            ))}
          </div>

          {/* Tab content */}
          <div style={{ background: '#161b22', border: '1px solid #21262d', borderTop: 'none', borderRadius: '0 0 8px 8px', minHeight: '320px' }}>
            {activeTab === 'feed' && <EventFeed events={events} />}
            {activeTab === 'mobile' && (
              <MobileDevicePreview
                actions={mobileActions}
                onAcknowledge={(id) => {
                  setMobileActions(prev => prev.map(a => a.id === id ? { ...a, acknowledged: true } : a));
                }}
              />
            )}
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

// ─── Mobile Device Preview — simulates what the driver sees on their phone ───

function MobileDevicePreview({
  actions,
  onAcknowledge,
}: {
  actions: MobileAction[];
  onAcknowledge: (id: string) => void;
}) {
  const bottomRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [actions.length]);

  const unacked = actions.filter(a => !a.acknowledged);
  const acked = actions.filter(a => a.acknowledged).slice(-5);

  // Urgency color based on severity
  const urgencyColor = (sev: string) => {
    switch (sev) {
      case 'CRITICAL': return { bg: '#3d0008', border: '#f85149', text: '#ff7b72', label: 'PULL OVER' };
      case 'HIGH':     return { bg: '#2d1600', border: '#da3633', text: '#f97583', label: 'ACTION NEEDED' };
      case 'WARN':     return { bg: '#2d2000', border: '#9e6a03', text: '#e3b341', label: 'CAUTION' };
      case 'WATCH':    return { bg: '#0d2847', border: '#1f6feb', text: '#58a6ff', label: 'MONITOR' };
      default:         return { bg: '#21262d', border: '#30363d', text: '#8b949e', label: 'INFO' };
    }
  };

  return (
    <div style={{ display: 'flex', justifyContent: 'center', padding: '24px 16px', minHeight: '400px' }}>
      {/* Phone frame */}
      <div style={{
        width: '375px',
        minHeight: '500px',
        background: '#000',
        borderRadius: '32px',
        border: '3px solid #30363d',
        overflow: 'hidden',
        display: 'flex',
        flexDirection: 'column',
        boxShadow: '0 8px 32px rgba(0,0,0,0.5)',
      }}>
        {/* Phone status bar */}
        <div style={{
          padding: '8px 24px 4px',
          display: 'flex',
          justifyContent: 'space-between',
          alignItems: 'center',
          fontSize: '12px',
          fontWeight: 600,
          color: '#e6edf3',
        }}>
          <span>{new Date().toLocaleTimeString('en-US', { hour: '2-digit', minute: '2-digit', hour12: false })}</span>
          <div style={{
            width: '80px', height: '24px', background: '#1a1a1a',
            borderRadius: '12px', margin: '0 auto',
          }} />
          <span style={{ fontSize: '11px' }}>100%</span>
        </div>

        {/* App header */}
        <div style={{
          padding: '12px 20px',
          background: 'linear-gradient(135deg, #0d1117, #161b22)',
          borderBottom: '1px solid #21262d',
          display: 'flex',
          alignItems: 'center',
          gap: '10px',
        }}>
          <div style={{
            width: '24px', height: '24px', borderRadius: '6px',
            background: 'linear-gradient(135deg, #58a6ff, #388bfd)',
            display: 'flex', alignItems: 'center', justifyContent: 'center',
            fontSize: '12px', fontWeight: 700, color: '#000',
          }}>A</div>
          <div>
            <div style={{ fontSize: '14px', fontWeight: 600, color: '#e6edf3' }}>AnomEdge Driver</div>
            <div style={{ fontSize: '10px', color: '#8b949e' }}>Vehicle VH-LIVE-001</div>
          </div>
          {unacked.length > 0 && (
            <div style={{
              marginLeft: 'auto',
              background: unacked.some(a => a.severity === 'CRITICAL') ? '#f85149' : '#da3633',
              color: '#fff', borderRadius: '12px', padding: '2px 10px',
              fontSize: '11px', fontWeight: 700,
              animation: unacked.some(a => a.severity === 'CRITICAL') ? 'pulse 1s ease-in-out infinite' : 'none',
            }}>
              {unacked.length} alert{unacked.length > 1 ? 's' : ''}
            </div>
          )}
        </div>

        {/* Notification area */}
        <div style={{
          flex: 1,
          overflowY: 'auto',
          padding: '12px 12px',
          background: '#0d1117',
        }}>
          {actions.length === 0 && (
            <div style={{
              textAlign: 'center', padding: '60px 20px',
              color: '#484f58', fontSize: '13px',
            }}>
              <div style={{ fontSize: '40px', marginBottom: '12px', opacity: 0.4 }}>{'\u{1F6A6}'}</div>
              <div>All systems normal</div>
              <div style={{ fontSize: '11px', marginTop: '4px' }}>Alerts will appear here when anomalies are detected</div>
            </div>
          )}

          {/* Active (unacknowledged) alerts */}
          {unacked.map((action) => {
            const uc = urgencyColor(action.severity);
            return (
              <div
                key={action.id}
                style={{
                  background: uc.bg,
                  border: `1px solid ${uc.border}`,
                  borderRadius: '12px',
                  padding: '14px',
                  marginBottom: '10px',
                  animation: action.severity === 'CRITICAL' ? 'pulse 1.5s ease-in-out infinite' : 'none',
                }}
              >
                {/* Header row */}
                <div style={{ display: 'flex', alignItems: 'center', gap: '8px', marginBottom: '8px' }}>
                  <span style={{ fontSize: '22px' }}>{action.icon}</span>
                  <div style={{ flex: 1 }}>
                    <div style={{ fontSize: '14px', fontWeight: 700, color: uc.text }}>{action.title}</div>
                    <div style={{ fontSize: '10px', color: '#6e7681', marginTop: '2px' }}>
                      {new Date(action.ts).toLocaleTimeString('en-US', { hour12: false })}
                      {' \u00B7 '}
                      {action.rule_id}
                    </div>
                  </div>
                  <span style={{
                    background: uc.border,
                    color: '#fff',
                    padding: '2px 8px',
                    borderRadius: '6px',
                    fontSize: '9px',
                    fontWeight: 800,
                    letterSpacing: '0.06em',
                  }}>{uc.label}</span>
                </div>

                {/* Guidance */}
                <div style={{
                  fontSize: '12px',
                  lineHeight: '1.5',
                  color: '#c9d1d9',
                  marginBottom: '10px',
                  padding: '8px 10px',
                  background: 'rgba(0,0,0,0.25)',
                  borderRadius: '8px',
                }}>
                  {action.guidance}
                </div>

                {/* Value bar */}
                <div style={{
                  display: 'flex', alignItems: 'center', gap: '10px',
                  fontSize: '11px', color: '#8b949e', marginBottom: '10px',
                }}>
                  <span>Value: <span style={{ color: uc.text, fontWeight: 600 }}>{action.raw_value.toFixed(1)}</span></span>
                  <span style={{ color: '#484f58' }}>|</span>
                  <span>Threshold: {action.threshold.toFixed(1)}</span>
                  {action.speak && (
                    <>
                      <span style={{ color: '#484f58' }}>|</span>
                      <span style={{ color: uc.text }}>{'\u{1F50A}'} Voice alert</span>
                    </>
                  )}
                </div>

                {/* Acknowledge button */}
                <button
                  onClick={() => onAcknowledge(action.id)}
                  style={{
                    width: '100%',
                    padding: '8px',
                    background: 'rgba(255,255,255,0.08)',
                    border: `1px solid ${uc.border}`,
                    borderRadius: '8px',
                    color: uc.text,
                    fontSize: '12px',
                    fontWeight: 600,
                    cursor: 'pointer',
                    transition: 'background 0.15s',
                  }}
                  onMouseEnter={e => (e.currentTarget.style.background = 'rgba(255,255,255,0.15)')}
                  onMouseLeave={e => (e.currentTarget.style.background = 'rgba(255,255,255,0.08)')}
                >
                  {action.severity === 'CRITICAL' || action.severity === 'HIGH' ? 'Acknowledged - Taking Action' : 'Dismiss'}
                </button>
              </div>
            );
          })}

          {/* Acknowledged (dimmed) */}
          {acked.length > 0 && (
            <div style={{ marginTop: '8px', opacity: 0.45 }}>
              <div style={{ fontSize: '10px', color: '#484f58', letterSpacing: '0.06em', textTransform: 'uppercase', marginBottom: '6px', paddingLeft: '4px' }}>
                Acknowledged
              </div>
              {acked.map((action) => (
                <div key={action.id} style={{
                  background: '#161b22',
                  border: '1px solid #21262d',
                  borderRadius: '8px',
                  padding: '10px 12px',
                  marginBottom: '6px',
                  display: 'flex',
                  alignItems: 'center',
                  gap: '10px',
                }}>
                  <span style={{ fontSize: '16px' }}>{action.icon}</span>
                  <div style={{ flex: 1 }}>
                    <div style={{ fontSize: '12px', fontWeight: 600, color: '#8b949e' }}>{action.title}</div>
                    <div style={{ fontSize: '10px', color: '#484f58' }}>
                      {new Date(action.ts).toLocaleTimeString('en-US', { hour12: false })}
                    </div>
                  </div>
                  <span style={{ fontSize: '10px', color: '#3fb950' }}>{'\u2713'}</span>
                </div>
              ))}
            </div>
          )}

          <div ref={bottomRef} />
        </div>

        {/* Phone bottom bar */}
        <div style={{
          padding: '8px 0 16px',
          background: '#0d1117',
          borderTop: '1px solid #21262d',
          display: 'flex',
          justifyContent: 'center',
        }}>
          <div style={{ width: '120px', height: '4px', background: '#30363d', borderRadius: '2px' }} />
        </div>
      </div>

      {/* Side panel — action log */}
      <div style={{
        width: '320px',
        marginLeft: '24px',
        fontSize: '12px',
        fontFamily: 'monospace',
        color: '#8b949e',
      }}>
        <div style={{ fontSize: '11px', color: '#484f58', letterSpacing: '0.06em', textTransform: 'uppercase', marginBottom: '12px' }}>
          Action Bus Log (actions topic)
        </div>
        <div style={{ maxHeight: '460px', overflowY: 'auto' }}>
          {actions.slice(-15).reverse().map(a => {
            const uc = urgencyColor(a.severity);
            return (
              <div key={a.id} style={{
                padding: '8px',
                borderBottom: '1px solid #21262d',
                opacity: a.acknowledged ? 0.4 : 1,
              }}>
                <div style={{ display: 'flex', gap: '6px', alignItems: 'center' }}>
                  <SeverityBadge severity={a.severity} />
                  <span style={{ color: uc.text, fontWeight: 600, fontSize: '11px' }}>{a.title}</span>
                </div>
                <div style={{ fontSize: '10px', color: '#484f58', marginTop: '4px' }}>
                  {a.speak ? '\u{1F50A} TTS' : '\u{1F515} silent'} · {a.rule_id} · {new Date(a.ts).toLocaleTimeString('en-US', { hour12: false })}
                </div>
              </div>
            );
          })}
        </div>
        <div style={{ marginTop: '16px', padding: '12px', background: '#161b22', borderRadius: '8px', border: '1px solid #21262d' }}>
          <div style={{ fontSize: '10px', color: '#484f58', letterSpacing: '0.06em', textTransform: 'uppercase', marginBottom: '8px' }}>
            Severity Legend
          </div>
          {[
            { sev: 'CRITICAL', label: 'Pull over immediately', color: '#f85149' },
            { sev: 'HIGH', label: 'Action needed now', color: '#f97583' },
            { sev: 'WARN', label: 'Caution - monitor closely', color: '#e3b341' },
            { sev: 'WATCH', label: 'Informational - be aware', color: '#58a6ff' },
          ].map(item => (
            <div key={item.sev} style={{ display: 'flex', alignItems: 'center', gap: '8px', marginBottom: '4px' }}>
              <SeverityBadge severity={item.sev} />
              <span style={{ fontSize: '11px', color: '#8b949e' }}>{item.label}</span>
            </div>
          ))}
        </div>
      </div>
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
