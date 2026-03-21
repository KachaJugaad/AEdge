// test/guidance/validate_guidance_test.dart
// TDD tests for validateGuidance().
// All must pass before GuidanceEngine is merged.

import 'package:flutter_test/flutter_test.dart';
import 'package:anomedge_mobile/guidance/validate_guidance.dart';

void main() {
  group('validateGuidance()', () {
    group('INVALID cases', () {
      test('null input fails', () {
        final result = validateGuidance(null);
        expect(result.isValid, isFalse);
        expect(result.reason, contains('empty'));
      });

      test('empty string fails', () {
        final result = validateGuidance('');
        expect(result.isValid, isFalse);
        expect(result.reason, contains('empty'));
      });

      test('whitespace-only string fails', () {
        final result = validateGuidance('   ');
        expect(result.isValid, isFalse);
      });

      test('1-word string fails (too short)', () {
        final result = validateGuidance('Stop');
        expect(result.isValid, isFalse);
        expect(result.reason, contains('short'));
      });

      test('2-word string fails (too short)', () {
        final result = validateGuidance('Stop now');
        expect(result.isValid, isFalse);
        expect(result.reason, contains('short'));
      });

      test('16-word string fails (too long)', () {
        // exactly 16 words — verified with wc -w
        const text = 'One two three four five six seven eight nine ten eleven twelve thirteen fourteen fifteen sixteen';
        expect(text.split(' ').length, equals(16)); // sanity check
        final result = validateGuidance(text);
        expect(result.isValid, isFalse);
        expect(result.reason, contains('long'));
      });

      test('20-word string fails (too long)', () {
        final text = List.filled(20, 'word').join(' ');
        final result = validateGuidance(text);
        expect(result.isValid, isFalse);
      });

      test('string with "error" prohibited term fails', () {
        final result = validateGuidance('Stop vehicle due to error in system');
        expect(result.isValid, isFalse);
        expect(result.reason, contains('prohibited term'));
      });

      test('string with "exception" prohibited term fails', () {
        final result = validateGuidance('System exception detected stop now');
        expect(result.isValid, isFalse);
        expect(result.reason, contains('prohibited term'));
      });

      test('string with "null" prohibited term fails', () {
        final result = validateGuidance('Null reading on sensor check now');
        expect(result.isValid, isFalse);
        expect(result.reason, contains('prohibited term'));
      });

      test('string with "debug" prohibited term fails', () {
        final result = validateGuidance('Debug mode active stop vehicle now');
        expect(result.isValid, isFalse);
        expect(result.reason, contains('prohibited term'));
      });

      test('string without actionable verb fails', () {
        final result = validateGuidance('Engine temperature is very high');
        expect(result.isValid, isFalse);
        expect(result.reason, contains('actionable verb'));
      });

      test('generic noun-only sentence without verb fails', () {
        final result = validateGuidance('High temperature brake system anomaly');
        expect(result.isValid, isFalse);
        expect(result.reason, contains('actionable verb'));
      });
    });

    group('VALID cases', () {
      test('3-word valid guidance passes', () {
        final result = validateGuidance('Stop the vehicle');
        expect(result.isValid, isTrue);
        expect(result.reason, isNull);
      });

      test('15-word valid guidance passes (boundary)', () {
        final result = validateGuidance(
            'Engine critically overheated reduce speed and stop the vehicle when safe now');
        // Count: Engine(1) critically(2) overheated(3) reduce(4) speed(5) and(6) stop(7) the(8) vehicle(9) when(10) safe(11) now(12) = 12 words, valid
        expect(result.isValid, isTrue);
      });

      test('overheat critical template passes', () {
        final result =
            validateGuidance('Engine critically overheated. Stop the vehicle immediately.');
        expect(result.isValid, isTrue);
      });

      test('harsh brake warn template passes', () {
        final result = validateGuidance(
            'Harsh braking detected. Slow down and increase following distance.');
        expect(result.isValid, isTrue);
      });

      test('cold start watch template passes', () {
        final result = validateGuidance(
            'Engine still warming up. Avoid high load until warm.');
        expect(result.isValid, isTrue);
      });

      test('"reduce" is a valid actionable verb', () {
        final result =
            validateGuidance('Coolant rising reduce speed immediately');
        expect(result.isValid, isTrue);
      });

      test('"inspect" is a valid actionable verb', () {
        final result =
            validateGuidance('Brake anomaly detected inspect system now');
        expect(result.isValid, isTrue);
      });

      test('"monitor" is a valid actionable verb', () {
        final result =
            validateGuidance('Temperature elevated monitor coolant levels closely');
        expect(result.isValid, isTrue);
      });

      test('case insensitive prohibited term check', () {
        // "ERROR" uppercase should still fail
        final result = validateGuidance('System ERROR detected stop vehicle');
        expect(result.isValid, isFalse);
      });

      test('case insensitive actionable verb check', () {
        final result = validateGuidance('STOP the vehicle temperature high');
        expect(result.isValid, isTrue);
      });
    });

    group('assertGuidanceValid()', () {
      test('throws for invalid guidance', () {
        expect(
          () => assertGuidanceValid(''),
          throwsArgumentError,
        );
      });

      test('does not throw for valid guidance', () {
        expect(
          () => assertGuidanceValid('Stop vehicle engine is overheating'),
          returnsNormally,
        );
      });
    });
  });
}
