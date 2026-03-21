// lib/guidance/validate_guidance.dart
// Output validator — every guidance string passes this gate before reaching the operator.
// 3–15 words, actionable verb, no prohibited terms.

class GuidanceValidationResult {
  final bool isValid;
  final String? reason;
  const GuidanceValidationResult.valid() : isValid = true, reason = null;
  const GuidanceValidationResult.invalid(this.reason) : isValid = false;
}

// Terms that must never appear in operator guidance
const _prohibitedTerms = [
  'error', 'exception', 'null', 'undefined', 'nan', 'infinity',
  'fault code', 'stack trace', 'debug', 'warning:', 'critical:',
];

// Actionable verbs — at least one must appear in the guidance string
const _actionableVerbs = [
  'stop', 'reduce', 'check', 'inspect', 'pull', 'slow', 'monitor',
  'avoid', 'call', 'alert', 'engage', 'disengage', 'restart',
  'contact', 'notify', 'proceed', 'wait', 'maintain', 'apply',
  'release', 'turn', 'move', 'park', 'idle', 'cool', 'shut',
];

GuidanceValidationResult validateGuidance(String? text) {
  if (text == null || text.trim().isEmpty) {
    return const GuidanceValidationResult.invalid('Guidance text is empty');
  }

  final trimmed = text.trim();
  final words = trimmed.split(RegExp(r'\s+')).where((w) => w.isNotEmpty).toList();

  if (words.length < 3) {
    return GuidanceValidationResult.invalid(
        'Too short: ${words.length} words (minimum 3)');
  }

  if (words.length > 15) {
    return GuidanceValidationResult.invalid(
        'Too long: ${words.length} words (maximum 15)');
  }

  final lowerText = trimmed.toLowerCase();

  // Use whole-word matching for prohibited terms to avoid false positives
  // e.g. "nan" inside "maintenance", "null" inside "nullify"
  for (final term in _prohibitedTerms) {
    final pattern = RegExp(r'\b' + RegExp.escape(term) + r'\b', caseSensitive: false);
    if (pattern.hasMatch(lowerText)) {
      return GuidanceValidationResult.invalid(
          'Contains prohibited term: "$term"');
    }
  }

  final hasActionableVerb = _actionableVerbs.any((verb) {
    // Prefix match so "monitor" matches "monitoring", "check" matches "checking", etc.
    final pattern = RegExp(r'\b' + verb, caseSensitive: false);
    return pattern.hasMatch(lowerText);
  });

  if (!hasActionableVerb) {
    return GuidanceValidationResult.invalid(
        'No actionable verb found. Must contain one of: ${_actionableVerbs.take(8).join(', ')}...');
  }

  return const GuidanceValidationResult.valid();
}

/// Throws if invalid — use in tests
void assertGuidanceValid(String text) {
  final result = validateGuidance(text);
  if (!result.isValid) {
    throw ArgumentError('Invalid guidance: ${result.reason}');
  }
}
