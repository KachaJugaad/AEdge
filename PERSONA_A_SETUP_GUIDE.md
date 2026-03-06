# AnomEdge — Person A: Intelligence Engineer
## Complete Setup Guide, Agent Playbook & Telematics Adapter System

> **Role:** You are the data and decision brain.  
> **Domain:** Simulator Service | FeatureEngine | RuleEngine | TrustEngine | Chaos Engine  
> **Stack:** Node.js / TypeScript (monorepo via pnpm workspaces)  
> **Vancouver, BC | Phase 0 Start — March 2026**

---

## Part 1: Project Bootstrap (Day 1 — Do This First)

### 1.1 Prerequisites

```bash
node --version   # Must be >= 20
pnpm --version   # Must be >= 8  (npm install -g pnpm)
git --version    # Any recent
```

### 1.2 Monorepo Scaffold

```bash
mkdir anomedge && cd anomedge
git init
git checkout -b main

# Create monorepo root
cat > package.json << 'EOF'
{
  "name": "anomedge",
  "private": true,
  "scripts": {
    "build":  "pnpm -r build",
    "test":   "pnpm -r test",
    "demo":   "ts-node packages/web-terminal/src/cli.ts",
    "gate:0": "ts-node gate-tests/phase0.gate.ts"
  },
  "devDependencies": {
    "typescript":   "^5.4.0",
    "ts-node":      "^10.9.2",
    "@types/node":  "^20.12.0",
    "vitest":       "^1.4.0"
  }
}
EOF

cat > pnpm-workspace.yaml << 'EOF'
packages:
  - 'packages/*'
  - 'gate-tests'
EOF

cat > tsconfig.json << 'EOF'
{
  "compilerOptions": {
    "target": "ES2022",
    "module": "commonjs",
    "moduleResolution": "node",
    "strict": true,
    "esModuleInterop": true,
    "outDir": "dist",
    "rootDir": "src",
    "declaration": true,
    "paths": {
      "@anomedge/contracts": ["./packages/contracts/src"],
      "@anomedge/bus":       ["./packages/bus/src"]
    }
  }
}
EOF

# Create folder structure
mkdir -p packages/{contracts,simulator,core,chaos,llm-edge}/src
mkdir -p packages/{contracts,simulator,core,chaos,llm-edge}/__tests__
mkdir -p scenarios policy gate-tests adapters/telematics

pnpm install
```

### 1.3 Folder Map — Person A Owns

```
anomedge/
├── packages/
│   ├── contracts/          ← YOU OWN (Day 1, then frozen)
│   │   └── src/index.ts
│   ├── simulator/          ← YOU OWN
│   │   ├── src/
│   │   │   ├── SimulatorService.ts
│   │   │   └── index.ts
│   │   └── __tests__/
│   ├── core/               ← YOU OWN
│   │   ├── src/
│   │   │   ├── FeatureEngine.ts
│   │   │   ├── RuleEngine.ts
│   │   │   ├── TrustEngine.ts
│   │   │   └── pipeline.ts
│   │   └── __tests__/
│   └── chaos/              ← YOU OWN (Phase 2)
├── adapters/
│   └── telematics/         ← YOU OWN (NEW — real-world data layer)
│       ├── base/TelematicsAdapter.ts
│       ├── obd2/OBD2Adapter.ts
│       ├── ford-f450/FordF450Adapter.ts
│       ├── caterpillar/CaterpillarAdapter.ts
│       ├── john-deere-139/JohnDeere139Adapter.ts
│       └── registry.ts
├── scenarios/              ← YOU OWN (JSON scenario files)
│   ├── overheat_highway.json
│   ├── harsh_brake_city.json
│   ├── cold_start_normal.json
│   ├── oscillating_fault.json
│   └── real_world/         ← mapped from real telematics
├── policy/                 ← YOU OWN
│   ├── policy.yaml
│   └── driver_profile.yaml
└── gate-tests/             ← Person C owns (you consult)
```

---

## Part 2: Contracts — Write This First, Then Freeze

**File: `packages/contracts/src/index.ts`**

This is the single source of truth. All three engineers code against this. Do not change after Day 1 merge without all-team sign-off.

```typescript
// packages/contracts/src/index.ts
// AnomEdge Shared Contracts — Version 1.0 — FROZEN after Day 1 merge

export type Severity = 'NORMAL' | 'WATCH' | 'WARN' | 'HIGH' | 'CRITICAL';

// ─── Raw Signal (from Simulator or real telematics adapter) ──────────────────
export interface SignalEvent {
  ts:          number;           // Unix ms timestamp
  asset_id:    string;           // Vehicle identifier e.g. "TRUCK-001"
  driver_id:   string;           // e.g. "DRV-042"
  source:      SignalSource;     // Which telematics adapter produced this
  signals:     SignalMap;        // Key-value of all PID readings
  raw_frame?:  unknown;          // Original bytes (optional, for debugging)
}

export type SignalSource =
  | 'SIMULATOR'
  | 'OBD2_GENERIC'
  | 'FORD_F450'
  | 'CAT_HEAVY'
  | 'JOHN_DEERE_139'
  | 'CUSTOM';

export interface SignalMap {
  // Common OBD-II signals (all vehicles)
  coolant_temp?:        number;   // °C
  engine_rpm?:          number;   // RPM
  vehicle_speed?:       number;   // km/h
  throttle_position?:   number;   // %
  engine_load?:         number;   // %
  fuel_level?:          number;   // %
  intake_air_temp?:     number;   // °C
  battery_voltage?:     number;   // V
  brake_pedal?:         number;   // 0=off, 1=on or % pressure
  oil_pressure?:        number;   // kPa
  dtc_codes?:           string[]; // Diagnostic Trouble Codes

  // Heavy fleet extensions (Cat / JD139 / F450 work)
  hydraulic_pressure?:  number;   // kPa — Cat/JD specific
  transmission_temp?:   number;   // °C
  axle_weight?:         number;   // kg
  pto_rpm?:             number;   // Power Take-Off RPM
  boom_position?:       number;   // degrees — Cat excavator
  load_weight?:         number;   // kg — JD haul trucks
  def_level?:           number;   // % — Diesel Exhaust Fluid
  adblue_level?:        number;   // % — alternative name
  boost_pressure?:      number;   // kPa — turbo
  exhaust_temp?:        number;   // °C

  // Allow arbitrary additional signals from any adapter
  [key: string]: number | string | string[] | undefined;
}

// ─── Feature Window (computed by FeatureEngine) ──────────────────────────────
export interface FeatureWindow {
  ts:                   number;
  asset_id:             string;
  window_seconds:       number;   // rolling window size (default 30)
  coolant_slope:        number;   // °C per second (positive = heating)
  brake_spike_count:    number;   // sudden brake events in window
  speed_mean:           number;   // km/h average
  rpm_mean:             number;
  engine_load_mean:     number;
  throttle_variance:    number;   // smoothness indicator
  hydraulic_spike:      boolean;  // heavy fleet: pressure anomaly
  transmission_heat:    boolean;  // heavy fleet: overtemp flag
  dtc_new:              string[]; // new DTC codes since last window
  signals_snapshot:     Partial<SignalMap>; // last known values
}

// ─── Decision (from RuleEngine) ──────────────────────────────────────────────
export interface Decision {
  ts:             number;
  asset_id:       string;
  severity:       Severity;
  rule_id:        string;     // e.g. "coolant_overheat_critical"
  rule_group:     RuleGroup;
  confidence:     number;     // 0.0–1.0
  triggered_by:   string[];   // which feature(s) fired this rule
  raw_value:      number;     // the value that crossed threshold
  threshold:      number;     // the threshold it crossed
  context:        Partial<FeatureWindow>;
}

export type RuleGroup =
  | 'thermal'
  | 'braking'
  | 'speed'
  | 'hydraulic'
  | 'electrical'
  | 'dtc'
  | 'transmission'
  | 'fuel'
  | 'composite';

// ─── Action (output to mobile after TrustEngine + GuidanceEngine) ────────────
export interface Action {
  seq:          number;       // monotonic sequence number
  ts:           number;
  asset_id:     string;
  severity:     Severity;
  title:        string;       // Short: "Coolant Overheating"
  guidance:     string;       // Full operator instruction
  rule_id:      string;
  speak:        boolean;      // TTS fires if true (HIGH/CRITICAL)
  acknowledged: boolean;
  source:       'TEMPLATE' | 'LLM';
}

// ─── Policy Pack (loaded from YAML) ──────────────────────────────────────────
export interface PolicyPack {
  version:        string;
  vehicle_class:  VehicleClass;
  rules:          PolicyRule[];
}

export type VehicleClass =
  | 'LIGHT_TRUCK'       // Ford F450, pickups
  | 'HEAVY_EQUIPMENT'   // Cat, JD139
  | 'FLEET_DIESEL'      // Generic long-haul
  | 'PASSENGER'
  | 'SIMULATOR';

export interface PolicyRule {
  id:          string;
  group:       RuleGroup;
  signal:      string;       // FeatureWindow field or derived
  operator:    'gt' | 'lt' | 'gte' | 'lte' | 'eq' | 'contains';
  threshold:   number;
  severity:    Severity;
  cooldown_ms: number;       // minimum ms between same-rule alerts
  hysteresis:  number;       // must exceed threshold by this to re-fire
  description: string;
}

// ─── EventEnvelope (wraps all bus messages) ──────────────────────────────────
export interface EventEnvelope<T = unknown> {
  id:      string;           // UUID
  topic:   BusTopic;
  seq:     number;
  ts:      number;
  payload: T;
}

export type BusTopic =
  | 'signals.raw'
  | 'signals.features'
  | 'decisions'
  | 'decisions.gated'
  | 'actions'
  | 'telemetry.sync'
  | 'model.ota'
  | 'system.heartbeat'
  | 'system.error';
```

---

## Part 3: Telematics Adapter System — Real-World Vehicle Data

This is the new layer that maps raw telematics frames from any real vehicle into the standard `SignalEvent` that the rest of the system understands.

### 3.1 Base Adapter Interface

**File: `adapters/telematics/base/TelematicsAdapter.ts`**

```typescript
import { SignalEvent, SignalSource } from '@anomedge/contracts';

export interface TelematicsConfig {
  asset_id:       string;
  driver_id:      string;
  vehicle_class:  string;
  options?:       Record<string, unknown>;
}

export interface RawTelematicsFrame {
  timestamp?:     number;
  raw?:           Buffer | string | Record<string, unknown>;
  pid_readings?:  Record<string, number | string>;
  j1939_spns?:    Record<string, number>;   // heavy fleet J1939 SPNs
  can_frames?:    CanFrame[];
}

export interface CanFrame {
  id:     number;   // CAN ID
  data:   number[]; // 8 bytes
  ts:     number;
}

export abstract class TelematicsAdapter {
  protected config: TelematicsConfig;

  constructor(config: TelematicsConfig) {
    this.config = config;
  }

  /** Normalize any raw frame into a standard SignalEvent */
  abstract normalize(frame: RawTelematicsFrame): SignalEvent;

  /** Return the source identifier for this adapter */
  abstract get source(): SignalSource;

  /** Validate that a raw frame has minimum required fields */
  abstract validate(frame: RawTelematicsFrame): boolean;

  /** Optional: list of PID/SPN codes this adapter supports */
  supportedSignals(): string[] {
    return [];
  }

  protected timestampNow(): number {
    return Date.now();
  }

  protected clamp(val: number, min: number, max: number): number {
    return Math.max(min, Math.min(max, val));
  }
}
```

---

### 3.2 Generic OBD-II Adapter

**File: `adapters/telematics/obd2/OBD2Adapter.ts`**

Works with any standard OBD-II ELM327 dongle output (Mode 01 PIDs).

```typescript
import { SignalEvent, SignalMap } from '@anomedge/contracts';
import { TelematicsAdapter, RawTelematicsFrame } from '../base/TelematicsAdapter';

// Standard OBD-II PID mappings
const OBD2_PID_MAP: Record<string, keyof SignalMap> = {
  '0105': 'coolant_temp',        // Engine Coolant Temperature
  '010C': 'engine_rpm',          // Engine RPM
  '010D': 'vehicle_speed',       // Vehicle Speed
  '0111': 'throttle_position',   // Throttle Position
  '0104': 'engine_load',         // Calculated Engine Load
  '012F': 'fuel_level',          // Fuel Tank Level Input
  '010F': 'intake_air_temp',     // Intake Air Temperature
  '0142': 'battery_voltage',     // Control Module Voltage
  '010A': 'fuel_pressure',       // Fuel Pressure
  '010B': 'boost_pressure',      // Intake Manifold Pressure
};

export class OBD2Adapter extends TelematicsAdapter {
  get source() { return 'OBD2_GENERIC' as const; }

  validate(frame: RawTelematicsFrame): boolean {
    return !!(frame.pid_readings && Object.keys(frame.pid_readings).length > 0);
  }

  normalize(frame: RawTelematicsFrame): SignalEvent {
    const signals: SignalMap = {};

    for (const [pid, value] of Object.entries(frame.pid_readings ?? {})) {
      const pidUpper = pid.toUpperCase();
      const mappedKey = OBD2_PID_MAP[pidUpper];
      if (mappedKey) {
        signals[mappedKey] = this.decodeOBD2Value(pidUpper, Number(value));
      }
    }

    // Handle DTC codes if present
    if (frame.raw && typeof frame.raw === 'object') {
      const raw = frame.raw as Record<string, unknown>;
      if (Array.isArray(raw['dtc_codes'])) {
        signals.dtc_codes = raw['dtc_codes'] as string[];
      }
    }

    return {
      ts:       frame.timestamp ?? this.timestampNow(),
      asset_id: this.config.asset_id,
      driver_id: this.config.driver_id,
      source:   this.source,
      signals,
      raw_frame: frame.raw,
    };
  }

  private decodeOBD2Value(pid: string, raw: number): number {
    switch (pid) {
      case '0105': return raw - 40;                    // °C: A-40
      case '010C': return (raw * 256) / 4;             // RPM: ((A*256)+B)/4  (simplified)
      case '010D': return raw;                         // km/h: A
      case '0111': return (raw / 255) * 100;           // %: A*100/255
      case '0104': return (raw / 255) * 100;           // %
      case '010F': return raw - 40;                    // °C
      case '0142': return raw / 1000;                  // V: raw is mV
      default:     return raw;
    }
  }

  supportedSignals(): string[] {
    return Object.keys(OBD2_PID_MAP);
  }
}
```

---

### 3.3 Ford F450 Adapter

**File: `adapters/telematics/ford-f450/FordF450Adapter.ts`**

Ford F450 Super Duty uses Ford-specific PIDs on top of OBD-II, plus FordPass/SYNC telematics JSON format.

```typescript
import { SignalEvent, SignalMap } from '@anomedge/contracts';
import { TelematicsAdapter, RawTelematicsFrame } from '../base/TelematicsAdapter';

// Ford-specific enhanced PID extensions (Ford PIDS over CAN)
const FORD_F450_PID_MAP: Record<string, keyof SignalMap> = {
  // Standard OBD-II
  '0105': 'coolant_temp',
  '010C': 'engine_rpm',
  '010D': 'vehicle_speed',
  '0111': 'throttle_position',
  '0104': 'engine_load',
  '012F': 'fuel_level',
  '0142': 'battery_voltage',
  // Ford-specific extended PIDs (Mode 22)
  '22FF00': 'oil_pressure',
  '22FF01': 'transmission_temp',
  '22FF02': 'axle_weight',
  '22FF03': 'def_level',           // Diesel Exhaust Fluid (6.7L Power Stroke)
  '22FF04': 'boost_pressure',
  '22FF05': 'exhaust_temp',
  '22FF06': 'pto_rpm',
};

// FordPass Connect JSON field mapping
const FORDPASS_FIELD_MAP: Record<string, keyof SignalMap> = {
  'engineCoolantTemp':     'coolant_temp',
  'engineRpm':             'engine_rpm',
  'speed':                 'vehicle_speed',
  'throttlePosition':      'throttle_position',
  'fuelLevel':             'fuel_level',
  'batteryVoltage':        'battery_voltage',
  'oilPressure':           'oil_pressure',
  'transmissionFluidTemp': 'transmission_temp',
  'gvwr':                  'axle_weight',
  'defFluidLevel':         'def_level',
  'boostPressure':         'boost_pressure',
  'exhaustGasTemp':        'exhaust_temp',
};

export class FordF450Adapter extends TelematicsAdapter {
  get source() { return 'FORD_F450' as const; }

  validate(frame: RawTelematicsFrame): boolean {
    return !!(
      (frame.pid_readings && Object.keys(frame.pid_readings).length > 0) ||
      (frame.raw && typeof frame.raw === 'object')
    );
  }

  normalize(frame: RawTelematicsFrame): SignalEvent {
    const signals: SignalMap = {};

    // 1. Decode PID readings (OBD-II + Ford extended Mode 22)
    for (const [pid, value] of Object.entries(frame.pid_readings ?? {})) {
      const pidUpper = pid.toUpperCase();
      const mappedKey = FORD_F450_PID_MAP[pidUpper];
      if (mappedKey) {
        signals[mappedKey] = this.decodeFordValue(pidUpper, Number(value));
      }
    }

    // 2. Decode FordPass JSON telemetry (cloud-sourced or Sync 4 export)
    if (frame.raw && typeof frame.raw === 'object') {
      const raw = frame.raw as Record<string, unknown>;
      for (const [field, value] of Object.entries(raw)) {
        const mappedKey = FORDPASS_FIELD_MAP[field];
        if (mappedKey && typeof value === 'number') {
          signals[mappedKey] = value;
        }
      }
      // DTC codes from Ford format
      if (Array.isArray(raw['dtcCodes'])) {
        signals.dtc_codes = raw['dtcCodes'] as string[];
      }
      // F450 payload / towing weight
      if (typeof raw['payloadWeight'] === 'number') {
        signals.axle_weight = raw['payloadWeight'] as number;
      }
    }

    // 3. Decode raw CAN frames (direct CAN bus tap)
    if (frame.can_frames) {
      this.decodeCanFrames(frame.can_frames, signals);
    }

    return {
      ts:        frame.timestamp ?? this.timestampNow(),
      asset_id:  this.config.asset_id,
      driver_id: this.config.driver_id,
      source:    this.source,
      signals,
      raw_frame: frame.raw,
    };
  }

  private decodeFordValue(pid: string, raw: number): number {
    switch (pid) {
      case '0105':   return raw - 40;                  // coolant °C
      case '010C':   return raw / 4;                   // RPM
      case '010D':   return raw;                       // km/h
      case '22FF00': return raw * 0.1;                 // oil pressure kPa
      case '22FF01': return raw - 40;                  // trans temp °C
      case '22FF02': return raw * 10;                  // axle weight kg
      case '22FF03': return (raw / 255) * 100;         // DEF level %
      case '22FF04': return raw * 0.5;                 // boost kPa
      case '22FF05': return raw * 2;                   // exhaust °C
      default:       return raw;
    }
  }

  private decodeCanFrames(frames: Array<{id: number; data: number[]; ts: number}>, signals: SignalMap): void {
    // Ford F450 known CAN IDs on MS-CAN (Medium Speed CAN @ 125kbps)
    for (const frame of frames) {
      switch (frame.id) {
        case 0x3B3: // Engine data frame
          if (frame.data.length >= 4) {
            signals.engine_rpm = ((frame.data[0] << 8) | frame.data[1]) * 0.25;
            signals.engine_load = frame.data[2] * 0.392;
          }
          break;
        case 0x420: // Transmission status
          if (frame.data.length >= 2) {
            signals.transmission_temp = frame.data[1] - 40;
          }
          break;
        case 0x217: // Vehicle speed
          if (frame.data.length >= 2) {
            signals.vehicle_speed = ((frame.data[0] << 8) | frame.data[1]) * 0.01;
          }
          break;
      }
    }
  }
}
```

---

### 3.4 Caterpillar Heavy Equipment Adapter

**File: `adapters/telematics/caterpillar/CaterpillarAdapter.ts`**

Cat uses J1939 (heavy-duty CAN protocol) via SPN/PGN codes. Common on 320/336 excavators, 745/777 haul trucks, D6/D8 dozers.

```typescript
import { SignalEvent, SignalMap } from '@anomedge/contracts';
import { TelematicsAdapter, RawTelematicsFrame } from '../base/TelematicsAdapter';

// J1939 SPN (Suspect Parameter Numbers) to signal mapping
// Source: SAE J1939-71 standard + Cat-specific SPNs
const CAT_J1939_SPN_MAP: Record<number, keyof SignalMap> = {
  // Standard J1939 Engine SPNs
  110:  'coolant_temp',         // Engine Coolant Temperature (°C, offset -40)
  190:  'engine_rpm',           // Engine Speed (RPM, resolution 0.125)
  84:   'vehicle_speed',        // Wheel-Based Vehicle Speed (km/h, res 0.00390625)
  91:   'throttle_position',    // Accelerator Pedal Position 1 (%, res 0.4)
  92:   'engine_load',          // Engine Percent Load At Current Speed
  96:   'fuel_level',           // Fuel Level 1 (%, res 0.4)
  172:  'intake_air_temp',      // Engine Air Intake Temperature
  168:  'battery_voltage',      // Battery Potential / Power Input 1 (V, res 0.05)
  100:  'oil_pressure',         // Engine Oil Pressure (kPa, res 4)
  175:  'oil_temp',             // Engine Oil Temperature 1
  4076: 'def_level',            // Aftertreatment 1 Diesel Exhaust Fluid Tank Level
  3251: 'exhaust_temp',         // Aftertreatment Outlet NOx 1 (proxy)

  // Cat-specific heavy equipment SPNs
  // Hydraulic system
  2413: 'hydraulic_pressure',   // Hydraulic System Pressure (kPa)
  2414: 'hydraulic_oil_temp',   // Hydraulic Oil Temperature
  // Transmission
  127:  'transmission_temp',    // Transmission Oil Temperature 1
  523:  'trans_current_gear',   // Transmission Current Gear
  // Work tool / attachment
  2431: 'boom_position',        // Work Tool Position (degrees, excavator boom)
  2432: 'stick_position',       // Stick cylinder position
  1480: 'pto_rpm',              // Auxiliary-Driven Equipment Speed
  // Load management (haul trucks)
  1430: 'load_weight',          // Payload Mass (kg, res 2)
  1431: 'payload_percent',      // Payload as % of max capacity
  // Fuel system
  183:  'fuel_rate',            // Engine Fuel Rate (L/hr, res 0.05)
  182:  'trip_fuel',            // Trip Fuel (L)
  // Brake system
  521:  'brake_demand',         // Engine Retarder Torque Mode
  // DPF/Emissions
  3700: 'dpf_status',           // Diesel Particulate Filter Status
};

// Cat Machine Security System / VisionLink field map (JSON API)
const VISIONLINK_FIELD_MAP: Record<string, keyof SignalMap> = {
  'engineCoolantTemperature':     'coolant_temp',
  'engineSpeed':                   'engine_rpm',
  'groundSpeed':                   'vehicle_speed',
  'fuelLevel':                     'fuel_level',
  'hydraulicSystemPressure':       'hydraulic_pressure',
  'hydraulicOilTemperature':       'hydraulic_pressure',  // mapped to nearest
  'transmissionOilTemperature':    'transmission_temp',
  'payloadWeight':                 'load_weight',
  'engineLoad':                    'engine_load',
  'batteryVoltage':                'battery_voltage',
  'dieselExhaustFluid':            'def_level',
  'engineOilPressure':             'oil_pressure',
};

export class CaterpillarAdapter extends TelematicsAdapter {
  get source() { return 'CAT_HEAVY' as const; }

  validate(frame: RawTelematicsFrame): boolean {
    return !!(
      (frame.j1939_spns && Object.keys(frame.j1939_spns).length > 0) ||
      (frame.raw && typeof frame.raw === 'object')
    );
  }

  normalize(frame: RawTelematicsFrame): SignalEvent {
    const signals: SignalMap = {};

    // 1. Decode J1939 SPN readings (primary path for direct telematics)
    for (const [spnStr, rawValue] of Object.entries(frame.j1939_spns ?? {})) {
      const spn = parseInt(spnStr, 10);
      const mappedKey = CAT_J1939_SPN_MAP[spn];
      if (mappedKey) {
        signals[mappedKey] = this.decodeJ1939Value(spn, rawValue);
      }
    }

    // 2. Decode Cat VisionLink JSON API response
    if (frame.raw && typeof frame.raw === 'object') {
      const raw = frame.raw as Record<string, unknown>;
      for (const [field, value] of Object.entries(raw)) {
        const mappedKey = VISIONLINK_FIELD_MAP[field];
        if (mappedKey && typeof value === 'number') {
          signals[mappedKey] = value;
        }
      }
      // Cat fault codes use different format
      if (Array.isArray(raw['activeFaultCodes'])) {
        signals.dtc_codes = (raw['activeFaultCodes'] as Array<{code: string}>)
          .map(f => `CAT-${f.code}`);
      }
      // Machine hours instead of odometer
      if (typeof raw['machineHours'] === 'number') {
        signals['machine_hours'] = raw['machineHours'] as number;
      }
    }

    // 3. Decode raw CAN/J1939 frames if provided
    if (frame.can_frames) {
      this.decodeJ1939Frames(frame.can_frames, signals);
    }

    return {
      ts:        frame.timestamp ?? this.timestampNow(),
      asset_id:  this.config.asset_id,
      driver_id: this.config.driver_id,
      source:    this.source,
      signals,
      raw_frame: frame.raw,
    };
  }

  private decodeJ1939Value(spn: number, raw: number): number {
    // Apply J1939-71 standard scaling
    switch (spn) {
      case 110:  return raw * 0.03125 - 273;       // Engine coolant temp (K to °C)
      case 190:  return raw * 0.125;               // RPM
      case 84:   return raw * 0.00390625;          // Vehicle speed km/h
      case 91:   return raw * 0.4;                 // Throttle %
      case 92:   return raw * 0.4;                 // Engine load %
      case 96:   return raw * 0.4;                 // Fuel level %
      case 100:  return raw * 4;                   // Oil pressure kPa
      case 168:  return raw * 0.05;                // Battery V
      case 2413: return raw * 16;                  // Hydraulic pressure kPa (Cat-specific scale)
      case 127:  return raw - 273;                 // Transmission temp
      case 1430: return raw * 2;                   // Load weight kg
      case 183:  return raw * 0.05;                // Fuel rate L/hr
      default:   return raw;
    }
  }

  private decodeJ1939Frames(frames: Array<{id: number; data: number[]; ts: number}>, signals: SignalMap): void {
    for (const frame of frames) {
      const pgn = (frame.id >> 8) & 0xFFFF; // Extract PGN from 29-bit J1939 ID
      switch (pgn) {
        case 0xF004: // Electronic Engine Controller 1 (EEC1)
          if (frame.data.length >= 4) {
            signals.engine_rpm = ((frame.data[3] << 8) | frame.data[2]) * 0.125;
          }
          break;
        case 0xF005: // Electronic Engine Controller 2 (EEC2)
          if (frame.data.length >= 2) {
            signals.throttle_position = frame.data[1] * 0.4;
            signals.engine_load = frame.data[2] * 0.4;
          }
          break;
        case 0xFEEE: // Engine Temperature
          if (frame.data.length >= 2) {
            signals.coolant_temp = frame.data[0] - 40;
            signals.oil_temp = frame.data[2] - 40;
          }
          break;
        case 0xF001: // Transmission Control 1
          if (frame.data.length >= 3) {
            signals.transmission_temp = frame.data[3] - 40;
          }
          break;
      }
    }
  }
}
```

---

### 3.5 John Deere 139 Adapter

**File: `adapters/telematics/john-deere-139/JohnDeere139Adapter.ts`**

JD 139 refers to John Deere's telematics platform (JDLink/JD Operations Center). Used in 700K/800K dozers, 620G/672G graders, 350G/380G excavators, and 460E haul trucks used in mining.

```typescript
import { SignalEvent, SignalMap } from '@anomedge/contracts';
import { TelematicsAdapter, RawTelematicsFrame } from '../base/TelematicsAdapter';

// John Deere uses J1939 + JD-proprietary SPNs on JDLink API
const JD_J1939_SPN_MAP: Record<number, keyof SignalMap> = {
  // Standard J1939 (same as Cat base)
  110:  'coolant_temp',
  190:  'engine_rpm',
  84:   'vehicle_speed',
  91:   'throttle_position',
  92:   'engine_load',
  96:   'fuel_level',
  168:  'battery_voltage',
  100:  'oil_pressure',
  127:  'transmission_temp',
  4076: 'def_level',

  // JD-specific proprietary SPNs
  // Hydraulic
  520204: 'hydraulic_pressure',     // JD: Implement hydraulic pressure
  520205: 'hydraulic_oil_temp',     // JD: Hydraulic oil temperature
  520206: 'boom_position',          // JD: Loader lift arm angle
  // Load management (460E/370E haul trucks)
  520207: 'load_weight',            // JD: Body payload weight
  520208: 'payload_percent',        // JD: Payload % rated capacity
  // Powertrain
  520209: 'pto_rpm',                // JD: Ground drive motor speed
  520210: 'boost_pressure',         // JD: Turbocharger boost pressure
  520211: 'exhaust_temp',           // JD: SCR inlet temperature
  // JD Active Seat / Grade control
  520212: 'blade_position',         // JD: Blade cross-slope (672G grader)
  520213: 'blade_down_force',       // JD: Blade draft force
  // Emissions
  520214: 'dpf_soot_level',         // JD: DPF soot loading %
  520215: 'dpf_ash_level',          // JD: DPF ash loading %
};

// JDLink REST API JSON field mapping (Operations Center API v3)
const JDLINK_API_MAP: Record<string, keyof SignalMap> = {
  'engineCoolantTemperature':     'coolant_temp',
  'engineSpeed':                   'engine_rpm',
  'groundSpeed':                   'vehicle_speed',
  'throttlePosition':              'throttle_position',
  'engineLoad':                    'engine_load',
  'fuelLevelPercent':              'fuel_level',
  'batteryVoltage':                'battery_voltage',
  'engineOilPressure':             'oil_pressure',
  'hydraulicOilPressure':          'hydraulic_pressure',
  'hydraulicOilTemperature':       'hydraulic_pressure',   // nearest field
  'transmissionOilTemperature':    'transmission_temp',
  'exhaustFluidLevel':             'def_level',            // DEF/AdBlue
  'payloadMass':                   'load_weight',
  'boostPressure':                 'boost_pressure',
  'exhaustTemperature':            'exhaust_temp',
  'liftArmAngle':                  'boom_position',
};

export class JohnDeere139Adapter extends TelematicsAdapter {
  get source() { return 'JOHN_DEERE_139' as const; }

  validate(frame: RawTelematicsFrame): boolean {
    return !!(
      (frame.j1939_spns && Object.keys(frame.j1939_spns).length > 0) ||
      (frame.raw && typeof frame.raw === 'object')
    );
  }

  normalize(frame: RawTelematicsFrame): SignalEvent {
    const signals: SignalMap = {};

    // 1. J1939 SPN readings (direct ECM access or JDLink gateway)
    for (const [spnStr, rawValue] of Object.entries(frame.j1939_spns ?? {})) {
      const spn = parseInt(spnStr, 10);
      const mappedKey = JD_J1939_SPN_MAP[spn];
      if (mappedKey) {
        signals[mappedKey] = this.decodeJDValue(spn, rawValue);
      }
    }

    // 2. JDLink REST API JSON (via Operations Center API)
    if (frame.raw && typeof frame.raw === 'object') {
      const raw = frame.raw as Record<string, unknown>;

      // Handle JDLink "readings" array format
      if (Array.isArray(raw['readings'])) {
        for (const reading of raw['readings'] as Array<{name: string; value: number}>) {
          const mappedKey = JDLINK_API_MAP[reading.name];
          if (mappedKey) {
            signals[mappedKey] = reading.value;
          }
        }
      } else {
        // Flat JSON format
        for (const [field, value] of Object.entries(raw)) {
          const mappedKey = JDLINK_API_MAP[field];
          if (mappedKey && typeof value === 'number') {
            signals[mappedKey] = value;
          }
        }
      }

      // JD fault codes (different format from standard OBD)
      if (Array.isArray(raw['activeDtcs'])) {
        signals.dtc_codes = (raw['activeDtcs'] as Array<{spn: number; fmi: number}>)
          .map(dtc => `JD-SPN${dtc.spn}-FMI${dtc.fmi}`);
      }

      // Machine hours
      if (typeof raw['engineHours'] === 'number') {
        signals['machine_hours'] = raw['engineHours'] as number;
      }
    }

    return {
      ts:        frame.timestamp ?? this.timestampNow(),
      asset_id:  this.config.asset_id,
      driver_id: this.config.driver_id,
      source:    this.source,
      signals,
      raw_frame: frame.raw,
    };
  }

  private decodeJDValue(spn: number, raw: number): number {
    // JD uses same J1939 scaling for standard SPNs
    switch (spn) {
      case 110:    return raw * 0.03125 - 273;     // coolant temp
      case 190:    return raw * 0.125;             // RPM
      case 84:     return raw * 0.00390625;        // speed
      case 91:     return raw * 0.4;               // throttle
      case 92:     return raw * 0.4;               // engine load
      case 96:     return raw * 0.4;               // fuel level
      case 168:    return raw * 0.05;              // battery V
      case 100:    return raw * 4;                 // oil pressure kPa
      // JD proprietary (custom scales from JD service manual)
      case 520204: return raw * 10;                // hydraulic pressure kPa
      case 520206: return raw * 0.1;               // boom angle degrees
      case 520207: return raw * 100;               // payload weight kg
      case 520214: return raw * 0.4;               // DPF soot %
      default:     return raw;
    }
  }
}
```

---

### 3.6 Adapter Registry

**File: `adapters/telematics/registry.ts`**

The single place to get the right adapter for any vehicle. Loaded from config YAML.

```typescript
import { TelematicsAdapter, TelematicsConfig } from './base/TelematicsAdapter';
import { OBD2Adapter }         from './obd2/OBD2Adapter';
import { FordF450Adapter }     from './ford-f450/FordF450Adapter';
import { CaterpillarAdapter }  from './caterpillar/CaterpillarAdapter';
import { JohnDeere139Adapter } from './john-deere-139/JohnDeere139Adapter';

export type AdapterType = 'OBD2_GENERIC' | 'FORD_F450' | 'CAT_HEAVY' | 'JOHN_DEERE_139';

export function createAdapter(type: AdapterType, config: TelematicsConfig): TelematicsAdapter {
  switch (type) {
    case 'OBD2_GENERIC':   return new OBD2Adapter(config);
    case 'FORD_F450':      return new FordF450Adapter(config);
    case 'CAT_HEAVY':      return new CaterpillarAdapter(config);
    case 'JOHN_DEERE_139': return new JohnDeere139Adapter(config);
    default:
      throw new Error(`Unknown adapter type: ${type}`);
  }
}

// Load fleet config from YAML and instantiate all adapters
export interface FleetAdapterConfig {
  assets: Array<{
    asset_id:    string;
    driver_id:   string;
    adapter:     AdapterType;
    description: string;
    options?:    Record<string, unknown>;
  }>;
}

export function createFleetAdapters(config: FleetAdapterConfig): Map<string, TelematicsAdapter> {
  const adapters = new Map<string, TelematicsAdapter>();
  for (const asset of config.assets) {
    adapters.set(asset.asset_id, createAdapter(asset.adapter, {
      asset_id:      asset.asset_id,
      driver_id:     asset.driver_id,
      vehicle_class: asset.adapter,
      options:       asset.options,
    }));
  }
  return adapters;
}
```

---

### 3.7 Fleet Config YAML

**File: `policy/fleet-adapters.yaml`**

```yaml
# AnomEdge Fleet Telematics Adapter Configuration
# Each asset maps to one adapter type. Add new vehicles here.

assets:
  # ── Simulator (development & demo) ──────────────────────────
  - asset_id:    "SIM-001"
    driver_id:   "DRV-SIM"
    adapter:     "OBD2_GENERIC"
    description: "Simulated vehicle for Phase 0 development"

  # ── Light Trucks (Ford F450) ─────────────────────────────────
  - asset_id:    "F450-001"
    driver_id:   "DRV-042"
    adapter:     "FORD_F450"
    description: "Ford F450 Super Duty — BC Hydro service fleet"
    options:
      can_bus:   "MS-CAN"        # Medium Speed CAN
      protocol:  "FORDPASS_JSON" # Use FordPass Connect API

  - asset_id:    "F450-002"
    driver_id:   "DRV-089"
    adapter:     "FORD_F450"
    description: "Ford F450 — construction site support"
    options:
      can_bus:   "HS-CAN"        # High Speed CAN for direct tap

  # ── Caterpillar Heavy Equipment ──────────────────────────────
  - asset_id:    "CAT-320-001"
    driver_id:   "DRV-101"
    adapter:     "CAT_HEAVY"
    description: "Cat 320 Excavator — mining site Alpha"
    options:
      protocol:  "VISIONLINK"    # Cat telematics API
      machine_model: "320GX"
      j1939_address: 0x00

  - asset_id:    "CAT-745-001"
    driver_id:   "DRV-115"
    adapter:     "CAT_HEAVY"
    description: "Cat 745 Articulated Haul Truck — quarry"
    options:
      protocol:  "J1939_DIRECT"
      machine_model: "745"
      payload_rated_kg: 41000

  - asset_id:    "CAT-D8-001"
    driver_id:   "DRV-119"
    adapter:     "CAT_HEAVY"
    description: "Cat D8 Dozer — land clearing"
    options:
      protocol:  "VISIONLINK"
      machine_model: "D8T"

  # ── John Deere Construction ──────────────────────────────────
  - asset_id:    "JD-460E-001"
    driver_id:   "DRV-201"
    adapter:     "JOHN_DEERE_139"
    description: "JD 460E Haul Truck — open pit mine"
    options:
      protocol:  "JDLINK_API"    # JD Operations Center API
      api_version: "v3"
      payload_rated_kg: 55000

  - asset_id:    "JD-800K-001"
    driver_id:   "DRV-215"
    adapter:     "JOHN_DEERE_139"
    description: "JD 800K Dozer — reclamation site"
    options:
      protocol:  "J1939_DIRECT"
      machine_model: "800K"

  - asset_id:    "JD-620G-001"
    driver_id:   "DRV-222"
    adapter:     "JOHN_DEERE_139"
    description: "JD 620G Motor Grader — road construction"
    options:
      protocol:  "JDLINK_API"
      machine_model: "620G"
```

---

## Part 4: Policy YAML — Thresholds Per Vehicle Class

**File: `policy/policy.yaml`**

```yaml
# AnomEdge Policy Pack v1.0
# Thresholds are per vehicle class. 
# Heavy equipment runs HOTTER and at different RPM than passenger vehicles.

policies:
  - vehicle_class: LIGHT_TRUCK      # Ford F450 and similar
    version: "1.0.0"
    rules:
      - id: coolant_overheat_critical
        group: thermal
        signal: coolant_slope
        operator: gt
        threshold: 0.8        # rising >0.8°C/sec = critical
        severity: CRITICAL
        cooldown_ms: 30000
        hysteresis: 0.1
        description: "Rapid coolant temperature rise — engine overheating"

      - id: coolant_high_temp
        group: thermal
        signal: signals_snapshot.coolant_temp
        operator: gt
        threshold: 108         # °C — F450 thermostat opens at 93°C
        severity: HIGH
        cooldown_ms: 60000
        hysteresis: 3
        description: "Engine coolant above safe operating range"

      - id: harsh_brake_event
        group: braking
        signal: brake_spike_count
        operator: gte
        threshold: 3
        severity: WARN
        cooldown_ms: 10000
        hysteresis: 1
        description: "Repeated harsh braking events detected"

      - id: def_low_critical
        group: fuel
        signal: signals_snapshot.def_level
        operator: lt
        threshold: 10          # % — triggers derate at 5%
        severity: HIGH
        cooldown_ms: 300000
        hysteresis: 2
        description: "DEF fluid critically low — engine derate imminent"

      - id: transmission_overheat
        group: transmission
        signal: signals_snapshot.transmission_temp
        operator: gt
        threshold: 120         # °C
        severity: HIGH
        cooldown_ms: 45000
        hysteresis: 5
        description: "Transmission overheating — reduce load or pull over"

  - vehicle_class: HEAVY_EQUIPMENT  # Cat + JD139
    version: "1.0.0"
    rules:
      - id: hydraulic_pressure_critical
        group: hydraulic
        signal: hydraulic_spike
        operator: eq
        threshold: 1           # boolean flag
        severity: CRITICAL
        cooldown_ms: 15000
        hysteresis: 0
        description: "Hydraulic pressure spike — potential line failure"

      - id: coolant_overheat_critical
        group: thermal
        signal: coolant_slope
        operator: gt
        threshold: 0.5         # Heavy equipment heats slower — lower slope
        severity: CRITICAL
        cooldown_ms: 30000
        hysteresis: 0.05
        description: "Engine overheating — shut down to prevent damage"

      - id: coolant_high_heavy
        group: thermal
        signal: signals_snapshot.coolant_temp
        operator: gt
        threshold: 115         # °C — heavy diesel runs hotter (vs 108 for light)
        severity: HIGH
        cooldown_ms: 60000
        hysteresis: 5
        description: "Coolant temperature above heavy equipment safe range"

      - id: overload_payload
        group: composite
        signal: signals_snapshot.payload_percent
        operator: gt
        threshold: 110         # % — 10% overload
        severity: WARN
        cooldown_ms: 120000
        hysteresis: 3
        description: "Machine operating above rated payload capacity"

      - id: def_low_critical
        group: fuel
        signal: signals_snapshot.def_level
        operator: lt
        threshold: 15          # % — give more warning lead time on heavy equipment
        severity: HIGH
        cooldown_ms: 300000
        hysteresis: 2
        description: "DEF/AdBlue critically low — emissions derate imminent"

      - id: transmission_overheat_heavy
        group: transmission
        signal: transmission_heat
        operator: eq
        threshold: 1
        severity: HIGH
        cooldown_ms: 45000
        hysteresis: 0
        description: "Transmission overtemperature — reduce duty cycle"

      - id: hydraulic_temp_high
        group: hydraulic
        signal: signals_snapshot.hydraulic_oil_temp
        operator: gt
        threshold: 95          # °C — Cat/JD hydraulic system limit
        severity: WARN
        cooldown_ms: 60000
        hysteresis: 3
        description: "Hydraulic oil temperature elevated — check cooler"
```

---

## Part 5: FeatureEngine + RuleEngine + TrustEngine

### 5.1 FeatureEngine

**File: `packages/core/src/FeatureEngine.ts`**

```typescript
import { SignalEvent, FeatureWindow, SignalMap } from '@anomedge/contracts';

const WINDOW_SECONDS = 30;

export class FeatureEngine {
  private windows = new Map<string, SignalEvent[]>();

  ingest(event: SignalEvent): FeatureWindow {
    if (!this.windows.has(event.asset_id)) {
      this.windows.set(event.asset_id, []);
    }
    const buf = this.windows.get(event.asset_id)!;
    buf.push(event);

    // Trim to window
    const cutoff = event.ts - WINDOW_SECONDS * 1000;
    while (buf.length > 0 && buf[0].ts < cutoff) buf.shift();

    return this.compute(event.asset_id, buf, event);
  }

  private compute(asset_id: string, buf: SignalEvent[], latest: SignalEvent): FeatureWindow {
    const coolantVals  = buf.map(e => e.signals.coolant_temp).filter(isNumber);
    const speedVals    = buf.map(e => e.signals.vehicle_speed).filter(isNumber);
    const rpmVals      = buf.map(e => e.signals.engine_rpm).filter(isNumber);
    const loadVals     = buf.map(e => e.signals.engine_load).filter(isNumber);
    const throttleVals = buf.map(e => e.signals.throttle_position).filter(isNumber);
    const brakeVals    = buf.map(e => e.signals.brake_pedal).filter(isNumber);
    const hydraulicVals = buf.map(e => e.signals.hydraulic_pressure).filter(isNumber);
    const transVals    = buf.map(e => e.signals.transmission_temp).filter(isNumber);

    // DTC delta: new codes not seen in previous window
    const prevDtcs = new Set(buf.slice(0, -1).flatMap(e => e.signals.dtc_codes ?? []));
    const latestDtcs = latest.signals.dtc_codes ?? [];
    const dtcNew = latestDtcs.filter(c => !prevDtcs.has(c));

    return {
      ts:              latest.ts,
      asset_id,
      window_seconds:  WINDOW_SECONDS,
      coolant_slope:   linearSlope(coolantVals),
      brake_spike_count: countSpikes(brakeVals, 0.8),
      speed_mean:      mean(speedVals),
      rpm_mean:        mean(rpmVals),
      engine_load_mean: mean(loadVals),
      throttle_variance: variance(throttleVals),
      hydraulic_spike:  hydraulicVals.length > 1 && maxDelta(hydraulicVals) > 500,
      transmission_heat: transVals.length > 0 && Math.max(...transVals) > 110,
      dtc_new:         dtcNew,
      signals_snapshot: latest.signals,
    };
  }
}

function isNumber(v: unknown): v is number {
  return typeof v === 'number' && !isNaN(v);
}
function mean(arr: number[]): number {
  return arr.length === 0 ? 0 : arr.reduce((a, b) => a + b, 0) / arr.length;
}
function variance(arr: number[]): number {
  if (arr.length < 2) return 0;
  const m = mean(arr);
  return mean(arr.map(v => (v - m) ** 2));
}
function linearSlope(arr: number[]): number {
  if (arr.length < 2) return 0;
  const first = arr[0], last = arr[arr.length - 1];
  return (last - first) / arr.length;
}
function countSpikes(arr: number[], threshold: number): number {
  return arr.filter((v, i) => i > 0 && v > threshold && arr[i - 1] <= threshold).length;
}
function maxDelta(arr: number[]): number {
  let max = 0;
  for (let i = 1; i < arr.length; i++) max = Math.max(max, Math.abs(arr[i] - arr[i - 1]));
  return max;
}
```

---

## Part 6: Claude Agent Prompts for Person A

Use these exact prompts in Claude Code (one per session, one bounded task each).

### Agent 1 — Contracts
```
Write packages/contracts/src/index.ts.

Define these TypeScript exports:
- Severity = 'NORMAL' | 'WATCH' | 'WARN' | 'HIGH' | 'CRITICAL'
- SignalSource: union of 5 adapter types + SIMULATOR
- SignalMap: interface with coolant_temp, engine_rpm, vehicle_speed,
  throttle_position, engine_load, fuel_level, battery_voltage, brake_pedal,
  oil_pressure, dtc_codes[], plus heavy fleet fields:
  hydraulic_pressure, transmission_temp, axle_weight, pto_rpm, boom_position,
  load_weight, def_level, boost_pressure, exhaust_temp
  plus [key: string] index for extensibility
- SignalEvent: ts, asset_id, driver_id, source, signals: SignalMap, raw_frame?
- FeatureWindow: ts, asset_id, window_seconds, coolant_slope, brake_spike_count,
  speed_mean, rpm_mean, engine_load_mean, throttle_variance, hydraulic_spike bool,
  transmission_heat bool, dtc_new string[], signals_snapshot
- Decision: ts, asset_id, severity, rule_id, rule_group RuleGroup, confidence,
  triggered_by string[], raw_value, threshold, context
- Action: seq, ts, asset_id, severity, title, guidance, rule_id, speak bool,
  acknowledged, source 'TEMPLATE'|'LLM'
- PolicyPack, PolicyRule, VehicleClass
- BusTopic: 9 topics as union
- EventEnvelope<T>: id, topic, seq, ts, payload

Write vitest tests: import every type, assert field shapes. All tests pass before committing.
```

### Agent 2 — Simulator Service
```
Write packages/simulator/src/SimulatorService.ts.

It reads a scenario JSON file from scenarios/ directory given by path.
It publishes SignalEvent objects to EventBus topic 'signals.raw' at a configurable
speed multiplier (1x = real time, 100x = for tests).

Scenario JSON format:
{
  "name": "overheat_highway",
  "vehicle_class": "LIGHT_TRUCK",
  "asset_id": "SIM-001",
  "driver_id": "DRV-SIM",
  "frames": [
    { "ts_offset_ms": 0, "signals": { "coolant_temp": 85, "engine_rpm": 2200 } },
    { "ts_offset_ms": 5000, "signals": { "coolant_temp": 92, "engine_rpm": 2400 } }
  ]
}

Implement:
- loadScenario(path: string): Promise<void>
- start(speedMultiplier: number): void
- stop(): void

Write vitest tests: 
1. loads scenario file
2. publishes correct number of frames
3. speed multiplier 10x completes in < 2 seconds for 5 frames
4. stop() halts publishing
All tests must pass before committing.
```

### Agent 3 — FeatureEngine
```
Write packages/core/src/FeatureEngine.ts.

Class FeatureEngine with method: ingest(event: SignalEvent): FeatureWindow

Maintains a 30-second rolling buffer per asset_id.
Computes:
- coolant_slope: linear slope of coolant_temp values in window (°C per sample)
- brake_spike_count: number of rising edges where brake_pedal goes from <0.8 to >=0.8
- speed_mean, rpm_mean, engine_load_mean: simple averages
- throttle_variance: variance of throttle_position
- hydraulic_spike: true if max delta between consecutive hydraulic_pressure > 500 kPa
- transmission_heat: true if any transmission_temp in window > 110°C
- dtc_new: DTC codes appearing in latest event not present in any prior event in window
- signals_snapshot: signals from the latest event

Write vitest tests with 6 scenarios:
1. Single event returns zero slopes
2. Rising coolant over 10 events produces positive slope
3. Three brake events count correctly
4. hydraulic_spike fires on > 500 kPa delta
5. transmission_heat fires on temp > 110
6. dtc_new isolates only new codes

All 6 tests must pass before committing.
```

### Agent 4 — Telematics Adapters
```
Write adapters/telematics/ with 4 adapters.

Base class: adapters/telematics/base/TelematicsAdapter.ts
Abstract methods: normalize(frame): SignalEvent, get source(): SignalSource, validate(frame): boolean

Implement:
1. OBD2Adapter: maps standard Mode 01 PIDs (0105=coolant, 010C=rpm, 010D=speed, 0111=throttle, 0104=load)
2. FordF450Adapter: extends OBD2 with Ford Mode 22 PIDs (22FF00=oil_pressure, 22FF01=trans_temp, 22FF03=def_level), handles FordPass JSON keys
3. CaterpillarAdapter: maps J1939 SPNs from frame.j1939_spns, handles VisionLink JSON format, decodes CAN PGNs 0xF004 and 0xFEEE
4. JohnDeere139Adapter: maps J1939 + JD proprietary SPNs 520204-520215, handles JDLink API "readings" array format

AdapterRegistry: createAdapter(type, config) factory function

Write vitest tests for each adapter:
- OBD2: raw PID 0105 value 125 maps to coolant_temp = 85
- Ford: FordPass JSON field 'engineCoolantTemp' = 95 maps correctly
- Cat: j1939_spns SPN 110 raw=4224 maps to coolant_temp ≈ 89°C
- JD: JDLink readings array format maps hydraulicOilPressure correctly

All tests pass before committing.
```

### Agent 5 — RuleEngine
```
Write packages/core/src/RuleEngine.ts.

Class RuleEngine:
- constructor(policyPack: PolicyPack)  
- evaluate(window: FeatureWindow): Decision[]

For each PolicyRule in the policy:
- Resolve the signal value from FeatureWindow (support dot notation: "signals_snapshot.coolant_temp")
- Apply the operator (gt, lt, gte, lte, eq, contains for dtc arrays)
- If threshold crossed, emit a Decision with correct severity, rule_id, confidence=1.0
- Return array of all triggered decisions (can be multiple)

Write vitest tests:
1. coolant_slope = 1.0 with threshold 0.8 and operator 'gt' fires CRITICAL
2. brake_spike_count = 2 with threshold 3 does not fire
3. brake_spike_count = 4 fires WARN
4. Nested signal (signals_snapshot.coolant_temp = 110) fires against threshold 108
5. Empty FeatureWindow returns empty decisions
6. Multiple rules can fire simultaneously

All 6 tests pass before committing.
```

### Agent 6 — TrustEngine
```
Write packages/core/src/TrustEngine.ts.

Class TrustEngine:
- evaluate(decision: Decision): Decision | null

Rules:
- If same rule_id fired within cooldown_ms, return null (suppress)
- If value does not exceed (threshold + hysteresis) on second fire, return null
- Otherwise pass through

Internal state: Map<string, { lastFiredTs: number, lastValue: number }>

Write vitest tests:
1. First fire always passes through
2. Second fire within cooldown is suppressed
3. After cooldown, same rule fires again
4. Hysteresis: value at threshold+1 fires, threshold alone suppresses repeat
5. Different asset_ids have independent cooldown state

All 5 tests pass before committing.
```

### Agent 7 — Scenarios (Real-World Data)
```
Create 5 scenario JSON files in scenarios/:

1. scenarios/overheat_highway.json
   - 60 frames over 60 seconds
   - coolant_temp rises from 88°C to 118°C (0.5°C/sec average slope)
   - engine_rpm steady 2200-2400
   - vehicle_speed steady 100 km/h
   - asset_id: "SIM-001", driver_id: "DRV-SIM"
   - Should trigger: coolant_high_temp (HIGH) then coolant_overheat_critical (CRITICAL)

2. scenarios/harsh_brake_city.json
   - 40 frames, vehicle_speed varies 0-60 km/h
   - brake_pedal spikes to 1.0 four times with rapid speed drops
   - Should trigger: harsh_brake_event (WARN)

3. scenarios/cold_start_normal.json  
   - 30 frames, coolant_temp rises from -5°C to 85°C normally (slow)
   - No thresholds crossed
   - Zero WARN+ events expected

4. scenarios/oscillating_fault.json
   - coolant_slope oscillates, triggers then drops below hysteresis, then re-triggers
   - Tests TrustEngine hysteresis logic

5. scenarios/heavy_equipment_hydraulic.json
   - asset_id: "CAT-320-001"
   - hydraulic_pressure spikes from 180 bar to 250 bar suddenly
   - transmission_temp rises to 118°C
   - def_level at 12%
   - Should trigger 3 different rules simultaneously

Write a brief comment at the top of each JSON explaining what it tests.
```

---

## Part 7: Pipeline Wiring

**File: `packages/core/src/pipeline.ts`**

Wires everything together for Person A's layer.

```typescript
import { EventBus } from '@anomedge/bus';
import { SignalEvent } from '@anomedge/contracts';
import { FeatureEngine } from './FeatureEngine';
import { RuleEngine }    from './RuleEngine';
import { TrustEngine }   from './TrustEngine';
import { loadPolicyForAsset } from '../../policy/loader';

export function startCorePipeline(bus: EventBus): void {
  const featureEngine = new FeatureEngine();
  const trustEngines  = new Map<string, TrustEngine>();
  const ruleEngines   = new Map<string, RuleEngine>();

  bus.subscribe<SignalEvent>('signals.raw', async (envelope) => {
    const signal = envelope.payload;

    // Get or create per-asset engines (loaded with correct policy for vehicle class)
    if (!ruleEngines.has(signal.asset_id)) {
      const policy = await loadPolicyForAsset(signal.asset_id);
      ruleEngines.set(signal.asset_id, new RuleEngine(policy));
      trustEngines.set(signal.asset_id, new TrustEngine(policy));
    }

    const featureWindow = featureEngine.ingest(signal);
    bus.publish('signals.features', featureWindow);

    const decisions = ruleEngines.get(signal.asset_id)!.evaluate(featureWindow);
    for (const decision of decisions) {
      bus.publish('decisions', decision);
    }

    const trusted = decisions
      .map(d => trustEngines.get(signal.asset_id)!.evaluate(d))
      .filter(Boolean);

    for (const gated of trusted) {
      bus.publish('decisions.gated', gated!);
    }
  });
}
```

---

## Part 8: Day-by-Day Execution Plan

| Day | You Build | Agent Used | Output |
|-----|-----------|------------|--------|
| **Day 1** | packages/contracts/src/index.ts | Agent 1 | Shared types. Commit, tag `contracts-v1`, FROZEN |
| **Day 1** | adapters/telematics (all 4 adapters) | Agent 4 | OBD2 + F450 + Cat + JD139 adapters |
| **Day 2** | packages/simulator | Agent 2 | SimulatorService with scenario loading |
| **Day 2** | scenarios/ (5 JSON files) | Agent 7 | Scenario test data |
| **Day 3** | packages/core/FeatureEngine | Agent 3 | 30-sec rolling window features |
| **Day 3** | packages/core/RuleEngine | Agent 5 | Policy threshold evaluation |
| **Day 4** | packages/core/TrustEngine | Agent 6 | Cooldown + hysteresis filtering |
| **Day 4** | packages/core/pipeline.ts | Manual | Wire bus → feature → rule → trust |
| **Day 5** | policy/policy.yaml (all vehicle classes) | Manual | Thresholds for F450 + Cat + JD |
| **Day 5** | `pnpm test` all packages | Verify | All tests green before Day 5 EOD |
| **Day 7** | Person C runs phase0.gate.ts | Gate test | 8/8 must pass for Phase 0 ✓ |

---

## Part 9: Key Rules (From the Charter)

1. **Never import another person's package internals.** Only import from `@anomedge/contracts` and `@anomedge/bus`.
2. **Contracts frozen after Day 1.** All 3 team members sign off before any change.
3. **One agent, one bounded task.** Never ask an agent to touch two packages simultaneously.
4. **Test first.** Every PR starts with failing tests. No exceptions.
5. **Adapter is the only real-world bridge.** Nothing in FeatureEngine, RuleEngine, or TrustEngine knows about Ford or Cat — they only see `SignalEvent`.
6. **Policy YAML drives thresholds.** Never hardcode a number in FeatureEngine or RuleEngine. 

---

*AnomEdge | Person A — Intelligence Engineer | Vancouver BC | March 2026*
