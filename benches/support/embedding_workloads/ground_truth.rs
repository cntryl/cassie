//! Brute-force exact top-k ground truth, its tie policy, and recall@k.

use std::cmp::Ordering;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Metric {
    L2,
    Cosine,
}

impl Metric {
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::L2 => "l2",
            Self::Cosine => "cosine",
        }
    }

    /// SQL operator for this metric.
    #[must_use]
    pub fn operator(self) -> &'static str {
        match self {
            Self::L2 => "<->",
            Self::Cosine => "<=>",
        }
    }
}

/// Distance accumulated in f64 from the f32 components. Cosine is
/// `1 - dot / (|a||b|)` and `1` when either operand is zero.
#[must_use]
pub fn distance(metric: Metric, left: &[f32], right: &[f32]) -> f64 {
    match metric {
        Metric::L2 => left
            .iter()
            .zip(right)
            .map(|(a, b)| {
                let delta = f64::from(*a) - f64::from(*b);
                delta * delta
            })
            .sum::<f64>()
            .sqrt(),
        Metric::Cosine => {
            let (mut dot, mut left_norm, mut right_norm) = (0.0_f64, 0.0_f64, 0.0_f64);
            for (a, b) in left.iter().zip(right) {
                let (a, b) = (f64::from(*a), f64::from(*b));
                dot += a * b;
                left_norm += a * a;
                right_norm += b * b;
            }
            if left_norm == 0.0 || right_norm == 0.0 {
                1.0
            } else {
                1.0 - dot / (left_norm.sqrt() * right_norm.sqrt())
            }
        }
    }
}

/// Exact top-k for one query.
#[derive(Clone, Debug, PartialEq)]
pub struct Truth {
    /// Row ordinals, ascending by `(distance, ordinal)`.
    pub rows: Vec<usize>,
    /// Distance of the last truth row; rows tied with it also count as correct.
    pub boundary: f64,
}

/// Brute-force top-`k` over `rows` (row-major, `dimensions` per row), restricted
/// to `subset` when given. Order is ascending `(distance, row ordinal)`.
#[must_use]
pub fn exact_top_k(
    metric: Metric,
    rows: &[f32],
    dimensions: usize,
    query: &[f32],
    subset: Option<&[usize]>,
    k: usize,
) -> Truth {
    let row = |ordinal: usize| &rows[ordinal * dimensions..(ordinal + 1) * dimensions];
    let all: Vec<usize> = (0..rows.len() / dimensions).collect();
    let candidates = subset.unwrap_or(&all);
    let mut scored: Vec<(f64, usize)> = candidates
        .iter()
        .map(|&ordinal| (distance(metric, row(ordinal), query), ordinal))
        .collect();
    scored.sort_by(|left, right| {
        left.0
            .partial_cmp(&right.0)
            .unwrap_or(Ordering::Equal)
            .then(left.1.cmp(&right.1))
    });
    scored.truncate(k);
    Truth {
        boundary: scored.last().map_or(f64::INFINITY, |entry| entry.0),
        rows: scored.into_iter().map(|entry| entry.1).collect(),
    }
}

/// recall@k for one query: returned rows that are in the truth list, or whose
/// distance (from `distance_of`) equals the boundary, divided by the truth size.
/// An empty truth has recall 1.
pub fn recall_at_k(truth: &Truth, returned: &[usize], distance_of: impl Fn(usize) -> f64) -> f64 {
    if truth.rows.is_empty() {
        return 1.0;
    }
    let mut seen = std::collections::BTreeSet::new();
    let hits = returned
        .iter()
        .filter(|&&row| seen.insert(row))
        .filter(|&&row| {
            truth.rows.contains(&row)
                || distance_of(row).total_cmp(&truth.boundary) == Ordering::Equal
        })
        .count()
        .min(truth.rows.len());
    let hits = f64::from(u32::try_from(hits).unwrap_or(u32::MAX));
    hits / f64::from(u32::try_from(truth.rows.len()).unwrap_or(u32::MAX))
}

/// Text form of a truth list for the on-disk cache: one line per query,
/// `boundary row row ...`. `f64` Display round-trips exactly.
#[must_use]
pub fn encode_truths(truths: &[Truth]) -> String {
    use std::fmt::Write;
    truths.iter().fold(String::new(), |mut text, truth| {
        let rows: Vec<String> = truth.rows.iter().map(usize::to_string).collect();
        let _ = writeln!(text, "{} {}", truth.boundary, rows.join(" "));
        text
    })
}

/// Inverse of [`encode_truths`].
///
/// # Errors
///
/// Returns a message for a malformed line.
pub fn decode_truths(text: &str) -> Result<Vec<Truth>, String> {
    text.lines()
        .map(|line| {
            let mut parts = line.split_whitespace();
            let boundary = parts
                .next()
                .and_then(|part| part.parse::<f64>().ok())
                .ok_or_else(|| format!("bad truth line {line:?}"))?;
            let rows = parts
                .map(|part| part.parse::<usize>().map_err(|error| error.to_string()))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Truth { rows, boundary })
        })
        .collect()
}
