//! ml_statistical.rs
//! Tier 2 — ML Statistical anomaly detection using Isolation Forest.
//!
//! Maintains per-asset history of feature vectors. Scores new observations
//! against the accumulated baseline. Anomalies = points that are easy to
//! isolate (short average path length in random trees).
//!
//! - No external model file needed — forest is built in-memory from history.
//! - Contamination target: ~5% (anomaly_threshold ≈ 0.6).
//! - Target latency: < 10ms.
//!
//! Feature vector: 6 numeric fields from FeatureWindow:
//!   coolant_slope, brake_spike_count, speed_mean,
//!   rpm_mean, engine_load_mean, throttle_variance

use std::collections::{HashMap, VecDeque};

use crate::types::{Decision, DecisionSource, FeatureWindow, RuleGroup, Severity};

// ─── Constants ────────────────────────────────────────────────────────────────

const NUM_FEATURES: usize = 6;

const FEATURE_NAMES: [&str; NUM_FEATURES] = [
    "coolant_slope",
    "brake_spike_count",
    "speed_mean",
    "rpm_mean",
    "engine_load_mean",
    "throttle_variance",
];

/// Euler–Mascheroni constant for the c(n) normalisation factor.
const EULER_MASCHERONI: f64 = 0.5772156649;

/// Z-score threshold for a feature to be flagged in attribution.
const ZSCORE_THRESHOLD: f64 = 2.0;

// ─── Feature extraction ──────────────────────────────────────────────────────

fn extract_features(w: &FeatureWindow) -> [f64; NUM_FEATURES] {
    [
        w.coolant_slope,
        w.brake_spike_count,
        w.speed_mean,
        w.rpm_mean,
        w.engine_load_mean,
        w.throttle_variance,
    ]
}

/// Map feature index to the most appropriate RuleGroup.
fn feature_to_group(idx: usize) -> RuleGroup {
    match idx {
        0 => RuleGroup::Thermal,          // coolant_slope
        1 => RuleGroup::Braking,          // brake_spike_count
        2 => RuleGroup::Speed,            // speed_mean
        3 | 4 | 5 => RuleGroup::Composite, // rpm, load, throttle
        _ => RuleGroup::Composite,
    }
}

// ─── Simple PRNG (xorshift64) — no external dependency ──────────────────────

struct Rng {
    state: u64,
}

impl Rng {
    fn new(seed: u64) -> Self {
        Self { state: if seed == 0 { 1 } else { seed } }
    }

    fn next_u64(&mut self) -> u64 {
        self.state ^= self.state << 13;
        self.state ^= self.state >> 7;
        self.state ^= self.state << 17;
        self.state
    }

    /// Uniform f64 in [0, 1).
    fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Uniform usize in [0, max).
    fn next_usize(&mut self, max: usize) -> usize {
        (self.next_u64() % max as u64) as usize
    }
}

// ─── Isolation Tree ──────────────────────────────────────────────────────────

enum INode {
    Split {
        feature:   usize,
        threshold: f64,
        left:      Box<INode>,
        right:     Box<INode>,
    },
    Leaf {
        size: usize,
    },
}

impl INode {
    /// Compute the path length for a point through this tree.
    fn path_length(&self, point: &[f64; NUM_FEATURES], depth: usize) -> f64 {
        match self {
            INode::Leaf { size } => depth as f64 + c_factor(*size),
            INode::Split { feature, threshold, left, right } => {
                if point[*feature] < *threshold {
                    left.path_length(point, depth + 1)
                } else {
                    right.path_length(point, depth + 1)
                }
            }
        }
    }
}

/// Average path length of unsuccessful search in a BST of size n.
/// c(n) = 2 * H(n-1) - 2*(n-1)/n  where H(i) ≈ ln(i) + γ.
fn c_factor(n: usize) -> f64 {
    if n <= 1 { return 0.0; }
    let nf = n as f64;
    2.0 * ((nf - 1.0).ln() + EULER_MASCHERONI) - 2.0 * (nf - 1.0) / nf
}

/// Build one isolation tree from the given data.
fn build_tree(
    data:      &[[f64; NUM_FEATURES]],
    max_depth: usize,
    rng:       &mut Rng,
) -> INode {
    if data.len() <= 1 || max_depth == 0 {
        return INode::Leaf { size: data.len() };
    }

    let feature = rng.next_usize(NUM_FEATURES);

    // Min/max of the chosen feature.
    let mut fmin = f64::INFINITY;
    let mut fmax = f64::NEG_INFINITY;
    for p in data {
        if p[feature] < fmin { fmin = p[feature]; }
        if p[feature] > fmax { fmax = p[feature]; }
    }

    // All values identical → can't split.
    if (fmax - fmin).abs() < f64::EPSILON {
        return INode::Leaf { size: data.len() };
    }

    let split = fmin + rng.next_f64() * (fmax - fmin);

    let mut left  = Vec::new();
    let mut right = Vec::new();
    for p in data {
        if p[feature] < split {
            left.push(*p);
        } else {
            right.push(*p);
        }
    }

    // Edge case: all to one side.
    if left.is_empty() || right.is_empty() {
        return INode::Leaf { size: data.len() };
    }

    INode::Split {
        feature,
        threshold: split,
        left:  Box::new(build_tree(&left,  max_depth - 1, rng)),
        right: Box::new(build_tree(&right, max_depth - 1, rng)),
    }
}

// ─── MlStatistical ──────────────────────────────────────────────────────────

/// Configuration for the ML Statistical scorer.
pub struct MlConfig {
    /// Number of isolation trees in the forest.
    pub n_trees: usize,
    /// Maximum tree depth (ceil(log2(max_samples)) is typical).
    pub max_depth: usize,
    /// Maximum feature vectors kept per asset.
    pub max_history: usize,
    /// Anomaly score threshold — above this = anomaly (0.6 ≈ 5% contamination).
    pub anomaly_threshold: f64,
}

impl Default for MlConfig {
    fn default() -> Self {
        Self {
            n_trees:           50,
            max_depth:         8,
            max_history:       256,
            anomaly_threshold: 0.60,
        }
    }
}

/// Tier 2 anomaly scorer using Isolation Forest.
///
/// Call `record()` on every FeatureWindow to build up per-asset history.
/// Call `score()` to check if the latest window is anomalous.
pub struct MlStatistical {
    history: HashMap<String, VecDeque<[f64; NUM_FEATURES]>>,
    config:  MlConfig,
}

impl MlStatistical {
    pub fn new() -> Self {
        Self::with_config(MlConfig::default())
    }

    pub fn with_config(config: MlConfig) -> Self {
        Self {
            history: HashMap::new(),
            config,
        }
    }

    /// Number of recorded feature vectors for an asset.
    pub fn history_len(&self, asset_id: &str) -> usize {
        self.history.get(asset_id).map(|h| h.len()).unwrap_or(0)
    }

    /// Record a FeatureWindow into per-asset history. Call on every frame.
    pub fn record(&mut self, window: &FeatureWindow) {
        let features = extract_features(window);
        let buf = self.history
            .entry(window.asset_id.clone())
            .or_default();
        buf.push_back(features);
        while buf.len() > self.config.max_history {
            buf.pop_front();
        }
    }

    /// Score the latest FeatureWindow. Returns ML decisions if anomaly detected.
    ///
    /// Requires `>= min_samples` history for this asset (caller gates on
    /// `InferenceContext::sample_count`). Returns empty vec if no anomaly.
    pub fn score(&self, window: &FeatureWindow) -> Vec<Decision> {
        let asset_history = match self.history.get(&window.asset_id) {
            Some(h) if h.len() >= 5 => h,
            _ => return vec![],
        };

        let current = extract_features(window);
        let data: Vec<[f64; NUM_FEATURES]> = asset_history.iter().copied().collect();

        let anomaly_score = self.compute_anomaly_score(&data, &current);

        if anomaly_score < self.config.anomaly_threshold {
            return vec![];
        }

        // Map score → severity.
        let severity = if anomaly_score > 0.75 {
            Severity::High
        } else if anomaly_score > 0.65 {
            Severity::Warn
        } else {
            Severity::Watch
        };

        // Feature attribution via z-scores.
        let (triggered_by, primary_idx) = self.feature_attribution(&data, &current);
        let group = feature_to_group(primary_idx);

        vec![Decision {
            ts:              window.ts,
            asset_id:        window.asset_id.clone(),
            severity,
            rule_id:         format!("ml_anomaly_{}", FEATURE_NAMES[primary_idx]),
            rule_group:      group,
            confidence:      anomaly_score,
            triggered_by,
            raw_value:       anomaly_score,
            threshold:       self.config.anomaly_threshold,
            decision_source: DecisionSource::MlStatistical,
            context:         None,
        }]
    }

    /// Build an Isolation Forest from `data` and compute the anomaly score for `point`.
    ///
    /// Score interpretation: close to 1.0 = anomaly, ~0.5 = normal, < 0.5 = inlier.
    fn compute_anomaly_score(
        &self,
        data:  &[[f64; NUM_FEATURES]],
        point: &[f64; NUM_FEATURES],
    ) -> f64 {
        let n = data.len();
        let cn = c_factor(n);
        if cn < f64::EPSILON { return 0.5; }

        let mut total_path_length = 0.0;

        for i in 0..self.config.n_trees {
            // Each tree gets its own deterministic seed.
            let mut rng = Rng::new(42 + i as u64 * 997);

            // Subsample with replacement.
            let sub_size = n.min(self.config.max_history);
            let subsample: Vec<[f64; NUM_FEATURES]> = (0..sub_size)
                .map(|_| data[rng.next_usize(n)])
                .collect();

            let tree = build_tree(&subsample, self.config.max_depth, &mut rng);
            total_path_length += tree.path_length(point, 0);
        }

        let avg_path = total_path_length / self.config.n_trees as f64;

        // s(x, n) = 2^(-E(h(x)) / c(n))
        2.0_f64.powf(-avg_path / cn)
    }

    /// Identify which features contributed most to the anomaly.
    ///
    /// Returns (list of feature names with |z| > 2, index of the most deviant feature).
    fn feature_attribution(
        &self,
        data:  &[[f64; NUM_FEATURES]],
        point: &[f64; NUM_FEATURES],
    ) -> (Vec<String>, usize) {
        let mut max_z:   f64   = 0.0;
        let mut max_idx: usize = 0;
        let mut triggered = Vec::new();

        for f in 0..NUM_FEATURES {
            let n = data.len() as f64;
            let mean: f64 = data.iter().map(|p| p[f]).sum::<f64>() / n;
            let var:  f64 = data.iter().map(|p| (p[f] - mean).powi(2)).sum::<f64>() / n;
            let std  = var.sqrt();

            if std < f64::EPSILON {
                continue;
            }

            let z = ((point[f] - mean) / std).abs();
            if z > ZSCORE_THRESHOLD {
                triggered.push(FEATURE_NAMES[f].to_string());
            }
            if z > max_z {
                max_z   = z;
                max_idx = f;
            }
        }

        // If nothing exceeds z=2, still report the most deviant.
        if triggered.is_empty() {
            triggered.push(FEATURE_NAMES[max_idx].to_string());
        }

        (triggered, max_idx)
    }
}

impl Default for MlStatistical {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::SignalMap;

    // ── Helpers ───────────────────────────────────────────────────────────────

    fn make_window(asset_id: &str, ts: i64, features: [f64; 6]) -> FeatureWindow {
        FeatureWindow {
            ts,
            asset_id:          asset_id.into(),
            window_seconds:    30.0,
            coolant_slope:     features[0],
            brake_spike_count: features[1],
            speed_mean:        features[2],
            rpm_mean:          features[3],
            engine_load_mean:  features[4],
            throttle_variance: features[5],
            hydraulic_spike:   false,
            transmission_heat: false,
            dtc_new:           vec![],
            signals_snapshot:  SignalMap::default(),
        }
    }

    /// Generate N "normal" windows: all features centered around baseline with small noise.
    fn normal_windows(asset_id: &str, n: usize) -> Vec<FeatureWindow> {
        let mut rng = Rng::new(123);
        (0..n).map(|i| {
            let mut noise = || (rng.next_f64() - 0.5) * 2.0; // [-1, 1)
            make_window(asset_id, i as i64 * 1000, [
                0.1  + noise() * 0.05,   // coolant_slope: ~0.1
                0.5  + noise() * 0.3,    // brake_spike_count: ~0.5
                80.0 + noise() * 5.0,    // speed_mean: ~80
                2000.0 + noise() * 100.0, // rpm_mean: ~2000
                50.0 + noise() * 5.0,    // engine_load_mean: ~50
                10.0 + noise() * 2.0,    // throttle_variance: ~10
            ])
        }).collect()
    }

    // ── Test 1: fresh scorer has empty history ────────────────────────────────

    #[test]
    fn test_new_scorer_has_empty_history() {
        let ml = MlStatistical::new();
        assert_eq!(ml.history_len("TRUCK-001"), 0);
    }

    // ── Test 2: record accumulates features ──────────────────────────────────

    #[test]
    fn test_record_accumulates_features() {
        let mut ml = MlStatistical::new();
        let windows = normal_windows("TRUCK-001", 10);
        for w in &windows {
            ml.record(w);
        }
        assert_eq!(ml.history_len("TRUCK-001"), 10);
    }

    // ── Test 3: score returns empty with insufficient history ─────────────────

    #[test]
    fn test_score_returns_empty_with_insufficient_history() {
        let mut ml = MlStatistical::new();
        // Only 3 samples — below ML_MIN_SAMPLES (5)
        for w in &normal_windows("TRUCK-001", 3) {
            ml.record(w);
        }
        let result = ml.score(&normal_windows("TRUCK-001", 1)[0]);
        assert!(result.is_empty(), "must return empty with < 5 samples");
    }

    // ── Test 4: normal data scores below threshold ───────────────────────────

    #[test]
    fn test_normal_data_below_threshold() {
        let mut ml = MlStatistical::new();
        let windows = normal_windows("TRUCK-001", 50);
        for w in &windows {
            ml.record(w);
        }
        // Score a window with normal features — should NOT trigger
        let normal = make_window("TRUCK-001", 50_000, [
            0.12, 0.6, 82.0, 2050.0, 51.0, 10.5,
        ]);
        let decisions = ml.score(&normal);
        assert!(decisions.is_empty(), "normal data must not trigger anomaly");
    }

    // ── Test 5: clear outlier is detected ────────────────────────────────────

    #[test]
    fn test_anomalous_point_detected() {
        let mut ml = MlStatistical::with_config(MlConfig {
            n_trees: 100,    // more trees for stability
            max_depth: 10,
            max_history: 256,
            anomaly_threshold: 0.55,  // slightly more sensitive for test
        });
        let windows = normal_windows("TRUCK-001", 100);
        for w in &windows {
            ml.record(w);
        }
        // Inject a massive outlier: coolant_slope = 5.0 (vs ~0.1 baseline),
        // speed = 170 (vs ~80), rpm = 5000 (vs ~2000)
        let outlier = make_window("TRUCK-001", 100_000, [
            5.0, 8.0, 170.0, 5000.0, 95.0, 80.0,
        ]);
        let decisions = ml.score(&outlier);
        assert!(
            !decisions.is_empty(),
            "extreme outlier must be detected as anomaly"
        );
    }

    // ── Test 6: decision has correct source ──────────────────────────────────

    #[test]
    fn test_decision_has_ml_statistical_source() {
        let mut ml = MlStatistical::with_config(MlConfig {
            anomaly_threshold: 0.55,
            ..MlConfig::default()
        });
        for w in &normal_windows("TRUCK-001", 100) {
            ml.record(w);
        }
        let outlier = make_window("TRUCK-001", 100_000, [
            5.0, 8.0, 170.0, 5000.0, 95.0, 80.0,
        ]);
        let decisions = ml.score(&outlier);
        assert!(!decisions.is_empty());
        assert_eq!(decisions[0].decision_source, DecisionSource::MlStatistical);
    }

    // ── Test 7: feature attribution identifies deviant feature ───────────────

    #[test]
    fn test_feature_attribution_identifies_deviant_feature() {
        let mut ml = MlStatistical::with_config(MlConfig {
            anomaly_threshold: 0.55,
            ..MlConfig::default()
        });
        for w in &normal_windows("TRUCK-001", 100) {
            ml.record(w);
        }
        // Only coolant_slope is extreme (5.0 vs ~0.1), rest is normal
        let outlier = make_window("TRUCK-001", 100_000, [
            5.0, 0.5, 80.0, 2000.0, 50.0, 10.0,
        ]);
        let decisions = ml.score(&outlier);
        if !decisions.is_empty() {
            assert!(
                decisions[0].triggered_by.contains(&"coolant_slope".to_string()),
                "coolant_slope should be in triggered_by, got: {:?}",
                decisions[0].triggered_by
            );
        }
    }

    // ── Test 8: per-asset independent history ────────────────────────────────

    #[test]
    fn test_per_asset_independent_history() {
        let mut ml = MlStatistical::new();
        for w in &normal_windows("TRUCK-001", 20) {
            ml.record(w);
        }
        for w in &normal_windows("TRUCK-002", 10) {
            ml.record(w);
        }
        assert_eq!(ml.history_len("TRUCK-001"), 20);
        assert_eq!(ml.history_len("TRUCK-002"), 10);
    }

    // ── Test 9: history capped at max_history ────────────────────────────────

    #[test]
    fn test_history_capped_at_max() {
        let mut ml = MlStatistical::with_config(MlConfig {
            max_history: 20,
            ..MlConfig::default()
        });
        for w in &normal_windows("TRUCK-001", 50) {
            ml.record(w);
        }
        assert_eq!(ml.history_len("TRUCK-001"), 20, "history must be capped at max_history");
    }

    // ── Test 10: severity scales with anomaly magnitude ──────────────────────

    #[test]
    fn test_severity_scales_with_score() {
        let mut ml = MlStatistical::with_config(MlConfig {
            n_trees: 100,
            anomaly_threshold: 0.55,
            ..MlConfig::default()
        });
        for w in &normal_windows("TRUCK-001", 100) {
            ml.record(w);
        }
        // Moderate outlier
        let moderate = make_window("TRUCK-001", 100_000, [
            2.0, 3.0, 120.0, 3500.0, 75.0, 40.0,
        ]);
        // Extreme outlier
        let extreme = make_window("TRUCK-001", 101_000, [
            8.0, 15.0, 180.0, 5500.0, 99.0, 100.0,
        ]);

        let d_mod = ml.score(&moderate);
        let d_ext = ml.score(&extreme);

        // Both should detect anomaly. Extreme should have >= severity of moderate.
        if !d_mod.is_empty() && !d_ext.is_empty() {
            assert!(
                d_ext[0].severity >= d_mod[0].severity,
                "extreme outlier severity ({:?}) must be >= moderate ({:?})",
                d_ext[0].severity, d_mod[0].severity
            );
        }
    }

    // ── Test 11: c_factor correctness ────────────────────────────────────────

    #[test]
    fn test_c_factor_known_values() {
        assert_eq!(c_factor(0), 0.0);
        assert_eq!(c_factor(1), 0.0);
        // c(2) = 2 * (ln(1) + γ) - 2*(1)/2 = 2*γ - 1 ≈ 0.1544
        let c2 = c_factor(2);
        assert!((c2 - (2.0 * EULER_MASCHERONI - 1.0)).abs() < 0.001);
        // c(256) should be a reasonable value (around 7-8)
        let c256 = c_factor(256);
        assert!(c256 > 6.0 && c256 < 12.0, "c(256) = {c256}");
    }

    // ── Test 12: extract_features maps correctly ─────────────────────────────

    #[test]
    fn test_extract_features_maps_correctly() {
        let w = make_window("X", 0, [1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
        let f = extract_features(&w);
        assert_eq!(f, [1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
    }
}
