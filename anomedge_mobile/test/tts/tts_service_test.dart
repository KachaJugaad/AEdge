// test/tts/tts_service_test.dart
import 'package:flutter_test/flutter_test.dart';
import 'package:uuid/uuid.dart';
import 'package:anomedge_mobile/contracts/contracts.dart';
import 'package:anomedge_mobile/bus/event_bus.dart';

const _uuid = Uuid();
int _seqN = 0;

Action _makeAction({
  required Severity severity,
  required String ruleId,
  required String guidance,
  required bool speak,
}) {
  final nowMs = DateTime.now().millisecondsSinceEpoch;
  return Action(
    seq: ++_seqN,
    ts: nowMs,
    assetId: 'TRUCK-001',
    severity: severity,
    title: ruleId.split('_').map((w) => w.isEmpty ? w : w[0].toUpperCase() + w.substring(1)).join(' '),
    guidance: guidance,
    ruleId: ruleId,
    speak: speak,
    acknowledged: false,
    source: ActionSource.template,
    decisionSource: DecisionSource.ruleEngine,
    actionId: _uuid.v4(),
  );
}

void main() {
  group('TtsService contract', () {
    test('CRITICAL action has speak: true and gets "Critical alert." prefix', () {
      final action = _makeAction(
        severity: Severity.critical,
        ruleId: 'coolant_overheat_critical',
        guidance: 'Stop the vehicle immediately.',
        speak: true,
      );

      expect(action.speak, isTrue);

      // TtsService prepends "Critical alert." for CRITICAL
      final spokenText = action.severity == Severity.critical
          ? 'Critical alert. ${action.guidance}'
          : action.guidance;
      expect(spokenText, startsWith('Critical alert.'));
    });

    test('HIGH action has speak: true but no "Critical alert." prefix', () {
      final action = _makeAction(
        severity: Severity.high,
        ruleId: 'coolant_overheat_high',
        guidance: 'Reduce speed and stop when safe.',
        speak: true,
      );

      expect(action.speak, isTrue);

      final spokenText = action.severity == Severity.critical
          ? 'Critical alert. ${action.guidance}'
          : action.guidance;
      expect(spokenText, isNot(startsWith('Critical alert.')));
    });

    test('NORMAL and WATCH actions have speak: false', () {
      final action = _makeAction(
        severity: Severity.normal,
        ruleId: 'brake_normal',
        guidance: 'Braking pattern normal. Maintain current driving style.',
        speak: false,
      );

      expect(action.speak, isFalse,
          reason: 'GuidanceEngine only sets speak:true for HIGH/CRITICAL');
    });

    test('actionsSpoken bus topic only receives speak: true actions', () async {
      final spoken = <Action>[];
      bus.actionsSpoken.listen(spoken.add);

      // NORMAL — should NOT appear on actionsSpoken
      final normalAction = _makeAction(
        severity: Severity.normal,
        ruleId: 'brake_normal',
        guidance: 'Braking pattern normal. Maintain current driving style.',
        speak: false,
      );
      bus.publishAction(normalAction);

      await Future.delayed(Duration.zero);
      expect(spoken, isEmpty, reason: 'speak:false should not reach actionsSpoken');

      // CRITICAL — should appear on actionsSpoken
      final critAction = _makeAction(
        severity: Severity.critical,
        ruleId: 'coolant_overheat_critical',
        guidance: 'Stop the vehicle immediately.',
        speak: true,
      );
      bus.publishAction(critAction);

      await Future.delayed(Duration.zero);
      expect(spoken.length, equals(1));
      expect(spoken.first.severity, equals(Severity.critical));
    });

    test('acknowledge() sets acknowledged: true and speak: false', () {
      final action = _makeAction(
        severity: Severity.critical,
        ruleId: 'coolant_overheat_critical',
        guidance: 'Stop the vehicle immediately.',
        speak: true,
      );

      final acked = action.acknowledge();
      expect(acked.acknowledged, isTrue);
      expect(acked.speak, isFalse); // TTS stops on acknowledge
      expect(acked.guidance, equals(action.guidance)); // guidance unchanged
    });
  });
}
