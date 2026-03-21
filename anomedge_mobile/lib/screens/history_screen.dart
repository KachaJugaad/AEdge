// lib/screens/history_screen.dart
import 'dart:async';
import 'package:flutter/material.dart';
import '../bus/event_bus.dart';
import '../contracts/contracts.dart' as ac;

class HistoryScreen extends StatefulWidget {
  const HistoryScreen({super.key});

  @override
  State<HistoryScreen> createState() => _HistoryScreenState();
}

class _HistoryScreenState extends State<HistoryScreen> {
  final List<ac.Action> _allActions = [];
  StreamSubscription<ac.Action>? _subscription;

  @override
  void initState() {
    super.initState();
    _subscription = bus.actions.listen((action) {
      setState(() => _allActions.insert(0, action));
    });
  }

  @override
  void dispose() {
    _subscription?.cancel();
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

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      backgroundColor: const Color(0xFF0D1117),
      appBar: AppBar(
        backgroundColor: const Color(0xFF161B22),
        title: const Text('Alert History',
            style: TextStyle(color: Colors.white)),
        iconTheme: const IconThemeData(color: Colors.white),
      ),
      body: _allActions.isEmpty
          ? const Center(
              child: Text('No alerts yet.',
                  style: TextStyle(color: Colors.white38)))
          : ListView.separated(
              itemCount: _allActions.length,
              separatorBuilder: (_, __) =>
                  const Divider(color: Colors.white12, height: 1),
              itemBuilder: (_, i) {
                final a = _allActions[i];
                final color = _severityColor(a.severity);
                return ListTile(
                  leading: CircleAvatar(
                    backgroundColor: color.withAlpha(40),
                    child: Icon(Icons.notifications_outlined,
                        color: color, size: 20),
                  ),
                  title: Text(a.guidance,
                      style: const TextStyle(
                          color: Colors.white, fontSize: 14)),
                  subtitle: Text(
                    '${a.severity.name.toUpperCase()} · ${a.ruleId.split('_').first} · ${a.ruleId}'
                    '${a.acknowledged ? " · ✓ ACK" : ""}',
                    style: TextStyle(color: color, fontSize: 11),
                  ),
                  trailing: Text(
                    _formatTime(a.ts),
                    style: const TextStyle(
                        color: Colors.white38, fontSize: 11),
                  ),
                );
              },
            ),
    );
  }

  String _formatTime(int tsMs) {
    final dt = DateTime.fromMillisecondsSinceEpoch(tsMs);
    return '${dt.hour.toString().padLeft(2, '0')}:'
        '${dt.minute.toString().padLeft(2, '0')}:'
        '${dt.second.toString().padLeft(2, '0')}';
  }
}
