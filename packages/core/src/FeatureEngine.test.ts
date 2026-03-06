import { describe, it, expect } from 'vitest';
import { FeatureEngine } from './FeatureEngine';
import type { SignalEvent, SignalMap } from '@anomedge/contracts';

// ─── Test Helper ────────────────────────────────────────────────────────────

function makeEvent(
  asset_id: string,
  timestamp_ms: number,
  overrides: Partial<SignalMap> = {},
): SignalEvent {
  return {
    ts: timestamp_ms,
    asset_id,
    driver_id: 'DRV-TEST',
    source: 'SIMULATOR',
    signals: {
      coolant_temp: 90,
      engine_rpm: 3000,
      vehicle_speed: 60,
      throttle_position: 30,
      engine_load: 50,
      brake_pedal: 0.0,
      hydraulic_pressure: 200,
      transmission_temp: 80,
      dtc_codes: [],
      ...overrides,
    },
  };
}

// ─── Scenario 1 — Single event returns zero slopes ─────────────────────────

describe('Scenario 1 — Single event returns zero slopes', () => {
  it('should return zeroed slopes/counts and all DTC codes as new', () => {
    const engine = new FeatureEngine();
    const event = makeEvent('TRUCK-001', 1000, {
      dtc_codes: ['P0301', 'P0420'],
    });
    const fw = engine.ingest(event);

    expect(fw.coolant_slope).toBe(0);
    expect(fw.brake_spike_count).toBe(0);
    expect(fw.throttle_variance).toBe(0);
    expect(fw.hydraulic_spike).toBe(false);
    expect(fw.transmission_heat).toBe(false);
    expect(fw.dtc_new).toEqual(['P0301', 'P0420']);
  });
});

// ─── Scenario 2 — Rising coolant produces positive slope ───────────────────

describe('Scenario 2 — Rising coolant over 10 events produces positive slope', () => {
  it('should compute a positive coolant_slope approximately equal to 1.0', () => {
    const engine = new FeatureEngine();
    let fw;
    for (let i = 0; i < 10; i++) {
      fw = engine.ingest(
        makeEvent('TRUCK-002', 1000 + i * 1000, {
          coolant_temp: 80 + i,
        }),
      );
    }
    expect(fw!.coolant_slope).toBeGreaterThan(0);
    expect(fw!.coolant_slope).toBeCloseTo(1.0, 1);
  });
});

// ─── Scenario 3 — Three brake events count correctly ───────────────────────

describe('Scenario 3 — Three brake spikes counted correctly', () => {
  it('should detect 3 rising edges across 6 events', () => {
    const engine = new FeatureEngine();
    const brakePedals = [0.5, 0.9, 0.5, 0.9, 0.5, 0.9];
    let fw;
    for (let i = 0; i < brakePedals.length; i++) {
      fw = engine.ingest(
        makeEvent('TRUCK-003', 1000 + i * 1000, {
          brake_pedal: brakePedals[i],
        }),
      );
    }
    expect(fw!.brake_spike_count).toBe(3);
  });
});

// ─── Scenario 4 — hydraulic_spike fires on > 500 kPa delta ────────────────

describe('Scenario 4 — hydraulic_spike fires on > 500 kPa delta', () => {
  it('should be true when delta exceeds 500', () => {
    const engine = new FeatureEngine();
    engine.ingest(makeEvent('TRUCK-004', 1000, { hydraulic_pressure: 100 }));
    const fw = engine.ingest(
      makeEvent('TRUCK-004', 2000, { hydraulic_pressure: 602 }),
    );
    expect(fw.hydraulic_spike).toBe(true);
  });

  it('should be false when delta is exactly 500 (boundary)', () => {
    const engine = new FeatureEngine();
    engine.ingest(makeEvent('TRUCK-004B', 1000, { hydraulic_pressure: 100 }));
    const fw = engine.ingest(
      makeEvent('TRUCK-004B', 2000, { hydraulic_pressure: 600 }),
    );
    expect(fw.hydraulic_spike).toBe(false);
  });
});

// ─── Scenario 5 — transmission_heat fires on temp > 110 ───────────────────

describe('Scenario 5 — transmission_heat fires on temp > 110', () => {
  it('should be true when transmission_temp is 111', () => {
    const engine = new FeatureEngine();
    const fw = engine.ingest(
      makeEvent('TRUCK-005', 1000, { transmission_temp: 111 }),
    );
    expect(fw.transmission_heat).toBe(true);
  });

  it('should be false when transmission_temp is exactly 110 (boundary)', () => {
    const engine = new FeatureEngine();
    const fw = engine.ingest(
      makeEvent('TRUCK-005B', 1000, { transmission_temp: 110 }),
    );
    expect(fw.transmission_heat).toBe(false);
  });
});

// ─── Scenario 6 — dtc_new isolates only new codes ─────────────────────────

describe('Scenario 6 — dtc_new isolates only new codes', () => {
  it('should return only P0303 as new after second event', () => {
    const engine = new FeatureEngine();
    engine.ingest(
      makeEvent('TRUCK-006', 1000, { dtc_codes: ['P0301', 'P0302'] }),
    );
    const fw = engine.ingest(
      makeEvent('TRUCK-006', 2000, { dtc_codes: ['P0301', 'P0303'] }),
    );
    expect(fw.dtc_new).toEqual(['P0303']);
  });
});
