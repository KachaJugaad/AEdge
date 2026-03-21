// lib/sync/sync_agent.dart
import 'dart:async';
import 'dart:convert';
import 'package:connectivity_plus/connectivity_plus.dart';
import 'package:http/http.dart' as http;
import '../queue/local_queue.dart';

const _cloudSyncUrl = 'https://api.anomedge.ca/api/v1/sync';

enum SyncStatus { idle, syncing, offline, error }

class SyncAgent {
  final LocalQueue _queue;
  final String _syncUrl;
  final http.Client _client;

  SyncStatus status = SyncStatus.idle;
  int pendingSyncCount = 0;
  String? lastError;

  StreamSubscription<ConnectivityResult>? _connectivitySub;
  Timer? _retryTimer;
  bool _running = false;
  set running(bool v) => _running = v;

  void Function(SyncStatus)? onStatusChange;
  void Function(int pending)? onPendingCountChange;

  SyncAgent({
    LocalQueue? queue,
    String? syncUrl,
    http.Client? client,
  })  : _queue = queue ?? localQueue,
        _syncUrl = syncUrl ?? _cloudSyncUrl,
        _client = client ?? http.Client();

  void start() {
    if (_running) return;
    _running = true;
    _connectivitySub = Connectivity()
        .onConnectivityChanged
        .listen(_onConnectivityChanged);
    attemptSync();
  }

  void stop() {
    _running = false;
    _connectivitySub?.cancel();
    _retryTimer?.cancel();
  }

  Future<void> _onConnectivityChanged(ConnectivityResult result) async {
    final isOnline = result != ConnectivityResult.none;
    if (isOnline) {
      _retryTimer?.cancel();
      await attemptSync();
    } else {
      _setStatus(SyncStatus.offline);
    }
  }

  /// Public so tests and UI can trigger a manual sync
  Future<void> attemptSync() async {
    if (!_running) return;

    final pending = await _queue.pendingCount();
    pendingSyncCount = pending;
    onPendingCountChange?.call(pending);

    if (pending == 0) {
      _setStatus(SyncStatus.idle);
      return;
    }

    _setStatus(SyncStatus.syncing);

    int retryDelay = 30;
    int attempt = 0;
    const maxAttempts = 5;

    while (_running && attempt < maxAttempts) {
      try {
        final synced = await syncBatch();
        if (synced) {
          final remaining = await _queue.pendingCount();
          if (remaining > 0) {
            await attemptSync();
          } else {
            _setStatus(SyncStatus.idle);
          }
          return;
        }
      } catch (e) {
        lastError = e.toString();
      }

      attempt++;
      if (attempt >= maxAttempts) {
        _setStatus(SyncStatus.error);
        return;
      }

      _setStatus(SyncStatus.error);
      await Future.delayed(Duration(seconds: retryDelay));
      retryDelay = (retryDelay * 2).clamp(30, 300);
    }
  }

  /// Public for testing — syncs one batch, returns true on success
  Future<bool> syncBatch() async {
    final batch = await _queue.readBatch(50);
    if (batch.isEmpty) return true;

    final payload = jsonEncode(batch.map((e) => e.toJson()).toList());

    final response = await _client
        .post(
          Uri.parse(_syncUrl),
          headers: {'Content-Type': 'application/json'},
          body: payload,
        )
        .timeout(const Duration(seconds: 30));

    if (response.statusCode == 200) {
      final body = jsonDecode(response.body) as Map<String, dynamic>;
      final confirmed = (body['confirmed'] as List?)
              ?.map((id) => id.toString())
              .toList() ??
          [];

      if (confirmed.isNotEmpty) {
        await _queue.markSynced(confirmed);
      }

      pendingSyncCount = await _queue.pendingCount();
      onPendingCountChange?.call(pendingSyncCount);
      return true;
    }

    return false;
  }

  void _setStatus(SyncStatus newStatus) {
    status = newStatus;
    onStatusChange?.call(newStatus);
  }

  void dispose() {
    stop();
    _client.close();
  }
}

final syncAgent = SyncAgent();
