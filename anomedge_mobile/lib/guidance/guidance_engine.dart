// lib/guidance/guidance_engine.dart
// Subscribes to decisions.gated → generates guidance → publishes Action to bus.actions.
// Phase 0: template mode. Phase 2: live LLM with 400ms fallback.

import 'dart:async';
import 'package:uuid/uuid.dart';
import '../contracts/contracts.dart';
import '../bus/event_bus.dart';
import 'validate_guidance.dart';
import 'templates.dart';

const _uuid = Uuid();
int _seqCounter = 0;

class GuidanceEngine {
  StreamSubscription<Decision>? _subscription;
  bool _running = false;

  Future<String?> Function(Decision)? liveGenerator; // Phase 2 hook

  void start() {
    if (_running) return;
    _running = true;
    assertAllTemplatesValid();
    _subscription = bus.decisionsGated.listen(_handleDecision);
  }

  void stop() {
    _running = false;
    _subscription?.cancel();
    _subscription = null;
  }

  Future<void> _handleDecision(Decision decision) async {
    final guidanceText = await _generateGuidance(decision);
    final validated = validateGuidance(guidanceText);
    final finalText = validated.isValid
        ? guidanceText
        : getTemplate(decision.ruleGroup, decision.severity);

    final action = Action(
      seq: ++_seqCounter,
      ts: DateTime.now().millisecondsSinceEpoch,
      assetId: decision.assetId,
      severity: decision.severity,
      title: _buildTitle(decision.ruleGroup, decision.ruleId),
      guidance: finalText,
      ruleId: decision.ruleId,
      speak: decision.severity == Severity.high ||
          decision.severity == Severity.critical,
      acknowledged: false,
      source: liveGenerator != null ? ActionSource.llm : ActionSource.template,
      decisionSource: decision.decisionSource,
      actionId: _uuid.v4(),
    );

    bus.publishAction(action);
  }

  Future<String> _generateGuidance(Decision decision) async {
    if (liveGenerator != null) {
      try {
        final result = await liveGenerator!(decision)
            .timeout(const Duration(milliseconds: 400));
        if (result != null && result.isNotEmpty) return result;
      } catch (_) {
        // Timeout or LLM error — fall through to template
      }
    }
    return getTemplate(decision.ruleGroup, decision.severity);
  }

  /// Builds a short human-readable title from rule group + rule id.
  /// e.g. ruleGroup=thermal, ruleId="coolant_overheat_critical"
  ///   → "Coolant Overheat Critical"
  String _buildTitle(RuleGroup group, String ruleId) {
    final parts = ruleId.split('_').map((w) {
      if (w.isEmpty) return w;
      return w[0].toUpperCase() + w.substring(1);
    });
    return parts.join(' ');
  }
}

final guidanceEngine = GuidanceEngine();
