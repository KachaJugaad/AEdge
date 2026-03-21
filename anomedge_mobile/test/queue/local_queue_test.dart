// test/queue/local_queue_test.dart
import 'package:flutter_test/flutter_test.dart';
import 'package:sqflite_common_ffi/sqflite_ffi.dart';
import 'package:uuid/uuid.dart';
import 'package:anomedge_mobile/queue/local_queue.dart';
import 'package:anomedge_mobile/contracts/contracts.dart';

const _uuid = Uuid();
int _seqN = 0;

EventEnvelope _makeEnvelope({String? id}) {
  final nowMs = DateTime.now().millisecondsSinceEpoch;
  const ruleId = 'coolant_overheat_warn';
  final envId = id ?? _uuid.v4();
  _seqN++;
  final decision = Decision(
    ts: nowMs,
    assetId: 'TEST-ASSET',
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
    id: envId,
    topic: 'decisions.gated',
    seq: _seqN,
    ts: nowMs,
    payload: decision.toJson(),
    assetId: 'TEST-ASSET',
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

  group('LocalQueue', () {
    test('write() stores an event as pending', () async {
      await queue.write(_makeEnvelope());
      expect(await queue.pendingCount(), equals(1));
    });

    test('readBatch() returns pending events in FIFO order', () async {
      final e1 = _makeEnvelope(id: 'aaa');
      await Future.delayed(const Duration(milliseconds: 5));
      final e2 = _makeEnvelope(id: 'bbb');
      await Future.delayed(const Duration(milliseconds: 5));
      final e3 = _makeEnvelope(id: 'ccc');

      await queue.write(e1);
      await queue.write(e2);
      await queue.write(e3);

      final batch = await queue.readBatch(10);
      expect(batch.length, equals(3));
      expect(batch[0].id, equals('aaa'));
      expect(batch[1].id, equals('bbb'));
      expect(batch[2].id, equals('ccc'));
    });

    test('readBatch(n) returns at most n events', () async {
      for (var i = 0; i < 10; i++) {
        await queue.write(_makeEnvelope());
      }
      final batch = await queue.readBatch(3);
      expect(batch.length, equals(3));
    });

    test('markSynced() marks specific IDs as synced', () async {
      final e1 = _makeEnvelope(id: 'id-1');
      final e2 = _makeEnvelope(id: 'id-2');
      final e3 = _makeEnvelope(id: 'id-3');
      await queue.write(e1);
      await queue.write(e2);
      await queue.write(e3);

      await queue.markSynced(['id-1', 'id-3']);

      expect(await queue.pendingCount(), equals(1));
      final batch = await queue.readBatch(10);
      expect(batch.first.id, equals('id-2'));
    });

    test('markSynced() with empty list is a no-op', () async {
      await queue.write(_makeEnvelope());
      await queue.markSynced([]);
      expect(await queue.pendingCount(), equals(1));
    });

    test('pendingCount() returns 0 when empty', () async {
      expect(await queue.pendingCount(), equals(0));
    });

    test('totalCount() includes synced and pending', () async {
      final e1 = _makeEnvelope(id: 'sync-1');
      final e2 = _makeEnvelope(id: 'pend-1');
      await queue.write(e1);
      await queue.write(e2);
      await queue.markSynced(['sync-1']);

      expect(await queue.totalCount(), equals(2));
      expect(await queue.pendingCount(), equals(1));
    });

    test('clearSynced() removes synced but keeps pending', () async {
      final e1 = _makeEnvelope(id: 'del-me');
      final e2 = _makeEnvelope(id: 'keep-me');
      await queue.write(e1);
      await queue.write(e2);
      await queue.markSynced(['del-me']);

      await queue.clearSynced();

      expect(await queue.totalCount(), equals(1));
      expect(await queue.pendingCount(), equals(1));
    });

    test('EventEnvelope survives serialisation round-trip', () async {
      final original = _makeEnvelope();
      await queue.write(original);

      final batch = await queue.readBatch(1);
      final restored = batch.first;

      expect(restored.id, equals(original.id));
      expect(restored.asDecision?.severity, equals(Severity.warn));
      expect(restored.asDecision?.ruleId, equals('coolant_overheat_warn'));
      expect(restored.synced, isFalse);
    });

    test('GATE: 1000 events all present and pending after write', () async {
      for (var i = 0; i < 1000; i++) {
        await queue.write(_makeEnvelope());
      }
      expect(await queue.totalCount(), equals(1000));
      expect(await queue.pendingCount(), equals(1000));
    }, timeout: const Timeout(Duration(seconds: 30)));
  });
}
