//! Interleaved (ABAB) sampling and paired-ratio statistics.

use std::time::Instant;

use serde_json::{json, Value as Json};

use super::expected;
use super::Db;

/// Raw per-sample wall times in seconds. Index `i` of every vector belongs to
/// the same ABAB round.
#[derive(Default, Debug)]
pub struct Series {
    pub heap: Vec<f64>,
    pub column: Vec<f64>,
    /// Second heap sample of the same query (heap-versus-heap control).
    pub control: Vec<f64>,
}

/// One timed execution including result materialization and the fingerprint
/// check, which both arms pay identically.
fn sample(db: &Db, sql: &str, fingerprint: (usize, u64, u64)) -> Result<f64, String> {
    let started = Instant::now();
    let rows = db.try_exec(sql)?;
    let observed = expected::fingerprint(&rows);
    let elapsed = started.elapsed().as_secs_f64();
    if observed != fingerprint {
        return Err(format!(
            "timed result fingerprint {observed:?} differs from {fingerprint:?}"
        ));
    }
    std::hint::black_box(rows.len());
    Ok(elapsed)
}

/// Runs `warmup` untimed and `samples` measured rounds of heap, column and
/// (optionally) a second heap control, always in that order.
pub fn alternate(
    db: &Db,
    heap_sql: &str,
    column_sql: &str,
    fingerprint: (usize, u64, u64),
    warmup: usize,
    samples: usize,
    control: bool,
) -> Result<Series, String> {
    let mut series = Series::default();
    for round in 0..warmup + samples {
        let heap = sample(db, heap_sql, fingerprint)?;
        let column = sample(db, column_sql, fingerprint)?;
        let control_sample = if control {
            Some(sample(db, heap_sql, fingerprint)?)
        } else {
            None
        };
        if round >= warmup {
            series.heap.push(heap);
            series.column.push(column);
            series.control.extend(control_sample);
        }
    }
    Ok(series)
}

fn sorted(values: &[f64]) -> Vec<f64> {
    let mut copy = values.to_vec();
    copy.sort_by(f64::total_cmp);
    copy
}

/// Nearest-rank percentile in per-mille (500 = p50, 950 = p95).
pub fn percentile(values: &[f64], permille: usize) -> f64 {
    assert!(!values.is_empty(), "percentile of an empty series");
    let ordered = sorted(values);
    let rank = (ordered.len() * permille)
        .div_ceil(1000)
        .saturating_sub(1)
        .min(ordered.len() - 1);
    ordered[rank]
}

pub fn median(values: &[f64]) -> f64 {
    percentile(values, 500)
}

pub fn ratios(numerator: &[f64], denominator: &[f64]) -> Vec<f64> {
    numerator
        .iter()
        .zip(denominator)
        .map(|(top, bottom)| top / bottom)
        .collect()
}

/// Deterministic xorshift generator for the bootstrap.
struct Xorshift {
    state: u64,
}

impl Xorshift {
    fn next_index(&mut self, bound: usize) -> usize {
        self.state ^= self.state << 13;
        self.state ^= self.state >> 7;
        self.state ^= self.state << 17;
        let bound = u64::try_from(bound).expect("bound");
        usize::try_from(self.state % bound).expect("index")
    }
}

/// 95% bootstrap confidence interval of the median of `ratios`.
pub fn bootstrap_median_ci(ratios: &[f64], resamples: usize) -> (f64, f64) {
    let mut generator = Xorshift {
        state: 0x2545_F491_4F6C_DD1D,
    };
    let mut medians = Vec::with_capacity(resamples);
    let mut draw = vec![0.0; ratios.len()];
    for _ in 0..resamples {
        for slot in &mut draw {
            *slot = ratios[generator.next_index(ratios.len())];
        }
        medians.push(median(&draw));
    }
    (percentile(&medians, 25), percentile(&medians, 975))
}

pub fn describe(values: &[f64]) -> Json {
    json!({"p50_s": percentile(values, 500), "p95_s": percentile(values, 950), "samples": values.len()})
}

/// Summarizes a paired series against the gate: the lower CI bound of the
/// heap/column ratio must exceed `1 + noise band`, where the band is the
/// largest excursion of the heap-versus-heap control CI from 1. Without a
/// control the band is unmeasured and treated as zero.
pub fn analyze(series: &Series, resamples: usize) -> Json {
    let paired = ratios(&series.heap, &series.column);
    let (lower, upper) = bootstrap_median_ci(&paired, resamples);
    let ratio_median = median(&paired);
    let control = (!series.control.is_empty()).then(|| {
        let control_ratios = ratios(&series.heap, &series.control);
        let (c_lower, c_upper) = bootstrap_median_ci(&control_ratios, resamples);
        (median(&control_ratios), c_lower, c_upper)
    });
    let band = control.map_or(0.0, |(_, c_lower, c_upper)| {
        (c_upper - 1.0).max(1.0 - c_lower).max(0.0)
    });
    let gate_pass = lower > 1.0 + band;
    json!({
        "heap": describe(&series.heap),
        "column": describe(&series.column),
        "control": (!series.control.is_empty()).then(|| describe(&series.control)),
        "paired_ratio_heap_over_column": {
            "median": ratio_median,
            "ci95_lower": lower,
            "ci95_upper": upper,
            "ratios": paired,
        },
        "control_ratio_heap_over_heap": control.map(|(m, l, u)| json!({"median": m, "ci95_lower": l, "ci95_upper": u})),
        "noise_band": band,
        "noise_band_measured": control.is_some(),
        "gate": "ci95_lower > 1 + noise_band",
        "gate_pass": gate_pass,
        "target_1_25x_median_met": ratio_median >= 1.25,
        "raw_heap_s": series.heap,
        "raw_column_s": series.column,
        "raw_control_s": series.control,
    })
}
