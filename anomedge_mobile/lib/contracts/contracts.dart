// lib/contracts/contracts.dart
// Dart mirror of packages/contracts/src/index.ts — Person A's FROZEN contracts.
// DO NOT MODIFY field names without all-team sign-off.
// Last verified against: AEdge-main/packages/contracts/src/index.ts (March 2026)

// ─── Severity ─────────────────────────────────────────────────────────────────
// TypeScript: 'NORMAL' | 'WATCH' | 'WARN' | 'HIGH' | 'CRITICAL'
// JSON wire format is UPPERCASE. fromJson lowercases for enum lookup.
enum Severity { normal, watch, warn, high, critical }

// ─── RuleGroup ────────────────────────────────────────────────────────────────
// TypeScript: 'thermal' | 'braking' | 'speed' | 'hydraulic' | 'electrical'
//           | 'dtc' | 'transmission' | 'fuel' | 'composite'
// JSON wire format is lowercase (matches TS literal values directly).
enum RuleGroup {
  thermal,
  braking,
  speed,
  hydraulic,
  electrical,
  dtc,
  transmission,
  fuel,
  composite,
  unknown,
}

// ─── DecisionSource ───────────────────────────────────────────────────────────
// TypeScript: 'EDGE_AI' | 'ML_STATISTICAL' | 'RULE_ENGINE'
enum DecisionSource { edgeAi, mlStatistical, ruleEngine, unknown }

// ─── ActionSource ─────────────────────────────────────────────────────────────
// TypeScript: 'TEMPLATE' | 'LLM'
enum ActionSource { template, llm }

// ─── Decision ─────────────────────────────────────────────────────────────────
// Published by Person A on topic: decisions.gated
class Decision {
  final int ts;
  final String assetId;
  final Severity severity;
  final String ruleId;
  final RuleGroup ruleGroup;
  final double confidence;
  final List<String> triggeredBy;
  final double rawValue;
  final double threshold;
  final DecisionSource decisionSource;
  final Map<String, dynamic>? context;

  const Decision({
    required this.ts,
    required this.assetId,
    required this.severity,
    required this.ruleId,
    required this.ruleGroup,
    required this.confidence,
    required this.triggeredBy,
    required this.rawValue,
    required this.threshold,
    required this.decisionSource,
    this.context,
  });

  DateTime get timestamp => DateTime.fromMillisecondsSinceEpoch(ts);

  factory Decision.fromJson(Map<String, dynamic> json) => Decision(
        ts: json['ts'] as int,
        assetId: json['asset_id'] as String,
        severity: _parseSeverity(json['severity'] as String),
        ruleId: json['rule_id'] as String,
        ruleGroup: _parseRuleGroup(json['rule_group'] as String),
        confidence: (json['confidence'] as num).toDouble(),
        triggeredBy: (json['triggered_by'] as List<dynamic>?)
                ?.map((e) => e as String)
                .toList() ??
            [],
        rawValue: (json['raw_value'] as num).toDouble(),
        threshold: (json['threshold'] as num).toDouble(),
        decisionSource:
            _parseDecisionSource(json['decision_source'] as String? ?? ''),
        context: json['context'] as Map<String, dynamic>?,
      );

  Map<String, dynamic> toJson() => {
        'ts': ts,
        'asset_id': assetId,
        'severity': severity.name.toUpperCase(),
        'rule_id': ruleId,
        'rule_group': ruleGroup == RuleGroup.unknown ? 'unknown' : ruleGroup.name,
        'confidence': confidence,
        'triggered_by': triggeredBy,
        'raw_value': rawValue,
        'threshold': threshold,
        'decision_source': _decisionSourceToString(decisionSource),
        if (context != null) 'context': context,
      };

  static Severity _parseSeverity(String s) {
    switch (s.toUpperCase()) {
      case 'NORMAL':   return Severity.normal;
      case 'WATCH':    return Severity.watch;
      case 'WARN':     return Severity.warn;
      case 'HIGH':     return Severity.high;
      case 'CRITICAL': return Severity.critical;
      default:         return Severity.normal;
    }
  }

  static RuleGroup _parseRuleGroup(String s) {
    switch (s.toLowerCase()) {
      case 'thermal':      return RuleGroup.thermal;
      case 'braking':      return RuleGroup.braking;
      case 'speed':        return RuleGroup.speed;
      case 'hydraulic':    return RuleGroup.hydraulic;
      case 'electrical':   return RuleGroup.electrical;
      case 'dtc':          return RuleGroup.dtc;
      case 'transmission': return RuleGroup.transmission;
      case 'fuel':         return RuleGroup.fuel;
      case 'composite':    return RuleGroup.composite;
      default:             return RuleGroup.unknown;
    }
  }

  static DecisionSource _parseDecisionSource(String s) {
    switch (s.toUpperCase()) {
      case 'EDGE_AI':         return DecisionSource.edgeAi;
      case 'ML_STATISTICAL':  return DecisionSource.mlStatistical;
      case 'RULE_ENGINE':     return DecisionSource.ruleEngine;
      default:                return DecisionSource.unknown;
    }
  }

  static String _decisionSourceToString(DecisionSource ds) {
    switch (ds) {
      case DecisionSource.edgeAi:        return 'EDGE_AI';
      case DecisionSource.mlStatistical: return 'ML_STATISTICAL';
      case DecisionSource.ruleEngine:    return 'RULE_ENGINE';
      case DecisionSource.unknown:       return 'RULE_ENGINE';
    }
  }

  // Exposed for use in Action and other files
  static RuleGroup parseRuleGroup(String s) => _parseRuleGroup(s);
  static Severity parseSeverity(String s) => _parseSeverity(s);
}

// ─── Action ───────────────────────────────────────────────────────────────────
// Published by YOUR app on topic: actions
//
// Field renames from old contracts:
//   guidanceText  → guidance   (matches TS)
//   shouldSpeak   → speak      (matches TS)
//   decisionRuleId → ruleId    (matches TS field name: rule_id)
// Added:
//   seq, title, source, decisionSource
class Action {
  final int seq;
  final int ts;
  final String assetId;
  final Severity severity;
  final String title;
  final String guidance;
  final String ruleId;
  final bool speak;
  final bool acknowledged;
  final ActionSource source;
  final DecisionSource decisionSource;
  final String actionId; // internal routing only — not in TS contract

  const Action({
    required this.seq,
    required this.ts,
    required this.assetId,
    required this.severity,
    required this.title,
    required this.guidance,
    required this.ruleId,
    required this.speak,
    this.acknowledged = false,
    required this.source,
    required this.decisionSource,
    required this.actionId,
  });

  factory Action.fromJson(Map<String, dynamic> json) => Action(
        seq: json['seq'] as int? ?? 0,
        ts: json['ts'] as int,
        assetId: json['asset_id'] as String,
        severity: Decision._parseSeverity(json['severity'] as String),
        title: json['title'] as String? ?? '',
        guidance: json['guidance'] as String,
        ruleId: json['rule_id'] as String,
        speak: json['speak'] as bool? ?? false,
        acknowledged: json['acknowledged'] as bool? ?? false,
        source: (json['source'] as String?) == 'LLM'
            ? ActionSource.llm
            : ActionSource.template,
        decisionSource: Decision._parseDecisionSource(
            json['decision_source'] as String? ?? ''),
        actionId: json['action_id'] as String? ?? '',
      );

  Map<String, dynamic> toJson() => {
        'seq': seq,
        'ts': ts,
        'asset_id': assetId,
        'severity': severity.name.toUpperCase(),
        'title': title,
        'guidance': guidance,
        'rule_id': ruleId,
        'speak': speak,
        'acknowledged': acknowledged,
        'source': source == ActionSource.llm ? 'LLM' : 'TEMPLATE',
        'decision_source': Decision._decisionSourceToString(decisionSource),
      };

  Action acknowledge() => Action(
        seq: seq,
        ts: ts,
        assetId: assetId,
        severity: severity,
        title: title,
        guidance: guidance,
        ruleId: ruleId,
        speak: false,
        acknowledged: true,
        source: source,
        decisionSource: decisionSource,
        actionId: actionId,
      );
}

// ─── EventEnvelope ────────────────────────────────────────────────────────────
// Matches TypeScript EventEnvelope<T>. Wire format from Person A's WebSocket.
class EventEnvelope {
  final String id;
  final String topic;
  final int seq;
  final int ts;
  final Map<String, dynamic> payload;
  final String assetId; // local only
  bool synced;          // local only

  EventEnvelope({
    required this.id,
    required this.topic,
    required this.seq,
    required this.ts,
    required this.payload,
    required this.assetId,
    this.synced = false,
  });

  factory EventEnvelope.fromJson(Map<String, dynamic> json) => EventEnvelope(
        id: json['id'] as String,
        topic: json['topic'] as String,
        seq: json['seq'] as int,
        ts: json['ts'] as int,
        payload: json['payload'] as Map<String, dynamic>,
        assetId: json['asset_id'] as String? ?? '',
        synced: json['synced'] as bool? ?? false,
      );

  Map<String, dynamic> toJson() => {
        'id': id,
        'topic': topic,
        'seq': seq,
        'ts': ts,
        'payload': payload,
        'asset_id': assetId,
        'synced': synced,
      };

  Decision? get asDecision {
    try { return Decision.fromJson(payload); } catch (_) { return null; }
  }

  Action? get asAction {
    try { return Action.fromJson(payload); } catch (_) { return null; }
  }
}
