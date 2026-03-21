// lib/tts/tts_service.dart
// TtsService — subscribes to bus.actionsSpoken (HIGH + CRITICAL only).
// Speaks guidance text aloud via flutter_tts.
// Rules:
//   - Only speaks HIGH and CRITICAL (bus already filters via actionsSpoken topic)
//   - Does not interrupt speech in progress — queues instead
//   - isSpeaking flag lets UI show a visual indicator
//   - Respects device volume
//   - Can be enabled/disabled from Settings

import 'dart:async';
import 'package:flutter_tts/flutter_tts.dart';
import '../bus/event_bus.dart';
import '../contracts/contracts.dart';

class TtsService {
  final FlutterTts _tts = FlutterTts();

  StreamSubscription<Action>? _subscription;
  bool _running = false;
  bool _enabled = true;
  bool isSpeaking = false;

  final List<String> _queue = [];

  // UI listens to this to show/hide the speaking indicator
  void Function(bool speaking)? onSpeakingChanged;

  Future<void> start() async {
    if (_running) return;
    _running = true;

    await _tts.setLanguage('en-US');
    await _tts.setSpeechRate(0.45);  // slightly slower for noisy environments
    await _tts.setVolume(1.0);
    await _tts.setPitch(1.0);

    _tts.setCompletionHandler(() {
      isSpeaking = false;
      onSpeakingChanged?.call(false);
      _speakNext();
    });

    _tts.setErrorHandler((_) {
      isSpeaking = false;
      onSpeakingChanged?.call(false);
      _queue.clear();
    });

    // Only HIGH and CRITICAL arrive on actionsSpoken
    _subscription = bus.actionsSpoken.listen(_onAction);
  }

  void stop() {
    _running = false;
    _subscription?.cancel();
    _subscription = null;
    _tts.stop();
    _queue.clear();
    isSpeaking = false;
  }

  void setEnabled(bool enabled) {
    _enabled = enabled;
    if (!enabled) {
      _tts.stop();
      _queue.clear();
      isSpeaking = false;
      onSpeakingChanged?.call(false);
    }
  }

  void _onAction(Action action) {
    if (!_enabled) return;

    // Prefix CRITICAL with an audio cue so operator knows severity immediately
    final text = action.severity == Severity.critical
        ? 'Critical alert. ${action.guidance}'
        : action.guidance;

    _queue.add(text);
    if (!isSpeaking) _speakNext();
  }

  Future<void> _speakNext() async {
    if (_queue.isEmpty || !_enabled) return;
    final text = _queue.removeAt(0);
    isSpeaking = true;
    onSpeakingChanged?.call(true);
    await _tts.speak(text);
  }

  // Called from AlertScreen replay button
  Future<void> replay(String text) async {
    if (!_enabled) return;
    await _tts.stop();
    _queue.clear();
    isSpeaking = true;
    onSpeakingChanged?.call(true);
    await _tts.speak(text);
  }

  void dispose() {
    stop();
    _tts.stop();
  }
}

final ttsService = TtsService();
