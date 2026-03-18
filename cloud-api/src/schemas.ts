/**
 * Zod validation schemas for cloud-api.
 * Source of truth: packages/contracts/src/index.ts — FROZEN contract.
 *
 * Canadian data sovereignty: all validated data remains in CA infrastructure.
 */

import { z } from 'zod';

// ── SignalMap ──────────────────────────────────────────────────────────────────
// All known OBD-II and heavy-fleet fields are optional numbers.
// Unknown additional fields are accepted as number | string (open telemetry).

export const SignalMapSchema = z.object({
  // OBD-II common
  coolant_temp:       z.number().optional(),
  engine_rpm:         z.number().optional(),
  vehicle_speed:      z.number().optional(),
  throttle_position:  z.number().optional(),
  engine_load:        z.number().optional(),
  fuel_level:         z.number().optional(),
  intake_air_temp:    z.number().optional(),
  battery_voltage:    z.number().optional(),
  brake_pedal:        z.number().optional(),
  oil_pressure:       z.number().optional(),
  dtc_codes:          z.array(z.string()).optional(),
  // Heavy fleet extensions
  hydraulic_pressure: z.number().optional(),
  transmission_temp:  z.number().optional(),
  axle_weight:        z.number().optional(),
  pto_rpm:            z.number().optional(),
  boom_position:      z.number().optional(),
  load_weight:        z.number().optional(),
  def_level:          z.number().optional(),
  adblue_level:       z.number().optional(),
  boost_pressure:     z.number().optional(),
  exhaust_temp:       z.number().optional(),
}).catchall(z.union([z.number(), z.string()]));

export type SignalMap = z.infer<typeof SignalMapSchema>;

// ── Bus topics — matches BusTopic union in contracts ─────────────────────────

const BUS_TOPICS = [
  'signals.raw',
  'signals.features',
  'decisions',
  'decisions.gated',
  'actions',
  'telemetry.sync',
  'model.ota',
  'system.heartbeat',
  'system.error',
] as const;

// ── EventEnvelope ─────────────────────────────────────────────────────────────

export const EventEnvelopeSchema = z.object({
  id:      z.string().uuid(),
  topic:   z.enum(BUS_TOPICS),
  seq:     z.number().int().positive(),
  ts:      z.number().int(),
  payload: z.unknown(),
});

export type EventEnvelopeInput = z.infer<typeof EventEnvelopeSchema>;

// ── SyncBatch ─────────────────────────────────────────────────────────────────

const MAX_BATCH_SIZE = 500;

export const SyncBatchSchema = z.array(EventEnvelopeSchema).max(MAX_BATCH_SIZE);

export type SyncBatch = z.infer<typeof SyncBatchSchema>;

// ── Query params for GET /api/v1/events ──────────────────────────────────────

const DEFAULT_EVENTS_LIMIT = 100;

export const EventsQuerySchema = z.object({
  topic:    z.enum(BUS_TOPICS).optional(),
  asset_id: z.string().optional(),
  limit:    z.coerce.number().int().positive().default(DEFAULT_EVENTS_LIMIT),
});

export type EventsQuery = z.infer<typeof EventsQuerySchema>;
