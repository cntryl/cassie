use std::sync::{Arc, Barrier};

use cassie::app::Cassie;
use cassie::embeddings::VectorIndexState;
use cassie::types::Value;

use super::sql_ann_metrics::{
    assert_matches_oracle, corpus, counter, oracle, ranking_sql, seed, Metric, ALGORITHMS, QUERY,
};
use super::support_read_equivalence::{sql, with_fixture};

fn explain_plan(cassie: &Cassie, session: &cassie::app::CassieSession, query: &str) -> String {
    let result = sql(cassie, session, &format!("EXPLAIN {query}"));
    let Value::String(plan) = &result.rows()[0][0] else {
        panic!("EXPLAIN must return a textual plan");
    };
    plan.clone()
}

fn clear_manifest(cassie: &Cassie, table: &str) {
    let record = cassie
        .midge
        .get_vector_index(table, "embedding")
        .expect("read index")
        .expect("index record");
    cassie
        .midge
        .put_vector_index_state(
            table,
            "embedding",
            VectorIndexState {
                built_generation: 0,
                hnsw_graph: None,
                ivfflat_training: None,
            },
        )
        .expect("invalidate manifest");
    drop(record);
}

#[test]
fn should_report_selected_index_plus_metric_in_explain() {
    // Arrange
    for (algorithm, options) in ALGORITHMS {
        for metric in [Metric::L2, Metric::Cosine, Metric::Dot] {
            with_fixture("sql_ann_metric_explain", |cassie, session| {
                seed(cassie, session, "ann_explain", metric, options);

                // Act
                let plan = explain_plan(
                    cassie,
                    session,
                    &ranking_sql("ann_explain", metric, &QUERY, "LIMIT 5"),
                );

                // Assert
                let expected = format!(
                    "vector_ann=eligible vector_ann_index={algorithm} vector_ann_index_metric={0} vector_ann_query_metric={0}",
                    metric.name()
                );
                assert!(plan.contains(&expected), "{algorithm} {}", metric.name());
            });
        }
    }
}

#[test]
fn should_explain_why_a_mismatched_or_descending_query_stays_exact() {
    // Arrange
    for (algorithm, options) in ALGORITHMS {
        with_fixture("sql_ann_metric_explain_fallback", |cassie, session| {
            seed(cassie, session, "ann_explain_fb", Metric::Cosine, options);

            // Act
            let mismatched = explain_plan(
                cassie,
                session,
                &ranking_sql("ann_explain_fb", Metric::Dot, &QUERY, "LIMIT 5"),
            );
            let descending = explain_plan(
                cassie,
                session,
                &ranking_sql("ann_explain_fb", Metric::Cosine, &QUERY, "DESC LIMIT 5"),
            );

            // Assert
            assert!(
                mismatched.contains("vector_ann=incompatible-metric"),
                "{algorithm}"
            );
            assert!(
                mismatched.contains("vector_ann_query_metric=dot"),
                "{algorithm}"
            );
            assert!(
                descending.contains("vector_ann=unsupported-sort"),
                "{algorithm}"
            );
        });
    }
}

#[test]
fn should_fall_back_completely_when_the_index_manifest_is_invalidated() {
    // Arrange
    for (algorithm, options) in ALGORITHMS {
        for metric in [Metric::Cosine, Metric::Dot] {
            with_fixture("sql_ann_metric_invalid_manifest", |cassie, session| {
                seed(cassie, session, "ann_manifest", metric, options);
                clear_manifest(cassie, "ann_manifest");
                let expected = oracle(metric, &QUERY, false, &corpus());

                // Act
                let result = sql(
                    cassie,
                    session,
                    &ranking_sql("ann_manifest", metric, &QUERY, "LIMIT 5"),
                );

                // Assert
                let context = format!("{algorithm} {}", metric.name());
                assert_matches_oracle(result.rows(), &expected[..5], &context);
                assert_eq!(counter(cassie, &format!("{algorithm}_executions")), 0);
                assert_eq!(counter(cassie, &format!("{algorithm}_fallbacks")), 1);
                let reason = if algorithm == "hnsw" {
                    "missing-graph"
                } else {
                    "missing-training"
                };
                assert_eq!(
                    cassie.metrics()["vector"]["last_fallback_reason"].as_str(),
                    Some(reason),
                    "{context}"
                );
            });
        }
    }
}

#[test]
fn should_keep_snapshot_when_a_vector_is_replaced_before_rerank() {
    // Arrange
    let _hook_guard = cassie::midge::adapter::query_scan_control_test_guard();
    for (algorithm, options) in ALGORITHMS {
        for metric in [Metric::Cosine, Metric::Dot] {
            with_fixture("sql_ann_metric_replace_race", |cassie, session| {
                seed(cassie, session, "ann_replace", metric, options);
                let query = ranking_sql("ann_replace", metric, &QUERY, "LIMIT 5");
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
                            .expect("concurrent ANN query")
                    });
                    selected.wait();
                    cassie
                        .midge
                        .put_document(
                            "ann_replace",
                            Some("aligned_far".into()),
                            serde_json::json!({
                                "body": "aligned_far",
                                "label": "aligned_far",
                                "embedding": [-10.0, -2.0, 0.0]
                            }),
                        )
                        .expect("replace vector with the same row count");
                    resume.wait();
                    reader.join().expect("reader thread")
                });
                let reason = cassie.metrics()["vector"]["last_fallback_reason"]
                    .as_str()
                    .map(str::to_owned);
                let next = sql(cassie, session, &query);

                // Assert
                let captured = oracle(metric, &QUERY, false, &corpus());
                let replaced = corpus()
                    .into_iter()
                    .map(|(label, vector)| {
                        if label == "aligned_far" {
                            (label, [-10.0, -2.0, 0.0])
                        } else {
                            (label, vector)
                        }
                    })
                    .collect::<Vec<_>>();
                let current = oracle(metric, &QUERY, false, &replaced);
                let context = format!("{algorithm} {}", metric.name());
                assert_matches_oracle(paused.rows(), &captured[..5], &context);
                assert_matches_oracle(next.rows(), &current[..5], &context);
                assert_ne!(reason.as_deref(), Some("concurrent-source-change"));
            });
        }
    }
}

#[test]
fn should_return_a_full_window_when_candidates_are_exhausted() {
    // Arrange
    let window = corpus().len() - 1;
    for (algorithm, options) in [
        ("hnsw", "index_type = hnsw, m = 16, ef_construction = 64, ef_search = 64"),
        (
            "ivfflat",
            "index_type = ivfflat, lists = 4, probes = 1, training_sample_size = 30, training_seed = 7",
        ),
    ] {
        for metric in [Metric::Cosine, Metric::Dot] {
            with_fixture("sql_ann_metric_exhaustion", |cassie, session| {
                seed(cassie, session, "ann_exhaust", metric, options);
                let expected = oracle(metric, &QUERY, false, &corpus());

                // Act
                let result = sql(
                    cassie,
                    session,
                    &ranking_sql(
                        "ann_exhaust",
                        metric,
                        &QUERY,
                        &format!("LIMIT {window}"),
                    ),
                );

                // Assert
                let context = format!("{algorithm} {}", metric.name());
                assert_matches_oracle(result.rows(), &expected[..window], &context);
                if algorithm == "ivfflat" {
                    assert_eq!(counter(cassie, "ivfflat_executions"), 0, "{context}");
                    assert_eq!(
                        cassie.metrics()["vector"]["last_fallback_reason"].as_str(),
                        Some("candidate-exhausted"),
                        "{context}"
                    );
                }
            });
        }
    }
}
