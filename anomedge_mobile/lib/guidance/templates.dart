// lib/guidance/templates.dart
// DEFAULT_TEMPLATES — mandatory fallback when LLM unavailable or too slow (>400ms).
// Keyed by "$ruleGroup.$severity" — matches Person A's rule_group + severity fields.
// Every string must pass validateGuidance(): 3–15 words, actionable verb, no banned terms.

import '../contracts/contracts.dart';
import 'validate_guidance.dart';

const Map<String, String> DEFAULT_TEMPLATES = {
  // --- THERMAL (coolant, engine temp) ---
  'thermal.normal':   'Engine temperature normal. Monitor coolant and proceed.',
  'thermal.watch':    'Coolant temperature rising. Monitor closely and proceed carefully.',
  'thermal.warn':     'Engine running hot. Reduce load and check coolant level.',
  'thermal.high':     'High engine temperature. Reduce speed and stop when safe.',
  'thermal.critical': 'Engine critically overheated. Stop the vehicle immediately.',

  // --- BRAKING ---
  'braking.normal':   'Braking pattern normal. Maintain current driving style.',
  'braking.watch':    'Brake pressure elevated. Apply brakes more gradually.',
  'braking.warn':     'Harsh braking detected. Slow down and increase following distance.',
  'braking.high':     'Repeated harsh braking. Reduce speed and inspect brake system.',
  'braking.critical': 'Brake anomaly detected. Stop and inspect brakes immediately.',

  // --- SPEED ---
  'speed.normal':   'Vehicle speed normal. Proceed and maintain safe speed.',
  'speed.watch':    'Speed approaching limit. Monitor and reduce if needed.',
  'speed.warn':     'Excess speed detected. Reduce speed to safe limit now.',
  'speed.high':     'Dangerous speed detected. Reduce speed immediately.',
  'speed.critical': 'Critical speed exceeded. Stop accelerating and slow down now.',

  // --- TRANSMISSION ---
  'transmission.normal':   'Transmission operating normally. Monitor and proceed.',
  'transmission.watch':    'Transmission temperature slightly elevated. Monitor closely.',
  'transmission.warn':     'Transmission warning detected. Reduce load and check fluid.',
  'transmission.high':     'Transmission overheating. Stop when safe and inspect.',
  'transmission.critical': 'Transmission failure risk. Stop vehicle and call maintenance.',

  // --- ELECTRICAL ---
  'electrical.normal':   'Electrical systems normal. Monitor and proceed.',
  'electrical.watch':    'Minor electrical anomaly detected. Monitor system status.',
  'electrical.warn':     'Electrical fault detected. Reduce load and inspect system.',
  'electrical.high':     'Electrical system fault. Stop when safe and inspect.',
  'electrical.critical': 'Critical electrical failure. Stop vehicle immediately.',

  // --- DTC (Diagnostic Trouble Codes) ---
  'dtc.normal':   'No active trouble codes. Proceed and monitor systems.',
  'dtc.watch':    'Pending trouble code detected. Monitor and inspect soon.',
  'dtc.warn':     'Active trouble code present. Reduce load and inspect vehicle.',
  'dtc.high':     'Multiple trouble codes active. Stop when safe and inspect.',
  'dtc.critical': 'Critical trouble code active. Stop and call maintenance now.',

  // --- FUEL ---
  'fuel.normal':   'Fuel system normal. Monitor and proceed with trip.',
  'fuel.watch':    'Fuel efficiency dropping. Monitor consumption and check system.',
  'fuel.warn':     'Fuel system anomaly detected. Reduce load and inspect.',
  'fuel.high':     'Fuel system fault detected. Stop when safe and inspect.',
  'fuel.critical': 'Critical fuel system fault. Stop vehicle immediately.',

  // --- UNKNOWN (fallback for unrecognised rule_group) ---
  'unknown.normal':   'System status normal. Monitor and proceed with caution.',
  'unknown.watch':    'Minor anomaly detected. Monitor system and proceed carefully.',
  'unknown.warn':     'System anomaly detected. Reduce load and inspect vehicle.',
  'unknown.high':     'System fault detected. Stop when safe and inspect.',
  'unknown.critical': 'Critical system fault. Stop vehicle and call maintenance.',
};

/// Get guidance text for a given rule_group + severity combination.
String getTemplate(RuleGroup ruleGroup, Severity severity) {
  final key = '${ruleGroup.name}.${severity.name}';
  return DEFAULT_TEMPLATES[key] ??
      'Anomaly detected. Reduce speed and proceed with caution.';
}

/// Validate ALL templates at startup — gate tests also assert this.
void assertAllTemplatesValid() {
  for (final entry in DEFAULT_TEMPLATES.entries) {
    final result = validateGuidance(entry.value);
    if (!result.isValid) {
      throw StateError(
          'Template "${entry.key}" failed validation: ${result.reason}\n'
          'Text: "${entry.value}"');
    }
  }
}
