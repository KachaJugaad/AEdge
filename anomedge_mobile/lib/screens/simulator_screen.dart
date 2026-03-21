// lib/screens/simulator_screen.dart
// Replays demo scenarios using Person A's exact Decision field structure.
// FIX: Timer moved to a singleton ScenarioRunner so it survives tab switches.

import 'dart:async';
import 'package:flutter/material.dart';
import '../bus/event_bus.dart';
import '../contracts/contracts.dart';
import '../guidance/guidance_engine.dart';

// ─── Demo scenarios ───────────────────────────────────────────────────────────
final _demoScenarios = {
  'overheat_highway': [
    {'severity': 'normal',   'rule_id': 'coolant_temp_watch',        'rule_group': 'thermal',       'raw_value': 88.0,  'threshold': 95.0,  'confidence': 1.0},
    {'severity': 'watch',    'rule_id': 'coolant_temp_watch',        'rule_group': 'thermal',       'raw_value': 97.0,  'threshold': 95.0,  'confidence': 1.0},
    {'severity': 'warn',     'rule_id': 'coolant_overheat_warn',     'rule_group': 'thermal',       'raw_value': 105.0, 'threshold': 100.0, 'confidence': 1.0},
    {'severity': 'high',     'rule_id': 'coolant_overheat_high',     'rule_group': 'thermal',       'raw_value': 115.0, 'threshold': 110.0, 'confidence': 1.0},
    {'severity': 'critical', 'rule_id': 'coolant_overheat_critical', 'rule_group': 'thermal',       'raw_value': 128.0, 'threshold': 120.0, 'confidence': 1.0},
  ],
  'harsh_brake_city': [
    {'severity': 'normal',   'rule_id': 'brake_normal',       'rule_group': 'braking', 'raw_value': 2.1,  'threshold': 7.0,  'confidence': 1.0},
    {'severity': 'warn',     'rule_id': 'harsh_brake_event',  'rule_group': 'braking', 'raw_value': 9.2,  'threshold': 7.0,  'confidence': 1.0},
    {'severity': 'high',     'rule_id': 'harsh_brake_event',  'rule_group': 'braking', 'raw_value': 13.5, 'threshold': 7.0,  'confidence': 1.0},
    {'severity': 'critical', 'rule_id': 'brake_failure_risk', 'rule_group': 'braking', 'raw_value': 18.0, 'threshold': 15.0, 'confidence': 1.0},
  ],
  'transmission_fault': [
    {'severity': 'watch',    'rule_id': 'trans_temp_watch',        'rule_group': 'transmission', 'raw_value': 92.0,  'threshold': 90.0,  'confidence': 1.0},
    {'severity': 'warn',     'rule_id': 'trans_temp_warn',         'rule_group': 'transmission', 'raw_value': 105.0, 'threshold': 100.0, 'confidence': 1.0},
    {'severity': 'critical', 'rule_id': 'trans_overheat_critical', 'rule_group': 'transmission', 'raw_value': 130.0, 'threshold': 120.0, 'confidence': 1.0},
  ],
  'dtc_codes': [
    {'severity': 'watch',    'rule_id': 'dtc_pending',  'rule_group': 'dtc', 'raw_value': 1.0, 'threshold': 1.0, 'confidence': 1.0},
    {'severity': 'high',     'rule_id': 'dtc_active',   'rule_group': 'dtc', 'raw_value': 3.0, 'threshold': 2.0, 'confidence': 1.0},
    {'severity': 'critical', 'rule_id': 'dtc_critical', 'rule_group': 'dtc', 'raw_value': 5.0, 'threshold': 3.0, 'confidence': 1.0},
  ],
};

// ─── ScenarioRunner singleton ─────────────────────────────────────────────────
// Lives outside widget state so it survives tab switches.
class _ScenarioRunner {
  static final _ScenarioRunner instance = _ScenarioRunner._();
  _ScenarioRunner._();

  Timer? _timer;
  bool running = false;
  String scenario = 'overheat_highway';
  int frameIndex = 0;
  final List<String> log = [];

  // UI widgets subscribe to this to get log/state updates
  final _onChange = StreamController<void>.broadcast();
  Stream<void> get onChange => _onChange.stream;

  void start() {
    final frames = _demoScenarios[scenario] ?? [];
    if (frames.isEmpty || running) return;

    running = true;
    frameIndex = 0;
    log.clear();
    guidanceEngine.start();
    _notify();

    _timer = Timer.periodic(const Duration(seconds: 2), (timer) {
      if (frameIndex >= frames.length) {
        timer.cancel();
        running = false;
        _notify();
        return;
      }

      final frame = frames[frameIndex];
      final severityStr = frame['severity'] as String;
      final ruleGroupStr = frame['rule_group'] as String;

      final decision = Decision(
        ts: DateTime.now().millisecondsSinceEpoch,
        assetId: 'DEMO-ASSET-F450',
        severity: Decision.parseSeverity(severityStr),
        ruleId: frame['rule_id'] as String,
        ruleGroup: Decision.parseRuleGroup(ruleGroupStr),
        confidence: (frame['confidence'] as num).toDouble(),
        triggeredBy: [ruleGroupStr],
        rawValue: (frame['raw_value'] as num).toDouble(),
        threshold: (frame['threshold'] as num).toDouble(),
        decisionSource: DecisionSource.ruleEngine,
      );

      bus.publishDecisionGated(decision);

      log.add(
        '[${frameIndex + 1}/${frames.length}] '
        '${severityStr.toUpperCase()} · ${frame['rule_id']} · '
        'value=${frame['raw_value']} threshold=${frame['threshold']}',
      );
      frameIndex++;
      _notify();
    });
  }

  void stop() {
    _timer?.cancel();
    running = false;
    _notify();
  }

  void _notify() => _onChange.add(null);
}

final _runner = _ScenarioRunner.instance;

// ─── SimulatorScreen ──────────────────────────────────────────────────────────
class SimulatorScreen extends StatefulWidget {
  const SimulatorScreen({super.key});

  @override
  State<SimulatorScreen> createState() => _SimulatorScreenState();
}

// AutomaticKeepAliveClientMixin keeps the widget alive when tabs switch
class _SimulatorScreenState extends State<SimulatorScreen>
    with AutomaticKeepAliveClientMixin {
  @override
  bool get wantKeepAlive => true;

  StreamSubscription<void>? _sub;

  @override
  void initState() {
    super.initState();
    // Rebuild UI whenever the runner emits a change
    _sub = _runner.onChange.listen((_) {
      if (mounted) setState(() {});
    });
  }

  @override
  void dispose() {
    _sub?.cancel();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    super.build(context); // required by AutomaticKeepAliveClientMixin
    return Scaffold(
      backgroundColor: const Color(0xFF0D1117),
      appBar: AppBar(
        backgroundColor: const Color(0xFF161B22),
        title: const Text('Simulator',
            style: TextStyle(color: Colors.white)),
        iconTheme: const IconThemeData(color: Colors.white),
      ),
      body: Padding(
        padding: const EdgeInsets.all(16),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            const Text('SCENARIO',
                style: TextStyle(
                    color: Colors.white38, fontSize: 11, letterSpacing: 1.5)),
            const SizedBox(height: 8),
            Container(
              padding: const EdgeInsets.symmetric(horizontal: 12),
              decoration: BoxDecoration(
                color: const Color(0xFF161B22),
                borderRadius: BorderRadius.circular(8),
                border: Border.all(color: Colors.white12),
              ),
              child: DropdownButton<String>(
                value: _runner.scenario,
                isExpanded: true,
                dropdownColor: const Color(0xFF161B22),
                underline: const SizedBox(),
                items: _demoScenarios.keys
                    .map((k) => DropdownMenuItem(
                        value: k,
                        child: Text(k,
                            style: const TextStyle(color: Colors.white70))))
                    .toList(),
                onChanged: _runner.running
                    ? null
                    : (v) => setState(() => _runner.scenario = v!),
              ),
            ),
            const SizedBox(height: 16),
            SizedBox(
              width: double.infinity,
              child: ElevatedButton.icon(
                style: ElevatedButton.styleFrom(
                  backgroundColor: _runner.running
                      ? Colors.red[900]
                      : const Color(0xFF1B6CA8),
                  padding: const EdgeInsets.symmetric(vertical: 14),
                ),
                icon: Icon(_runner.running ? Icons.stop : Icons.play_arrow),
                label:
                    Text(_runner.running ? 'STOP' : 'RUN SCENARIO'),
                onPressed:
                    _runner.running ? _runner.stop : _runner.start,
              ),
            ),
            const SizedBox(height: 24),
            const Text('SIGNAL LOG',
                style: TextStyle(
                    color: Colors.white38, fontSize: 11, letterSpacing: 1.5)),
            const SizedBox(height: 8),
            Expanded(
              child: Container(
                padding: const EdgeInsets.all(12),
                decoration: BoxDecoration(
                  color: const Color(0xFF0A0F14),
                  borderRadius: BorderRadius.circular(8),
                  border: Border.all(color: Colors.white12),
                ),
                child: _runner.log.isEmpty
                    ? const Text('Run a scenario to see signal output.',
                        style: TextStyle(color: Colors.white38))
                    : ListView.builder(
                        itemCount: _runner.log.length,
                        reverse: true,
                        itemBuilder: (_, i) {
                          final entry =
                              _runner.log[_runner.log.length - 1 - i];
                          Color color = Colors.green;
                          if (entry.contains('CRITICAL')) {
                            color = const Color(0xFFB71C1C);
                          } else if (entry.contains('HIGH')) {
                            color = const Color(0xFFE65100);
                          } else if (entry.contains('WARN')) {
                            color = const Color(0xFFF57F17);
                          } else if (entry.contains('WATCH')) {
                            color = const Color(0xFF1565C0);
                          }
                          return Padding(
                            padding: const EdgeInsets.symmetric(vertical: 2),
                            child: Text(entry,
                                style: TextStyle(
                                    color: color,
                                    fontSize: 12,
                                    fontFamily: 'monospace')),
                          );
                        },
                      ),
              ),
            ),
          ],
        ),
      ),
    );
  }
}
