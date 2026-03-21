// lib/queue/local_queue.dart
// Offline SQLite event store. Survives app restarts. Marked pending until synced.

import 'dart:convert';
import 'package:sqflite/sqflite.dart';
import 'package:path/path.dart';
import '../contracts/contracts.dart';

class LocalQueue {
  static const _dbName = 'anomedge_queue.db';
  static const _tableName = 'event_queue';

  Database? _db;

  Future<Database> get database async {
    _db ??= await _initDb();
    return _db!;
  }

  Future<Database> _initDb() async {
    final dbPath = await getDatabasesPath();
    final path = join(dbPath, _dbName);
    return openDatabase(
      path,
      version: 1,
      onCreate: (db, version) async {
        await db.execute('''
          CREATE TABLE $_tableName (
            id TEXT PRIMARY KEY,
            payload TEXT NOT NULL,
            synced INTEGER NOT NULL DEFAULT 0,
            ts INTEGER NOT NULL
          )
        ''');
        await db.execute('CREATE INDEX idx_synced ON $_tableName (synced)');
        await db.execute('CREATE INDEX idx_ts ON $_tableName (ts)');
      },
    );
  }

  Future<void> write(EventEnvelope envelope) async {
    final db = await database;
    await db.insert(
      _tableName,
      {
        'id': envelope.id,
        'payload': jsonEncode(envelope.toJson()),
        'synced': 0,
        'ts': envelope.ts,
      },
      conflictAlgorithm: ConflictAlgorithm.replace,
    );
  }

  Future<List<EventEnvelope>> readBatch([int n = 50]) async {
    final db = await database;
    final rows = await db.query(
      _tableName,
      where: 'synced = 0',
      orderBy: 'ts ASC',
      limit: n,
    );
    return rows.map((row) {
      final json = jsonDecode(row['payload'] as String) as Map<String, dynamic>;
      return EventEnvelope.fromJson(json);
    }).toList();
  }

  Future<void> markSynced(List<String> ids) async {
    if (ids.isEmpty) return;
    final db = await database;
    final placeholders = ids.map((_) => '?').join(', ');
    await db.rawUpdate(
      'UPDATE $_tableName SET synced = 1 WHERE id IN ($placeholders)',
      ids,
    );
  }

  Future<int> pendingCount() async {
    final db = await database;
    final result = await db.rawQuery(
        'SELECT COUNT(*) as count FROM $_tableName WHERE synced = 0');
    return (result.first['count'] as int?) ?? 0;
  }

  Future<int> totalCount() async {
    final db = await database;
    final result =
        await db.rawQuery('SELECT COUNT(*) as count FROM $_tableName');
    return (result.first['count'] as int?) ?? 0;
  }

  Future<void> clearSynced() async {
    final db = await database;
    await db.delete(_tableName, where: 'synced = 1');
  }

  Future<void> clear() async {
    final db = await database;
    await db.delete(_tableName);
  }

  Future<void> close() async {
    await _db?.close();
    _db = null;
  }
}

final localQueue = LocalQueue();
