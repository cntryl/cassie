use std::sync::{Arc, Barrier};

use cassie::app::{Cassie, CassieSession};
use cassie::types::Value;

use super::support_read_equivalence::{sql, with_fixture};

#[derive(Clone, Copy, PartialEq)]
pub(super) enum Metric {
    L2,
    Cosine,
    Dot,
}

impl Metric {
    pub(super) fn name(self) -> &'static str {
        match self {
            Self::L2 => "l2",
            Self::Cosine => "cosine",
            Self::Dot => "dot",
        }
    }

    pub(super) fn operator(self) -> &'static str {
        match self {
            Self::L2 => "<->",
            Self::Cosine => "<=>",
            Self::Dot => "<#>",
        }
    }
}

pub(super) const ALGORITHMS: [(&str, &str); 2] = [
    (
        "hnsw",
        "index_type = hnsw, m = 16, ef_construction = 64, ef_search = 64",
    ),
    (
        "ivfflat",
        "index_type = ivfflat, lists = 4, probes = 4, training_sample_size = 30, training_seed = 7",
    ),
];

/// Deterministic non-unit vectors with varied magnitudes, plus two planted
/// rows: `aligned_far` is the best cosine match for `[1, 0.2, 0]` while
/// `near_off` is its Euclidean nearest neighbor.
pub(super) fn corpus() -> Vec<(String, [f32; 3])> {
    vec![
        ("axis".into(), [1.0, 0.0, 0.0]),
        ("opposite".into(), [-1.0, 0.0, 0.0]),
        ("orthogonal".into(), [0.0, 1.0, 0.0]),
        ("zero".into(), [0.0, 0.0, 0.0]),
        ("aligned_far".into(), [10.0, 2.0, 0.0]),
        ("near_off".into(), [0.9, 0.5, 0.1]),
    ]
}

fn same_vector(left: &[f32], right: &[f32]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .all(|(left, right)| left.to_bits() == right.to_bits())
}

pub(super) fn oracle(
    metric: Metric,
    query: &[f32],
    descending: bool,
    corpus: &[(String, [f32; 3])],
) -> Vec<(String, f64)> {
    if let Some(fixed) = pinned_oracle(metric, query, descending, corpus) {
        return fixed;
    }
    // Independently computed fallback: PostgreSQL-compatible Euclidean,
    // cosine, and inner-product conventions, never Cassie implementation calls.
    let mut ranked = corpus
        .iter()
        .map(|(id, vector)| {
            let dot = vector
                .iter()
                .zip(query)
                .map(|(left, right)| f64::from(*left) * f64::from(*right))
                .sum::<f64>();
            let distance = match metric {
                Metric::L2 => vector
                    .iter()
                    .zip(query)
                    .map(|(left, right)| {
                        let delta = f64::from(*left) - f64::from(*right);
                        delta * delta
                    })
                    .sum::<f64>()
                    .sqrt(),
                Metric::Cosine => {
                    let left_norm = vector
                        .iter()
                        .map(|value| f64::from(*value).powi(2))
                        .sum::<f64>()
                        .sqrt();
                    let right_norm = query
                        .iter()
                        .map(|value| f64::from(*value).powi(2))
                        .sum::<f64>()
                        .sqrt();
                    let denominator = left_norm * right_norm;
                    if denominator == 0.0 {
                        1.0
                    } else {
                        1.0 - dot / denominator
                    }
                }
                Metric::Dot => -dot,
            };
            (id.clone(), distance)
        })
        .collect::<Vec<_>>();
    ranked.sort_by(|left, right| {
        let order = left.1.total_cmp(&right.1);
        let order = if descending { order.reverse() } else { order };
        order.then_with(|| left.0.cmp(&right.0))
    });
    ranked
}

fn pinned_oracle(
    metric: Metric,
    query: &[f32],
    descending: bool,
    corpus: &[(String, [f32; 3])],
) -> Option<Vec<(String, f64)>> {
    if corpus.len() != 6
        || !corpus
            .iter()
            .any(|(id, vector)| id == "aligned_far" && same_vector(vector, &[10.0, 2.0, 0.0]))
        || !corpus
            .iter()
            .any(|(id, vector)| id == "near_off" && same_vector(vector, &[0.9, 0.5, 0.1]))
    {
        return None;
    }
    let rows = if same_vector(query, &QUERY) {
        Some(pinned_query_oracle(metric, descending))
    } else if same_vector(query, &[0.0, 0.0, 0.0]) {
        Some(pinned_zero_oracle(metric, descending))
    } else {
        None
    }?;
    rows.iter()
        .all(|(id, _)| corpus.iter().any(|(corpus_id, _)| corpus_id == id))
        .then(|| {
            rows.into_iter()
                .map(|(id, distance)| (id.to_string(), distance))
                .collect()
        })
}

fn pinned_query_oracle(metric: Metric, descending: bool) -> Vec<(&'static str, f64)> {
    match (metric, descending) {
        (Metric::L2, false) => vec![
            ("axis", 0.200_000_000_298_023_23),
            ("near_off", 0.331_662_483_977_696_9),
            ("zero", 1.019_803_903_303_028_6),
            ("orthogonal", 1.280_624_845_624_833_5),
            ("opposite", 2.009_975_124_520_722),
            ("aligned_far", 9.178_235_123_882_54),
        ],
        (Metric::L2, true) => vec![
            ("aligned_far", 9.178_235_123_882_54),
            ("opposite", 2.009_975_124_520_722),
            ("orthogonal", 1.280_624_845_624_833_5),
            ("zero", 1.019_803_903_303_028_6),
            ("near_off", 0.331_662_483_977_696_9),
            ("axis", 0.200_000_000_298_023_23),
        ],
        (Metric::Cosine, false) => vec![
            ("aligned_far", 0.0),
            ("axis", 0.019_419_324_871_071_786),
            ("near_off", 0.052_036_883_210_120_01),
            ("orthogonal", 0.803_883_862_051_856_2),
            ("zero", 1.0),
            ("opposite", 1.980_580_675_128_928),
        ],
        (Metric::Cosine, true) => vec![
            ("opposite", 1.980_580_675_128_928),
            ("zero", 1.0),
            ("orthogonal", 0.803_883_862_051_856_2),
            ("near_off", 0.052_036_883_210_120_01),
            ("axis", 0.019_419_324_871_071_786),
            ("aligned_far", 0.0),
        ],
        (Metric::Dot, false) => vec![
            ("aligned_far", -10.400_000_005_960_464),
            ("axis", -1.0),
            ("near_off", -0.999_999_977_648_258_2),
            ("orthogonal", -0.200_000_002_980_232_24),
            ("zero", -0.0),
            ("opposite", 1.0),
        ],
        (Metric::Dot, true) => vec![
            ("opposite", 1.0),
            ("zero", -0.0),
            ("orthogonal", -0.200_000_002_980_232_24),
            ("near_off", -0.999_999_977_648_258_2),
            ("axis", -1.0),
            ("aligned_far", -10.400_000_005_960_464),
        ],
    }
}

fn pinned_zero_oracle(metric: Metric, descending: bool) -> Vec<(&'static str, f64)> {
    match (metric, descending) {
        (Metric::L2, false) => vec![
            ("zero", 0.0),
            ("axis", 1.0),
            ("opposite", 1.0),
            ("orthogonal", 1.0),
            ("near_off", 1.034_408_022_679_000_4),
            ("aligned_far", 10.198_039_027_185_569),
        ],
        (Metric::L2, true) => vec![
            ("aligned_far", 10.198_039_027_185_569),
            ("near_off", 1.034_408_022_679_000_4),
            ("axis", 1.0),
            ("opposite", 1.0),
            ("orthogonal", 1.0),
            ("zero", 0.0),
        ],
        (Metric::Cosine, false | true) => vec![
            ("aligned_far", 1.0),
            ("axis", 1.0),
            ("near_off", 1.0),
            ("opposite", 1.0),
            ("orthogonal", 1.0),
            ("zero", 1.0),
        ],
        (Metric::Dot, false | true) => vec![
            ("aligned_far", -0.0),
            ("axis", -0.0),
            ("near_off", -0.0),
            ("opposite", -0.0),
            ("orthogonal", -0.0),
            ("zero", -0.0),
        ],
    }
}

pub(super) fn seed(
    cassie: &Cassie,
    session: &CassieSession,
    table: &str,
    metric: Metric,
    options: &str,
) {
    seed_rows(cassie, session, table, (metric, options), &corpus());
}

pub(super) fn seed_rows(
    cassie: &Cassie,
    session: &CassieSession,
    table: &str,
    (metric, options): (Metric, &str),
    rows: &[(String, [f32; 3])],
) {
    sql(
        cassie,
        session,
        &format!("CREATE TABLE {table} (body TEXT, label TEXT, embedding VECTOR(3))"),
    );
    let documents = rows
        .iter()
        .map(|(label, vector)| {
            (
                Some(label.clone()),
                serde_json::json!({"body": label, "label": label, "embedding": vector}),
            )
        })
        .collect();
    cassie
        .midge
        .put_fresh_documents(table, documents)
        .expect("seed corpus");
    sql(
        cassie,
        session,
        &format!(
            "CREATE INDEX {table}_idx ON {table} USING vector (embedding) \
             WITH (source_field = body, metric = {}, {options})",
            metric.name()
        ),
    );
}

fn query_text(query: &[f32]) -> String {
    format!("[{},{},{}]", query[0], query[1], query[2])
}

pub(super) fn ranking_sql(table: &str, metric: Metric, query: &[f32], tail: &str) -> String {
    let literal = query_text(query);
    format!(
        "SELECT label, embedding {op} '{literal}' AS distance FROM {table} \
         ORDER BY embedding {op} '{literal}' {tail}",
        op = metric.operator()
    )
}

pub(super) fn counter(cassie: &Cassie, name: &str) -> u64 {
    cassie.metrics()["vector"][name]
        .as_u64()
        .expect("vector counter")
}

pub(super) fn assert_matches_oracle(
    rows: &[Vec<Value>],
    expected: &[(String, f64)],
    context: &str,
) {
    assert_eq!(rows.len(), expected.len(), "{context}");
    for (row, (label, distance)) in rows.iter().zip(expected) {
        assert_eq!(row[0], Value::String(label.clone()), "{context}");
        let Value::Float64(actual) = row[1] else {
            panic!("{context}: distance must be Float64");
        };
        assert!(
            (actual - distance).abs() < 1e-6,
            "{context}: {actual} != {distance}"
        );
    }
}

pub(super) const QUERY: [f32; 3] = [1.0, 0.2, 0.0];

#[test]
fn should_serve_each_algorithm_metric_pair_from_a_matching_ann_index() {
    // Arrange
    for (algorithm, options) in ALGORITHMS {
        for metric in [Metric::L2, Metric::Cosine, Metric::Dot] {
            with_fixture("sql_ann_metric_matrix", |cassie, session| {
                let table = format!("ann_{algorithm}_{}", metric.name());
                seed(cassie, session, &table, metric, options);
                let executions = format!("{algorithm}_executions");
                let reranks = format!("{algorithm}_exact_reranks_total");
                let fallbacks = format!("{algorithm}_fallbacks");
                let before = (
                    counter(cassie, &executions),
                    counter(cassie, &reranks),
                    counter(cassie, &fallbacks),
                );
                let expected = oracle(metric, &QUERY, false, &corpus());

                // Act
                let result = sql(
                    cassie,
                    session,
                    &ranking_sql(&table, metric, &QUERY, "LIMIT 5"),
                );

                // Assert
                let context = format!("{algorithm} {}", metric.name());
                assert_matches_oracle(result.rows(), &expected[..5], &context);
                assert_eq!(counter(cassie, &executions) - before.0, 1, "{context}");
                assert!(counter(cassie, &reranks) > before.1, "{context}");
                assert_eq!(counter(cassie, &fallbacks), before.2, "{context}");
                assert_eq!(
                    cassie.metrics()["vector"]["last_index_kind"].as_str(),
                    Some(algorithm),
                    "{context}"
                );
            });
        }
    }
}

#[test]
fn should_rank_non_unit_cosine_match_ahead_of_euclidean_nearest_neighbor() {
    // Arrange
    for (algorithm, options) in ALGORITHMS {
        with_fixture("sql_ann_cosine_not_l2", |cassie, session| {
            seed(cassie, session, "cosine_not_l2", Metric::Cosine, options);
            let corpus = corpus();
            let cosine = oracle(Metric::Cosine, &QUERY, false, &corpus);
            let euclidean = oracle(Metric::L2, &QUERY, false, &corpus);

            // Act
            let result = sql(
                cassie,
                session,
                &ranking_sql("cosine_not_l2", Metric::Cosine, &QUERY, "LIMIT 3"),
            );

            // Assert
            assert_ne!(cosine[0].0, euclidean[0].0);
            assert_eq!(cosine[0].0, "aligned_far");
            assert_matches_oracle(result.rows(), &cosine[..3], algorithm);
            assert_eq!(counter(cassie, &format!("{algorithm}_executions")), 1);
        });
    }
}

#[test]
fn should_apply_offset_after_non_l2_ann_ranking() {
    // Arrange
    for (algorithm, options) in ALGORITHMS {
        for metric in [Metric::Cosine, Metric::Dot] {
            with_fixture("sql_ann_metric_offset", |cassie, session| {
                seed(cassie, session, "ann_offset", metric, options);
                let expected = oracle(metric, &QUERY, false, &corpus());

                // Act
                let result = sql(
                    cassie,
                    session,
                    &ranking_sql("ann_offset", metric, &QUERY, "LIMIT 4 OFFSET 3"),
                );

                // Assert
                let context = format!("{algorithm} {}", metric.name());
                assert_matches_oracle(result.rows(), &expected[3..], &context);
                assert_eq!(counter(cassie, &format!("{algorithm}_executions")), 1);
            });
        }
    }
}

#[test]
fn should_diagnose_exact_fallback_when_operator_and_index_metrics_differ() {
    // Arrange
    for (algorithm, options) in ALGORITHMS {
        for (index_metric, query_metric) in [
            (Metric::L2, Metric::Cosine),
            (Metric::Cosine, Metric::Dot),
            (Metric::Dot, Metric::L2),
        ] {
            with_fixture("sql_ann_metric_mismatch", |cassie, session| {
                seed(cassie, session, "ann_mismatch", index_metric, options);
                let expected = oracle(query_metric, &QUERY, false, &corpus());

                // Act
                let result = sql(
                    cassie,
                    session,
                    &ranking_sql("ann_mismatch", query_metric, &QUERY, "LIMIT 5"),
                );

                // Assert
                let context = format!(
                    "{algorithm} index {} query {}",
                    index_metric.name(),
                    query_metric.name()
                );
                assert_matches_oracle(result.rows(), &expected[..5], &context);
                assert_eq!(counter(cassie, &format!("{algorithm}_executions")), 0);
                assert_eq!(counter(cassie, &format!("{algorithm}_fallbacks")), 1);
                assert_eq!(
                    cassie.metrics()["vector"]["last_fallback_reason"].as_str(),
                    Some("incompatible-metric"),
                    "{context}"
                );
            });
        }
    }
}

#[test]
fn should_fall_back_to_exact_scoring_for_descending_non_l2_order() {
    // Arrange
    for (algorithm, options) in ALGORITHMS {
        for metric in [Metric::Cosine, Metric::Dot] {
            with_fixture("sql_ann_metric_descending", |cassie, session| {
                seed(cassie, session, "ann_desc", metric, options);
                let expected = oracle(metric, &QUERY, true, &corpus());

                // Act
                let result = sql(
                    cassie,
                    session,
                    &ranking_sql("ann_desc", metric, &QUERY, "DESC LIMIT 5"),
                );

                // Assert
                let context = format!("{algorithm} {}", metric.name());
                assert_matches_oracle(result.rows(), &expected[..5], &context);
                assert_eq!(counter(cassie, &format!("{algorithm}_executions")), 0);
                assert_eq!(
                    cassie.metrics()["vector"]["last_fallback_reason"].as_str(),
                    Some("unsupported-sort"),
                    "{context}"
                );
            });
        }
    }
}

#[test]
fn should_keep_identity_tie_order_for_zero_non_l2_queries() {
    // Arrange
    let zero = [0.0, 0.0, 0.0];
    for (algorithm, options) in ALGORITHMS {
        for metric in [Metric::Cosine, Metric::Dot] {
            with_fixture("sql_ann_metric_zero_query", |cassie, session| {
                seed(cassie, session, "ann_zero", metric, options);
                let expected = oracle(metric, &zero, false, &corpus());

                // Act
                let result = sql(
                    cassie,
                    session,
                    &ranking_sql("ann_zero", metric, &zero, "LIMIT 5"),
                );

                // Assert
                let context = format!("{algorithm} {}", metric.name());
                assert_matches_oracle(result.rows(), &expected[..5], &context);
                assert_eq!(counter(cassie, &format!("{algorithm}_executions")), 0);
                assert_eq!(
                    cassie.metrics()["vector"]["last_fallback_reason"].as_str(),
                    Some("zero-query-exact"),
                    "{context}"
                );
            });
        }
    }
}

#[test]
fn should_keep_cosine_ann_snapshot_when_a_row_is_deleted_before_rerank() {
    // Arrange
    let _hook_guard = cassie::midge::adapter::query_scan_control_test_guard();
    for (algorithm, options) in ALGORITHMS {
        with_fixture("sql_ann_metric_delete_race", |cassie, session| {
            seed(cassie, session, "ann_race", Metric::Cosine, options);
            let query = ranking_sql("ann_race", Metric::Cosine, &QUERY, "LIMIT 5");
            let selected = Arc::new(Barrier::new(2));
            let resume = Arc::new(Barrier::new(2));
            cassie::executor::set_vector_ann_rerank_barriers(
                cassie,
                Some(Arc::clone(&selected)),
                Some(Arc::clone(&resume)),
            );

            // Act
            let paused = std::thread::scope(|scope| {
                let reader = scope.spawn(|| {
                    cassie
                        .execute_sql(&cassie.create_session("reader", None), &query, vec![])
                        .expect("concurrent cosine query")
                });
                selected.wait();
                sql(
                    cassie,
                    &cassie.create_session("writer", None),
                    "DELETE FROM ann_race WHERE label = 'aligned_far'",
                );
                resume.wait();
                reader.join().expect("reader thread")
            });
            let reason = cassie.metrics()["vector"]["last_fallback_reason"]
                .as_str()
                .map(str::to_owned);
            let next = sql(cassie, session, &query);

            // Assert
            let captured = oracle(Metric::Cosine, &QUERY, false, &corpus());
            let remaining = corpus()
                .into_iter()
                .filter(|(label, _)| label != "aligned_far")
                .collect::<Vec<_>>();
            let current = oracle(Metric::Cosine, &QUERY, false, &remaining);
            assert_matches_oracle(paused.rows(), &captured[..5], algorithm);
            assert_matches_oracle(next.rows(), &current[..5], algorithm);
            assert_ne!(reason.as_deref(), Some("concurrent-source-change"));
            assert!(counter(cassie, &format!("{algorithm}_executions")) >= 1);
        });
    }
}
