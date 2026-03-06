// packages/contracts/__tests__/contracts.test.ts
import { describe, it, expect } from 'vitest';
import type {
  Severity,
  SignalSource,
  SignalMap,
  SignalEvent,
  FeatureWindow,
  DecisionSource,
  RuleGroup,
  Decision,
  Action,
  VehicleClass,
  PolicyRule,
  PolicyPack,
  BusTopic,
  EventEnvelope,
} from '../src/index';

describe('contracts — SignalEvent', () => {
  it('creates a valid SignalEvent with all required fields', () => {
    const signals: SignalMap = {
      coolant_temp: 92.5,
      engine_rpm: 2400,
      vehicle_speed: 88,
      hydraulic_pressure: 3200,
      dtc_codes: ['P0300'],
    };

    const event: SignalEvent = {
      ts: 1709000000000,
      asset_id: 'TRUCK-001',
      driver_id: 'DRV-042',
      source: 'OBD2_GENERIC' as SignalSource,
      signals,
    };

    expect(event.ts).toBe(1709000000000);
    expect(event.asset_id).toBe('TRUCK-001');
    expect(event.driver_id).toBe('DRV-042');
    expect(event.source).toBe('OBD2_GENERIC');
    expect(event.signals.coolant_temp).toBe(92.5);
    expect(event.signals.dtc_codes).toEqual(['P0300']);
  });

  it('accepts all SignalSource values', () => {
    const sources: SignalSource[] = [
      'SIMULATOR',
      'OBD2_GENERIC',
      'FORD_F450',
      'CAT_HEAVY',
      'JOHN_DEERE_139',
      'CUSTOM',
    ];
    expect(sources).toHaveLength(6);
  });

  it('accepts heavy fleet signals', () => {
    const signals: SignalMap = {
      hydraulic_pressure: 3500,
      transmission_temp: 95,
      boom_position: 45.0,
      load_weight: 12000,
      def_level: 78,
      pto_rpm: 1800,
    };
    expect(signals.hydraulic_pressure).toBe(3500);
    expect(signals.boom_position).toBe(45.0);
  });

  it('accepts arbitrary extra signals via index signature', () => {
    const signals: SignalMap = {
      custom_sensor_x: 42,
      custom_label: 'zone-3',
    };
    expect(signals['custom_sensor_x']).toBe(42);
  });
});

describe('contracts — FeatureWindow', () => {
  it('creates a valid FeatureWindow with all required fields', () => {
    const window: FeatureWindow = {
      ts: 1709000030000,
      asset_id: 'TRUCK-001',
      window_seconds: 30,
      coolant_slope: 0.12,
      brake_spike_count: 3,
      speed_mean: 72.4,
      rpm_mean: 2100,
      engine_load_mean: 68.5,
      throttle_variance: 4.2,
      hydraulic_spike: false,
      transmission_heat: false,
      dtc_new: [],
      signals_snapshot: { coolant_temp: 95 },
    };

    expect(window.window_seconds).toBe(30);
    expect(window.coolant_slope).toBe(0.12);
    expect(window.hydraulic_spike).toBe(false);
    expect(window.dtc_new).toEqual([]);
  });
});

describe('contracts — Decision', () => {
  it('creates a valid Decision with decision_source field', () => {
    const decision: Decision = {
      ts: 1709000031000,
      asset_id: 'TRUCK-001',
      severity: 'HIGH' as Severity,
      rule_id: 'coolant_overheat_high',
      rule_group: 'thermal' as RuleGroup,
      confidence: 0.95,
      triggered_by: ['coolant_slope', 'coolant_temp'],
      raw_value: 112.0,
      threshold: 108.0,
      decision_source: 'RULE_ENGINE' as DecisionSource,
      context: { coolant_slope: 0.5 },
    };

    expect(decision.severity).toBe('HIGH');
    expect(decision.decision_source).toBe('RULE_ENGINE');
    expect(decision.confidence).toBe(0.95);
    expect(decision.triggered_by).toContain('coolant_slope');
  });

  it('accepts all DecisionSource values', () => {
    const sources: DecisionSource[] = ['EDGE_AI', 'ML_STATISTICAL', 'RULE_ENGINE'];
    expect(sources).toHaveLength(3);
  });

  it('accepts all RuleGroup values', () => {
    const groups: RuleGroup[] = [
      'thermal', 'braking', 'speed', 'hydraulic',
      'electrical', 'dtc', 'transmission', 'fuel', 'composite',
    ];
    expect(groups).toHaveLength(9);
  });
});

describe('contracts — Action', () => {
  it('creates a valid Action with speak: boolean', () => {
    const action: Action = {
      seq: 1,
      ts: 1709000032000,
      asset_id: 'TRUCK-001',
      severity: 'HIGH' as Severity,
      title: 'Coolant Overheating',
      guidance: 'Reduce engine load immediately. Pull over if safe.',
      rule_id: 'coolant_overheat_high',
      speak: true,
      acknowledged: false,
      source: 'TEMPLATE',
      decision_source: 'RULE_ENGINE' as DecisionSource,
    };

    expect(action.speak).toBe(true);
    expect(action.acknowledged).toBe(false);
    expect(action.source).toBe('TEMPLATE');
    expect(action.title).toBe('Coolant Overheating');
  });
});

describe('contracts — PolicyPack', () => {
  it('creates a valid PolicyPack with PolicyRule', () => {
    const rule: PolicyRule = {
      id: 'coolant_overheat_critical',
      group: 'thermal' as RuleGroup,
      signal: 'coolant_temp',
      operator: 'gt',
      threshold: 120,
      severity: 'CRITICAL' as Severity,
      cooldown_ms: 30000,
      hysteresis: 5,
      description: 'Engine coolant critically overheated',
    };

    const pack: PolicyPack = {
      version: '1.0.0',
      vehicle_class: 'FLEET_DIESEL' as VehicleClass,
      rules: [rule],
    };

    expect(pack.rules).toHaveLength(1);
    expect(pack.rules[0].threshold).toBe(120);
    expect(pack.rules[0].operator).toBe('gt');
  });

  it('accepts all VehicleClass values', () => {
    const classes: VehicleClass[] = [
      'LIGHT_TRUCK', 'HEAVY_EQUIPMENT', 'FLEET_DIESEL', 'PASSENGER', 'SIMULATOR',
    ];
    expect(classes).toHaveLength(5);
  });
});

describe('contracts — EventEnvelope + BusTopic', () => {
  it('wraps a SignalEvent in an EventEnvelope', () => {
    const event: SignalEvent = {
      ts: 1709000000000,
      asset_id: 'TRUCK-001',
      driver_id: 'DRV-042',
      source: 'SIMULATOR',
      signals: { engine_rpm: 1800 },
    };

    const envelope: EventEnvelope<SignalEvent> = {
      id: 'uuid-1234',
      topic: 'signals.raw' as BusTopic,
      seq: 1,
      ts: 1709000000000,
      payload: event,
    };

    expect(envelope.topic).toBe('signals.raw');
    expect(envelope.payload.asset_id).toBe('TRUCK-001');
    expect(envelope.seq).toBe(1);
  });

  it('accepts all 9 BusTopic values', () => {
    const topics: BusTopic[] = [
      'signals.raw',
      'signals.features',
      'decisions',
      'decisions.gated',
      'actions',
      'telemetry.sync',
      'model.ota',
      'system.heartbeat',
      'system.error',
    ];
    expect(topics).toHaveLength(9);
  });
});
