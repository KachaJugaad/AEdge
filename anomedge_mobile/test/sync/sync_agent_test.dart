// test/sync/sync_agent_test.dart
import 'dart:convert';
import 'package:flutter_test/flutter_test.dart';
import 'package:http/http.dart' as http;
import 'package:http/testing.dart';
import 'package:sqflite_common_ffi/sqflite_ffi.dart';
import 'package:uuid/uuid.dart';
import 'package:anomedge_mobile/sync/sync_agent.dart';
import 'package:anomedge_mobile/queue/local_queue.dart';
import 'package:anomedge_mobile/contracts/contracts.dart';

const _uuid = Uuid();
int _seqN = 0;

EventEnvelope _makeEnvelope({String? id}) {
  final nowMs = DateTime.now().millisecondsSinceEpoch;
  const ruleId = 'coolant_overheat_warn';
  final decision = Decision(
    ts: nowMs,
    assetId: 'TEST',
    severity: Severity.warn,
    ruleId: ruleId,
    ruleGroup: RuleGroup.thermal,
    confidence: 1.0,
    triggeredBy: ['coolant_slope'],
    rawValue: 105.0,
    threshold: 100.0,
    decisionSource: DecisionSource.ruleEngine,
  );
  return EventEnvelope(
    id: id ?? _uuid.v4(),
    topic: 'decisions.gated',
    seq: ++_seqN,
    ts: nowMs,
    payload: decision.toJson(),
    assetId: 'TEST',
  );
}

void main() {
  late LocalQueue queue;

  setUpAll(() {
    sqfliteFfiInit();
    databaseFactory = databaseFactoryFfi;
  });

  setUp(() async {
    queue = LocalQueue();
    await queue.clear();
  });

  tearDown(() async {
    await queue.clear();
    await queue.close();
  });

  group('SyncAgent', () {
    test('successful sync marks all confirmed IDs as synced', () async {
      final e1 = _makeEnvelope(id: 'evt-1');
      final e2 = _makeEnvelope(id: 'evt-2');
      await queue.write(e1);
      await queue.write(e2);

      final mockClient = MockClient((request) async {
        final body = jsonDecode(request.body) as List;
        final ids = body.map((e) => e['id'] as String).toList();
        return http.Response(
          jsonEncode({'confirmed': ids}),
          200,
          headers: {'content-type': 'application/json'},
        );
      });

      final agent = SyncAgent(
        queue: queue,
        syncUrl: 'https://mock.anomedge.ca/api/v1/sync',
        client: mockClient,
      );

      await agent.syncBatch();
      expect(await queue.pendingCount(), equals(0));
    });

    test('partial failure — only confirmed IDs marked synced', () async {
      await queue.write(_makeEnvelope(id: 'partial-1'));
      await queue.write(_makeEnvelope(id: 'partial-2'));
      await queue.write(_makeEnvelope(id: 'partial-3'));

      final mockClient = MockClient((_) async {
        return http.Response(
          jsonEncode({'confirmed': ['partial-1', 'partial-2']}),
          200,
          headers: {'content-type': 'application/json'},
        );
      });

      final agent = SyncAgent(
        queue: queue,
        syncUrl: 'https://mock.anomedge.ca/api/v1/sync',
        client: mockClient,
      );

      await agent.syncBatch();

      expect(await queue.pendingCount(), equals(1));
      final remaining = await queue.readBatch(10);
      expect(remaining.first.id, equals('partial-3'));
    });

    test('non-200 response returns false and leaves events pending', () async {
      await queue.write(_makeEnvelope(id: 'fail-1'));

      final mockClient = MockClient((_) async {
        return http.Response('Internal Server Error', 500);
      });

      final agent = SyncAgent(
        queue: queue,
        syncUrl: 'https://mock.anomedge.ca/api/v1/sync',
        client: mockClient,
      );

      final result = await agent.syncBatch();
      expect(result, isFalse);
      expect(await queue.pendingCount(), equals(1));
    });

    test('empty queue syncBatch returns true without HTTP call', () async {
      var httpCallCount = 0;
      final mockClient = MockClient((_) async {
        httpCallCount++;
        return http.Response('{}', 200);
      });

      final agent = SyncAgent(
        queue: queue,
        syncUrl: 'https://mock.anomedge.ca/api/v1/sync',
        client: mockClient,
      );

      final result = await agent.syncBatch();
      expect(result, isTrue);
      expect(httpCallCount, equals(0));
    });

    test('status changes to syncing during upload', () async {
      await queue.write(_makeEnvelope());

      final statuses = <SyncStatus>[];
      final mockClient = MockClient((_) async {
        await Future.delayed(const Duration(milliseconds: 10));
        return http.Response(
          jsonEncode({'confirmed': []}),
          200,
          headers: {'content-type': 'application/json'},
        );
      });

      final agent = SyncAgent(
        queue: queue,
        syncUrl: 'https://mock.anomedge.ca/api/v1/sync',
        client: mockClient,
      )
        ..running = true
        ..onStatusChange = statuses.add;

      await agent.attemptSync();
      expect(statuses, contains(SyncStatus.syncing));
    });
  });
}
