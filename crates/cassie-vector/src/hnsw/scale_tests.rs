//! Full-build validity and recall over a few thousand deterministic points.

use super::{build_graph, graph_validity_violation, search_graph};
use crate::types::{DistanceMetric, HnswIndexOptions, NormalizedVectorRecord};

const DIMENSIONS: usize = 32;
const POINTS: usize = 3_000;
const QUERIES: usize = 40;
const TOP_K: usize = 10;

fn next_value(state: &mut u64) -> f32 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    let unit = u32::try_from(*state >> 40).unwrap_or(0);
    f32::from(u16::try_from(unit >> 8).unwrap_or(0)) / 32_768.0 - 1.0
}

fn points(count: usize, stream: u64) -> Vec<Vec<f32>> {
    let mut state = stream;
    (0..count)
        .map(|_| (0..DIMENSIONS).map(|_| next_value(&mut state)).collect())
        .collect()
}

fn record(id: String, values: &[f32], metric: DistanceMetric) -> NormalizedVectorRecord {
    let normalized = crate::normalize(values).expect("normalizable vector");
    NormalizedVectorRecord {
        built_generation: 0,
        collection: "docs".to_string(),
        field: "embedding".to_string(),
        id,
        dimensions: DIMENSIONS,
        metric,
        normalization_version: NormalizedVectorRecord::CURRENT_NORMALIZATION_VERSION,
        payload_available: true,
        magnitude: normalized.magnitude,
        values: normalized.values,
    }
}

fn recall_for(metric: DistanceMetric, distance: fn(&[f32], &[f32]) -> f64) -> f64 {
    let vectors = points(POINTS, 0x9e37_79b9_7f4a_7c15);
    let records = vectors
        .iter()
        .enumerate()
        .map(|(index, values)| record(format!("doc-{index:05}"), values, metric))
        .collect::<Vec<_>>();
    let options = HnswIndexOptions {
        version: 1,
        m: 16,
        ef_construction: 64,
        ef_search: 64,
    };
    let graph = build_graph(records, &options, DIMENSIONS, metric);
    assert_eq!(graph_validity_violation(&graph, options.m), None);
    assert_eq!(graph.row_count, vectors.len());

    let mut hits = 0u32;
    for query in &points(QUERIES, 0x1234_5678_9abc_def1) {
        let mut exact = vectors
            .iter()
            .enumerate()
            .map(|(index, values)| (distance(query, values), format!("doc-{index:05}")))
            .collect::<Vec<_>>();
        exact.sort_by(|left, right| {
            left.0
                .total_cmp(&right.0)
                .then_with(|| left.1.cmp(&right.1))
        });
        let truth = exact
            .iter()
            .take(TOP_K)
            .map(|entry| entry.1.as_str())
            .collect::<Vec<_>>();
        let found = search_graph(&graph, query, &options, TOP_K).expect("graph search");
        hits += u32::try_from(
            found
                .candidates
                .iter()
                .filter(|candidate| truth.contains(&candidate.id.as_str()))
                .count(),
        )
        .unwrap_or(0);
    }
    f64::from(hits) / f64::from(u32::try_from(QUERIES * TOP_K).unwrap_or(1))
}

#[test]
fn should_build_valid_high_recall_l2_graph_over_thousands_of_points() {
    // Arrange
    let metric = DistanceMetric::L2;

    // Act
    let recall = recall_for(metric, crate::l2_distance);

    // Assert
    assert!(recall >= 0.90, "l2 recall {recall}");
}

#[test]
fn should_build_valid_high_recall_cosine_graph_over_thousands_of_points() {
    // Arrange
    let metric = DistanceMetric::Cosine;

    // Act
    let recall = recall_for(metric, crate::cosine_distance);

    // Assert
    assert!(recall >= 0.90, "cosine recall {recall}");
}
