// lib/screens/alert_screen.dart
import 'dart:async';
import 'package:flutter/material.dart';
import 'package:uuid/uuid.dart';
import '../bus/event_bus.dart';
import '../contracts/contracts.dart' as ac;
import '../queue/local_queue.dart';
import '../tts/tts_service.dart';

const _uuid = Uuid();
int _ackSeq = 0;

class AlertScreen extends StatefulWidget {
  const AlertScreen({super.key});

  @override
  State<AlertScreen> createState() => _AlertScreenState();
}

class _AlertScreenState extends State<AlertScreen>
    with SingleTickerProviderStateMixin, AutomaticKeepAliveClientMixin {
  @override
  bool get wantKeepAlive => true;
  ac.Action? _latestAction;
  final List<ac.Action> _history = [];
  StreamSubscription<ac.Action>? _subscription;
  late AnimationController _animController;
  late Animation<double> _fadeAnimation;
  bool _isSpeaking = false;

  @override
  void initState() {
    super.initState();

    _animController = AnimationController(
        vsync: this, duration: const Duration(milliseconds: 400));
    _fadeAnimation =
        Tween<double>(begin: 0.0, end: 1.0).animate(_animController);

    // Listen for new actions from GuidanceEngine
    // Only promote to active alert if not already acknowledged
    _subscription = bus.actions.listen((action) {
      setState(() {
        _history.insert(0, action);
        if (_history.length > 50) _history.removeLast();
        if (!action.acknowledged) {
          _latestAction = action;
        }
      });
      if (!action.acknowledged) _animController.forward(from: 0.0);
    });

    // Listen to TTS speaking state for visual indicator
    ttsService.onSpeakingChanged = (speaking) {
      if (mounted) setState(() => _isSpeaking = speaking);
    };
  }

  @override
  void dispose() {
    _subscription?.cancel();
    ttsService.onSpeakingChanged = null;
    _animController.dispose();
    super.dispose();
  }

  Color _severityColor(ac.Severity s) {
    switch (s) {
      case ac.Severity.normal:   return const Color(0xFF2E7D32);
      case ac.Severity.watch:    return const Color(0xFF1565C0);
      case ac.Severity.warn:     return const Color(0xFFF57F17);
      case ac.Severity.high:     return const Color(0xFFE65100);
      case ac.Severity.critical: return const Color(0xFFB71C1C);
    }
  }

  IconData _severityIcon(ac.Severity s) {
    switch (s) {
      case ac.Severity.normal:   return Icons.check_circle_outline;
      case ac.Severity.watch:    return Icons.visibility_outlined;
      case ac.Severity.warn:     return Icons.warning_amber_outlined;
      case ac.Severity.high:     return Icons.error_outline;
      case ac.Severity.critical: return Icons.dangerous_outlined;
    }
  }

  void _acknowledge(ac.Action action) {
    // Clear the screen immediately
    setState(() => _latestAction = null);

    final acked = action.acknowledge();

    // Publish on the acknowledged topic (not actions — avoids re-triggering our listener)
    bus.publishAcknowledged(acked);

    // Write to LocalQueue for offline sync to Person C's dashboard
    final envelope = ac.EventEnvelope(
      id: _uuid.v4(),
      topic: 'actions.acknowledged',
      seq: ++_ackSeq,
      ts: DateTime.now().millisecondsSinceEpoch,
      payload: acked.toJson(),
      assetId: acked.assetId,
    );
    localQueue.write(envelope);
  }

  @override
  Widget build(BuildContext context) {
    super.build(context); // required by AutomaticKeepAliveClientMixin
    return Scaffold(
      backgroundColor: const Color(0xFF0D1117),
      appBar: AppBar(
        backgroundColor: const Color(0xFF161B22),
        title: const Text('AnomEdge',
            style: TextStyle(
                color: Colors.white,
                fontWeight: FontWeight.bold,
                letterSpacing: 1.5)),
        actions: [
          // TTS test button — tap to verify audio is working
          IconButton(
            icon: const Icon(Icons.volume_up, color: Colors.white38, size: 20),
            tooltip: 'Test TTS audio',
            onPressed: () => ttsService.replay('AnomEdge audio test. TTS is working.'),
          ),
          // Speaking indicator in app bar
          if (_isSpeaking)
            Padding(
              padding: const EdgeInsets.only(right: 8),
              child: Row(
                children: [
                  Icon(Icons.graphic_eq, color: Colors.orange[300], size: 20),
                  const SizedBox(width: 4),
                  Text('Speaking',
                      style: TextStyle(
                          color: Colors.orange[300], fontSize: 12)),
                ],
              ),
            ),
          IconButton(
            icon: const Icon(Icons.history, color: Colors.white70),
            onPressed: () => Navigator.pushNamed(context, '/history'),
          ),
          IconButton(
            icon: const Icon(Icons.settings, color: Colors.white70),
            onPressed: () => Navigator.pushNamed(context, '/settings'),
          ),
        ],
      ),
      body: Column(
        children: [
          Expanded(
            flex: 3,
            child: _latestAction == null
                ? _buildIdleState()
                : _buildActiveAlert(_latestAction!),
          ),
          if (_history.length > 1)
            Expanded(flex: 2, child: _buildRecentHistory()),
        ],
      ),
    );
  }

  Widget _buildIdleState() {
    return Center(
      child: Column(
        mainAxisAlignment: MainAxisAlignment.center,
        children: [
          Icon(Icons.shield_outlined, size: 80, color: Colors.green[700]),
          const SizedBox(height: 16),
          const Text('All Systems Normal',
              style: TextStyle(
                  color: Colors.white,
                  fontSize: 22,
                  fontWeight: FontWeight.w600)),
          const SizedBox(height: 8),
          const Text('Monitoring active',
              style: TextStyle(color: Colors.white38, fontSize: 14)),
        ],
      ),
    );
  }

  Widget _buildActiveAlert(ac.Action action) {
    final color = _severityColor(action.severity);
    return FadeTransition(
      opacity: _fadeAnimation,
      child: Container(
        margin: const EdgeInsets.all(16),
        decoration: BoxDecoration(
          color: const Color(0xFF161B22),
          borderRadius: BorderRadius.circular(16),
          border: Border.all(color: color, width: 2),
        ),
        child: Padding(
          padding: const EdgeInsets.all(24),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              // Severity badge + TTS controls
              Row(
                children: [
                  Container(
                    padding: const EdgeInsets.symmetric(
                        horizontal: 12, vertical: 6),
                    decoration: BoxDecoration(
                        color: color,
                        borderRadius: BorderRadius.circular(20)),
                    child: Row(
                      mainAxisSize: MainAxisSize.min,
                      children: [
                        Icon(_severityIcon(action.severity),
                            color: Colors.white, size: 18),
                        const SizedBox(width: 6),
                        Text(action.severity.name.toUpperCase(),
                            style: const TextStyle(
                                color: Colors.white,
                                fontWeight: FontWeight.bold,
                                fontSize: 13,
                                letterSpacing: 1.2)),
                      ],
                    ),
                  ),
                  const Spacer(),

                  // TTS speaking animation or replay button
                  if (action.speak)
                    _isSpeaking
                        ? Row(children: [
                            Icon(Icons.graphic_eq, color: color, size: 22),
                            const SizedBox(width: 4),
                            Text('Speaking',
                                style: TextStyle(
                                    color: color, fontSize: 11)),
                          ])
                        : IconButton(
                            icon: Icon(Icons.replay, color: color, size: 22),
                            tooltip: 'Replay alert',
                            onPressed: () =>
                                ttsService.replay(action.guidance),
                          ),

                  const SizedBox(width: 8),
                  Text(_timeAgo(action.ts),
                      style: const TextStyle(
                          color: Colors.white38, fontSize: 12)),
                ],
              ),

              const SizedBox(height: 16),

              // Rule group chip + rule ID
              Row(
                children: [
                  Container(
                    padding: const EdgeInsets.symmetric(
                        horizontal: 8, vertical: 3),
                    decoration: BoxDecoration(
                      color: color.withAlpha(30),
                      borderRadius: BorderRadius.circular(6),
                    ),
                    child: Text(
                      action.ruleId.split('_').first.toUpperCase(),
                      style: TextStyle(
                          color: color,
                          fontSize: 11,
                          fontWeight: FontWeight.w600,
                          letterSpacing: 1.0),
                    ),
                  ),
                  const SizedBox(width: 8),
                  Expanded(
                    child: Text(action.ruleId,
                        style: const TextStyle(
                            color: Colors.white38, fontSize: 11),
                        overflow: TextOverflow.ellipsis),
                  ),
                ],
              ),

              const SizedBox(height: 12),

              // Guidance text — large and readable at a glance
              Text(action.guidance,
                  style: const TextStyle(
                      color: Colors.white,
                      fontSize: 24,
                      fontWeight: FontWeight.w700,
                      height: 1.3)),

              const SizedBox(height: 8),
              Text('Asset: ${action.assetId}',
                  style: const TextStyle(
                      color: Colors.white38, fontSize: 12)),

              const Spacer(),

              // Acknowledge — publishes Action with acknowledged: true
              SizedBox(
                width: double.infinity,
                child: ElevatedButton(
                  style: ElevatedButton.styleFrom(
                    backgroundColor: color,
                    foregroundColor: Colors.white,
                    padding: const EdgeInsets.symmetric(vertical: 16),
                    shape: RoundedRectangleBorder(
                        borderRadius: BorderRadius.circular(12)),
                  ),
                  onPressed: () => _acknowledge(action),
                  child: const Text('ACKNOWLEDGE',
                      style: TextStyle(
                          fontSize: 16,
                          fontWeight: FontWeight.bold,
                          letterSpacing: 1.5)),
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }

  Widget _buildRecentHistory() {
    final recent = _history.skip(1).take(4).toList();
    return Container(
      color: const Color(0xFF0D1117),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          const Padding(
            padding: EdgeInsets.symmetric(horizontal: 16, vertical: 8),
            child: Text('RECENT',
                style: TextStyle(
                    color: Colors.white38,
                    fontSize: 11,
                    letterSpacing: 1.5)),
          ),
          Expanded(
            child: ListView.builder(
              itemCount: recent.length,
              itemBuilder: (_, i) {
                final a = recent[i];
                final color = _severityColor(a.severity);
                return ListTile(
                  dense: true,
                  leading: Icon(_severityIcon(a.severity),
                      color: color, size: 20),
                  title: Text(a.guidance,
                      style: const TextStyle(
                          color: Colors.white70, fontSize: 13),
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis),
                  subtitle: Text(a.ruleId,
                      style: const TextStyle(
                          color: Colors.white38, fontSize: 11)),
                  trailing: Text(_timeAgo(a.ts),
                      style: const TextStyle(
                          color: Colors.white38, fontSize: 11)),
                );
              },
            ),
          ),
        ],
      ),
    );
  }

  String _timeAgo(int tsMs) {
    final diff = DateTime.now()
        .difference(DateTime.fromMillisecondsSinceEpoch(tsMs));
    if (diff.inSeconds < 60) return '${diff.inSeconds}s ago';
    if (diff.inMinutes < 60) return '${diff.inMinutes}m ago';
    return '${diff.inHours}h ago';
  }
}
