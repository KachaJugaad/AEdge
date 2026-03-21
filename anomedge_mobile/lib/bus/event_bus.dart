// lib/bus/event_bus.dart
// Dart EventBus — communication backbone.
// All layers talk only through this. No cross-package imports.

import 'dart:async';
import '../contracts/contracts.dart';

class BusMetrics {
  int publishCount = 0;
  final List<int> latenciesMs = [];

  double get p50 => _percentile(0.50);
  double get p95 => _percentile(0.95);
  double get p99 => _percentile(0.99);

  double _percentile(double p) {
    if (latenciesMs.isEmpty) return 0;
    final sorted = List<int>.from(latenciesMs)..sort();
    final index = (p * sorted.length).ceil() - 1;
    return sorted[index.clamp(0, sorted.length - 1)].toDouble();
  }

  void recordLatency(int ms) => latenciesMs.add(ms);

  Map<String, dynamic> toJson() => {
        'publish_count': publishCount,
        'p50_ms': p50,
        'p95_ms': p95,
        'p99_ms': p99,
      };
}

class AnomEdgeBus {
  static final AnomEdgeBus _instance = AnomEdgeBus._internal();
  factory AnomEdgeBus() => _instance;
  AnomEdgeBus._internal();

  // Topic: decisions.gated (post-trust-gate — GuidanceEngine subscribes here)
  final _decisionsGated = StreamController<Decision>.broadcast();
  Stream<Decision> get decisionsGated => _decisionsGated.stream;

  // Topic: actions (guidance text — AlertScreen subscribes here)
  final _actions = StreamController<Action>.broadcast();
  Stream<Action> get actions => _actions.stream;

  // Topic: actions.acknowledged (operator acked — Person C's dashboard subscribes here)
  final _actionsAcknowledged = StreamController<Action>.broadcast();
  Stream<Action> get actionsAcknowledged => _actionsAcknowledged.stream;

  // Topic: actions.spoken (HIGH + CRITICAL only — TTS subscribes here)
  final _actionsSpoken = StreamController<Action>.broadcast();
  Stream<Action> get actionsSpoken => _actionsSpoken.stream;

  final Map<String, BusMetrics> _metrics = {
    'decisions.gated': BusMetrics(),
    'actions': BusMetrics(),
    'actions.spoken': BusMetrics(),
  };

  void publishDecisionGated(Decision decision) {
    final start = DateTime.now().millisecondsSinceEpoch;
    _decisionsGated.add(decision);
    _record('decisions.gated', start);
  }

  void publishAcknowledged(Action action) {
    _actionsAcknowledged.add(action);
  }

  void publishAction(Action action) {
    final start = DateTime.now().millisecondsSinceEpoch;
    _actions.add(action);
    _record('actions', start);

    if (action.speak) {
      _actionsSpoken.add(action);
      _record('actions.spoken', start);
    }
  }

  void _record(String topic, int startMs) {
    final latency = DateTime.now().millisecondsSinceEpoch - startMs;
    _metrics[topic]?.publishCount++;
    _metrics[topic]?.recordLatency(latency);
  }

  Map<String, dynamic> getMetrics() =>
      _metrics.map((k, v) => MapEntry(k, v.toJson()));

  void reset() {
    _metrics.forEach((_, m) {
      m.publishCount = 0;
      m.latenciesMs.clear();
    });
  }

  void dispose() {
    _decisionsGated.close();
    _actions.close();
    _actionsSpoken.close();
    _actionsAcknowledged.close();
  }
}

final bus = AnomEdgeBus();
