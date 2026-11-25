#![allow(dead_code)]
//! Graded-relevance metrics shared by the in-tree relevance tests and the opt-in Cranfield
//! runner (`benches/relevance_cranfield.rs`), so both score rankings with one implementation.

use std::collections::{BTreeMap, BTreeSet};

/// Exact conversion of a small count for metric arithmetic.
pub fn count_f64(count: usize) -> f64 {
    f64::from(u32::try_from(count).expect("count fits in u32"))
}

fn gain(grade: u8) -> f64 {
    f64::from(2_u32.pow(u32::from(grade))) - 1.0
}

fn discount(rank_from_one: usize) -> f64 {
    count_f64(rank_from_one + 1).log2().recip()
}

/// nDCG@k with gain `2^grade - 1`. Ungraded documents and repeated appearances
/// of a document earn zero gain. A duplicate still occupies its original rank.
/// The score is zero when nothing is graded relevant.
pub fn ndcg_at_k(ranked: &[String], grades: &BTreeMap<String, u8>, k: usize) -> f64 {
    let mut seen = BTreeSet::new();
    let dcg: f64 = ranked
        .iter()
        .take(k)
        .enumerate()
        .filter(|(_, id)| seen.insert(id.as_str()))
        .map(|(index, id)| gain(grades.get(id).copied().unwrap_or(0)) * discount(index + 1))
        .sum();
    let mut ideal: Vec<u8> = grades.values().copied().collect();
    ideal.sort_unstable_by(|left, right| right.cmp(left));
    let ideal_dcg: f64 = ideal
        .iter()
        .take(k)
        .enumerate()
        .map(|(index, grade)| gain(*grade) * discount(index + 1))
        .sum();
    if ideal_dcg == 0.0 {
        0.0
    } else {
        dcg / ideal_dcg
    }
}

/// Reciprocal rank of the first document graded `relevant_grade` or above within the top `k`.
pub fn mrr_at_k_with_threshold(
    ranked: &[String],
    grades: &BTreeMap<String, u8>,
    k: usize,
    relevant_grade: u8,
) -> f64 {
    ranked
        .iter()
        .take(k)
        .position(|id| grades.get(id).copied().unwrap_or(0) >= relevant_grade)
        .map_or(0.0, |index| 1.0 / count_f64(index + 1))
}
