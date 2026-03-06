import type { SignalEvent, FeatureWindow, SignalMap } from '@anomedge/contracts';

// ─── Named Constants (spec thresholds) ──────────────────────────────────────

const WINDOW_SECONDS = 30;
const WINDOW_MS = WINDOW_SECONDS * 1000;
const BRAKE_SPIKE_THRESHOLD = 0.8;
const HYDRAULIC_SPIKE_DELTA = 500;
const TRANSMISSION_HEAT_LIMIT = 110;

// ─── FeatureEngine ──────────────────────────────────────────────────────────

export class FeatureEngine {
  private buffers: Map<string, SignalEvent[]> = new Map();

  /**
   * Ingest a SignalEvent, update the rolling buffer, and compute features.
   */
  ingest(event: SignalEvent): FeatureWindow {
    const { asset_id, ts } = event;

    // Get or create buffer for this asset
    let buffer = this.buffers.get(asset_id);
    if (!buffer) {
      buffer = [];
      this.buffers.set(asset_id, buffer);
    }

    // Append event (assumes chronological ingestion)
    buffer.push(event);

    // Evict events older than WINDOW_MS relative to the latest event
    const cutoff = ts - WINDOW_MS;
    while (buffer.length > 0 && buffer[0].ts < cutoff) {
      buffer.shift();
    }

    // Compute features from the current window
    return this._computeFeatures(asset_id, buffer);
  }

  // ─── Feature Computation ────────────────────────────────────────────────

  private _computeFeatures(asset_id: string, window: SignalEvent[]): FeatureWindow {
    const latest = window[window.length - 1];
    const n = window.length;

    // Extract signal arrays (defaulting optional fields to 0)
    const coolantTemps = window.map(e => e.signals.coolant_temp ?? 0);
    const speeds = window.map(e => e.signals.vehicle_speed ?? 0);
    const rpms = window.map(e => e.signals.engine_rpm ?? 0);
    const loads = window.map(e => e.signals.engine_load ?? 0);
    const throttles = window.map(e => e.signals.throttle_position ?? 0);

    // 1. coolant_slope — OLS over sample indices
    const coolant_slope = this._slope(coolantTemps);

    // 2. brake_spike_count — rising edges crossing BRAKE_SPIKE_THRESHOLD
    let brake_spike_count = 0;
    for (let i = 1; i < n; i++) {
      const prev = window[i - 1].signals.brake_pedal ?? 0;
      const curr = window[i].signals.brake_pedal ?? 0;
      if (prev < BRAKE_SPIKE_THRESHOLD && curr >= BRAKE_SPIKE_THRESHOLD) {
        brake_spike_count++;
      }
    }

    // 3-5. Arithmetic means
    const speed_mean = this._mean(speeds);
    const rpm_mean = this._mean(rpms);
    const engine_load_mean = this._mean(loads);

    // 6. throttle_variance — population variance
    const throttle_variance = this._populationVariance(throttles);

    // 7. hydraulic_spike — consecutive delta > HYDRAULIC_SPIKE_DELTA
    let hydraulic_spike = false;
    for (let i = 1; i < n; i++) {
      const prev = window[i - 1].signals.hydraulic_pressure ?? 0;
      const curr = window[i].signals.hydraulic_pressure ?? 0;
      if (Math.abs(curr - prev) > HYDRAULIC_SPIKE_DELTA) {
        hydraulic_spike = true;
        break;
      }
    }

    // 8. transmission_heat — any event exceeds TRANSMISSION_HEAT_LIMIT
    const transmission_heat = window.some(
      e => (e.signals.transmission_temp ?? 0) > TRANSMISSION_HEAT_LIMIT
    );

    // 9. dtc_new — codes in latest event not in any prior event
    const latestDtcCodes = (latest.signals.dtc_codes as string[] | undefined) ?? [];
    let dtc_new: string[];
    if (n <= 1) {
      dtc_new = [...latestDtcCodes];
    } else {
      const priorCodes = new Set<string>();
      for (let i = 0; i < n - 1; i++) {
        const codes = (window[i].signals.dtc_codes as string[] | undefined) ?? [];
        for (const code of codes) {
          priorCodes.add(code);
        }
      }
      dtc_new = latestDtcCodes.filter(code => !priorCodes.has(code));
    }

    // 10. signals_snapshot — pass through latest event's signals
    const signals_snapshot: Partial<SignalMap> = { ...latest.signals };

    return {
      ts: latest.ts,
      asset_id,
      window_seconds: WINDOW_SECONDS,
      coolant_slope,
      brake_spike_count,
      speed_mean,
      rpm_mean,
      engine_load_mean,
      throttle_variance,
      hydraulic_spike,
      transmission_heat,
      dtc_new,
      signals_snapshot,
    };
  }

  // ─── Helpers ────────────────────────────────────────────────────────────

  /**
   * OLS slope of values over their indices [0, 1, 2, ...].
   * Returns 0 if n <= 1 or denominator is 0.
   */
  private _slope(values: number[]): number {
    const n = values.length;
    if (n <= 1) return 0;

    const mean_x = (n - 1) / 2;
    const mean_y = this._mean(values);

    let numerator = 0;
    let denominator = 0;
    for (let i = 0; i < n; i++) {
      const dx = i - mean_x;
      numerator += dx * (values[i] - mean_y);
      denominator += dx * dx;
    }

    return denominator === 0 ? 0 : numerator / denominator;
  }

  private _mean(values: number[]): number {
    if (values.length === 0) return 0;
    let sum = 0;
    for (const v of values) sum += v;
    return sum / values.length;
  }

  private _populationVariance(values: number[]): number {
    const n = values.length;
    if (n <= 1) return 0;
    const mean = this._mean(values);
    let sumSq = 0;
    for (const v of values) {
      const d = v - mean;
      sumSq += d * d;
    }
    return sumSq / n;
  }
}
