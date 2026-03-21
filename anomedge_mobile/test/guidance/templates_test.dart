// test/guidance/templates_test.dart
import 'package:flutter_test/flutter_test.dart';
import 'package:anomedge_mobile/contracts/contracts.dart';
import 'package:anomedge_mobile/guidance/templates.dart';
import 'package:anomedge_mobile/guidance/validate_guidance.dart';

void main() {
  group('DEFAULT_TEMPLATES', () {
    test('all templates pass validateGuidance()', () {
      for (final entry in DEFAULT_TEMPLATES.entries) {
        final result = validateGuidance(entry.value);
        expect(result.isValid, isTrue,
            reason: 'Template "${entry.key}" failed: ${result.reason}\nText: "${entry.value}"');
      }
    });

    test('every RuleGroup × Severity combination has a template', () {
      for (final group in RuleGroup.values) {
        for (final severity in Severity.values) {
          final key = '${group.name}.${severity.name}';
          expect(DEFAULT_TEMPLATES.containsKey(key), isTrue,
              reason: 'Missing template for "$key"');
        }
      }
    });

    test('getTemplate returns non-empty string for all combinations', () {
      for (final group in RuleGroup.values) {
        for (final severity in Severity.values) {
          final text = getTemplate(group, severity);
          expect(text.isNotEmpty, isTrue,
              reason: 'Empty text for ${group.name}.${severity.name}');
        }
      }
    });

    test('getTemplate output always passes validation', () {
      for (final group in RuleGroup.values) {
        for (final severity in Severity.values) {
          final text = getTemplate(group, severity);
          final result = validateGuidance(text);
          expect(result.isValid, isTrue,
              reason: 'getTemplate(${group.name}, ${severity.name}) invalid: ${result.reason}');
        }
      }
    });

    group('spot checks', () {
      test('thermal.critical is urgent stop instruction', () {
        final text = getTemplate(RuleGroup.thermal, Severity.critical);
        expect(text.toLowerCase(), contains('stop'));
      });

      test('braking.warn includes slow or reduce', () {
        final text = getTemplate(RuleGroup.braking, Severity.warn);
        final lower = text.toLowerCase();
        expect(lower.contains('slow') || lower.contains('reduce'), isTrue);
      });

      test('dtc.normal does not sound alarming', () {
        final text = getTemplate(RuleGroup.dtc, Severity.normal);
        final lower = text.toLowerCase();
        expect(lower.contains('critical') || lower.contains('stop'), isFalse);
      });

      test('fuel.normal is reassuring', () {
        final text = getTemplate(RuleGroup.fuel, Severity.normal);
        expect(text.toLowerCase().contains('normal') || text.toLowerCase().contains('proceed'), isTrue);
      });
    });

    test('assertAllTemplatesValid() does not throw', () {
      expect(() => assertAllTemplatesValid(), returnsNormally);
    });
  });
}
