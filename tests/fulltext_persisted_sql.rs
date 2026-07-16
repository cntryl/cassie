use cassie::app::{Cassie, CassieError};
use cassie::config::CassieRuntimeConfig;
use cassie::runtime::QueryCancellationHandle;
use cassie::types::Value;
use std::sync::Arc;
use std::time::Duration;

#[path = "support/sql.rs"]
mod support;
use support::*;

#[test]
fn should_read_persisted_postings_before_fetching_candidate_rows() {
    // Arrange
    with_fallback();
    std::env::set_var("CASSIE_EXECUTION_RESULT_CACHE_ENABLED", "false");
    let path = data_dir("persisted_fulltext_sql");
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE persisted_search_docs (title TEXT, body TEXT)",
            vec![],
        )
        .expect("create search table");
    for (title, body) in [
        ("first", "alpha beta"),
        ("second", "bravo charlie"),
        ("third", "alpha alpha delta"),
    ] {
        cassie
            .execute_sql(
                &session,
                "INSERT INTO persisted_search_docs (title, body) VALUES ($1, $2)",
                vec![
                    Value::String(title.to_string()),
                    Value::String(body.to_string()),
                ],
            )
            .expect("insert search row");
    }
    cassie
        .execute_sql(
            &session,
            "CREATE INDEX persisted_search_body_idx ON persisted_search_docs USING fulltext (body)",
            vec![],
        )
        .expect("create fulltext index");
    let before = cassie.metrics();

    // Act
    let result = cassie
        .execute_sql(
            &session,
            "SELECT id, title, snippet(body, $1) AS excerpt, search_score(body, $1) AS score FROM persisted_search_docs WHERE search(body, $1) LIMIT 1",
            vec![Value::String("alpha".to_string())],
        )
        .expect("query persisted postings");
    let after = cassie.metrics();

    // Assert
    assert_eq!(result.rows.len(), 1);
    assert!(matches!(result.rows[0][0], Value::String(_)));
    assert!(
        matches!(&result.rows[0][2], Value::String(excerpt) if excerpt.contains("<mark>alpha</mark>"))
    );
    assert!(matches!(result.rows[0][3], Value::Float64(score) if score > 0.0));
    assert!(
        after["search"]["posting_reads_total"].as_u64().unwrap()
            > before["search"]["posting_reads_total"].as_u64().unwrap()
    );
    assert_eq!(
        after["search"]["candidate_row_fetches_total"]
            .as_u64()
            .unwrap()
            - before["search"]["candidate_row_fetches_total"]
                .as_u64()
                .unwrap(),
        1
    );
    assert_eq!(
        after["search"]["row_scan_fallback_total"].as_u64(),
        before["search"]["row_scan_fallback_total"].as_u64()
    );
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_match_row_baseline_scores_from_persisted_postings() {
    // Arrange
    with_fallback();
    std::env::set_var("CASSIE_EXECUTION_RESULT_CACHE_ENABLED", "false");
    let path = data_dir("persisted_fulltext_scores");
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE persisted_score_docs (body TEXT)",
            vec![],
        )
        .expect("create score table");
    for body in ["alpha beta", "alpha alpha alpha beta", "beta gamma"] {
        cassie
            .execute_sql(
                &session,
                "INSERT INTO persisted_score_docs (body) VALUES ($1)",
                vec![Value::String(body.to_string())],
            )
            .expect("insert score row");
    }
    let sql = "SELECT id, search_score(body, $1) AS score FROM persisted_score_docs WHERE search(body, $1) ORDER BY score DESC LIMIT 2";
    let query_params = || vec![Value::String("alpha beta".to_string())];
    let baseline = cassie
        .execute_sql(&session, sql, query_params())
        .expect("execute row baseline");
    cassie
        .execute_sql(
            &session,
            "CREATE INDEX persisted_score_body_idx ON persisted_score_docs USING fulltext (body)",
            vec![],
        )
        .expect("create score index");
    let before = cassie.metrics();

    // Act
    let persisted = cassie
        .execute_sql(&session, sql, query_params())
        .expect("execute persisted score query");
    let after = cassie.metrics();

    // Assert
    assert_eq!(persisted.rows, baseline.rows);
    assert!(
        after["search"]["posting_reads_total"].as_u64().unwrap()
            > before["search"]["posting_reads_total"].as_u64().unwrap()
    );
    assert_eq!(
        after["search"]["candidate_row_fetches_total"].as_u64(),
        before["search"]["candidate_row_fetches_total"].as_u64()
    );
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_apply_structured_predicates_only_to_posting_candidates() {
    // Arrange
    with_fallback();
    std::env::set_var("CASSIE_EXECUTION_RESULT_CACHE_ENABLED", "false");
    let path = data_dir("persisted_fulltext_structured_filter");
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE persisted_filter_docs (category TEXT, body TEXT)",
            vec![],
        )
        .expect("create filtered table");
    for (category, body) in [
        ("keep", "alpha beta"),
        ("drop", "alpha gamma"),
        ("keep", "bravo delta"),
    ] {
        cassie
            .execute_sql(
                &session,
                "INSERT INTO persisted_filter_docs (category, body) VALUES ($1, $2)",
                vec![
                    Value::String(category.to_string()),
                    Value::String(body.to_string()),
                ],
            )
            .expect("insert filtered row");
    }
    cassie
        .execute_sql(
            &session,
            "CREATE INDEX persisted_filter_body_idx ON persisted_filter_docs USING fulltext (body)",
            vec![],
        )
        .expect("create filtered index");
    cassie
        .execute_sql(
            &session,
            "CREATE INDEX persisted_filter_category_idx ON persisted_filter_docs (category)",
            vec![],
        )
        .expect("create category index");
    let before = cassie.metrics();

    // Act
    let result = cassie
        .execute_sql(
            &session,
            "SELECT id, category, search_score(body, $1) AS score FROM persisted_filter_docs WHERE search(body, $1) AND category = $2",
            vec![
                Value::String("alpha".to_string()),
                Value::String("keep".to_string()),
            ],
        )
        .expect("query filtered postings");
    let after = cassie.metrics();

    // Assert
    assert_eq!(result.rows.len(), 1);
    assert_eq!(result.rows[0][1], Value::String("keep".to_string()));
    assert_eq!(
        after["search"]["candidate_row_fetches_total"]
            .as_u64()
            .unwrap()
            - before["search"]["candidate_row_fetches_total"]
                .as_u64()
                .unwrap(),
        1
    );
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_overlay_transaction_mutations_on_fulltext_fallback() {
    // Arrange
    with_fallback();
    std::env::set_var("CASSIE_EXECUTION_RESULT_CACHE_ENABLED", "false");
    let path = data_dir("persisted_fulltext_transaction_overlay");
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE persisted_tx_docs (title TEXT, body TEXT)",
            vec![],
        )
        .expect("create transaction table");
    for title in ["keep", "change", "remove"] {
        cassie
            .execute_sql(
                &session,
                "INSERT INTO persisted_tx_docs (title, body) VALUES ($1, $2)",
                vec![
                    Value::String(title.to_string()),
                    Value::String("alpha".to_string()),
                ],
            )
            .expect("insert transaction row");
    }
    cassie
        .execute_sql(
            &session,
            "CREATE INDEX persisted_tx_body_idx ON persisted_tx_docs USING fulltext (body)",
            vec![],
        )
        .expect("create transaction index");
    cassie
        .execute_sql(&session, "BEGIN", vec![])
        .expect("begin transaction");
    cassie
        .execute_sql(
            &session,
            "UPDATE persisted_tx_docs SET body = $1 WHERE title = $2",
            vec![
                Value::String("bravo".to_string()),
                Value::String("change".to_string()),
            ],
        )
        .expect("update transaction row");
    cassie
        .execute_sql(
            &session,
            "DELETE FROM persisted_tx_docs WHERE title = $1",
            vec![Value::String("remove".to_string())],
        )
        .expect("delete transaction row");
    cassie
        .execute_sql(
            &session,
            "INSERT INTO persisted_tx_docs (title, body) VALUES ($1, $2)",
            vec![
                Value::String("new".to_string()),
                Value::String("alpha".to_string()),
            ],
        )
        .expect("insert transaction overlay");
    let before = cassie.metrics();

    // Act
    let result = cassie
        .execute_sql(
            &session,
            "SELECT id, title, search_score(body, $1) AS score FROM persisted_tx_docs WHERE search(body, $1)",
            vec![Value::String("alpha".to_string())],
        )
        .expect("query transaction overlay");
    let after = cassie.metrics();

    // Assert
    let mut titles = result
        .rows
        .iter()
        .filter_map(|row| row.get(1).and_then(Value::as_str))
        .collect::<Vec<_>>();
    titles.sort_unstable();
    assert_eq!(titles, vec!["keep", "new"]);
    assert!(
        after["search"]["row_scan_fallback_total"].as_u64().unwrap()
            > before["search"]["row_scan_fallback_total"]
                .as_u64()
                .unwrap()
    );
    cassie
        .execute_sql(&session, "ROLLBACK", vec![])
        .expect("rollback transaction");
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_enforce_memory_budget_during_persisted_fulltext_scoring() {
    // Arrange
    with_fallback();
    std::env::set_var("CASSIE_EXECUTION_RESULT_CACHE_ENABLED", "false");
    let path = data_dir("persisted_fulltext_memory_budget");
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.limits.query_memory_budget_bytes = 512;
    let cassie = Cassie::new_with_data_dir_and_config(&path, config).expect("create Cassie");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE persisted_memory_docs (body TEXT)",
            vec![],
        )
        .expect("create memory table");
    for index in 0..20 {
        cassie
            .execute_sql(
                &session,
                "INSERT INTO persisted_memory_docs (body) VALUES ($1)",
                vec![Value::String(format!(
                    "alpha fixture token number {index} with bounded candidate statistics"
                ))],
            )
            .expect("insert memory row");
    }
    cassie
        .execute_sql(
            &session,
            "CREATE INDEX persisted_memory_body_idx ON persisted_memory_docs USING fulltext (body)",
            vec![],
        )
        .expect("create memory index");

    // Act
    let error = cassie
        .execute_sql(
            &session,
            "SELECT id, search_score(body, $1) AS score FROM persisted_memory_docs WHERE search(body, $1) ORDER BY score DESC LIMIT 5",
            vec![Value::String("alpha".to_string())],
        )
        .expect_err("candidate statistics should exceed memory budget");

    // Assert
    assert!(matches!(error, CassieError::ResourceLimit(_)));
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_cancel_persisted_fulltext_scoring_at_a_candidate_boundary() {
    // Arrange
    with_fallback();
    std::env::set_var("CASSIE_EXECUTION_RESULT_CACHE_ENABLED", "false");
    let path = data_dir("persisted_fulltext_cancellation");
    let cassie = Arc::new(Cassie::new_with_data_dir(&path).expect("create Cassie"));
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE persisted_cancel_docs (body TEXT)",
            vec![],
        )
        .expect("create cancellation table");
    let rows = (0..50_000)
        .map(|index| {
            (
                Some(format!("cancel-{index:05}")),
                serde_json::json!({"body": format!("alpha beta candidate {index:05}")}),
            )
        })
        .collect();
    cassie
        .midge
        .put_fresh_documents("persisted_cancel_docs", rows)
        .expect("seed cancellation rows");
    cassie
        .execute_sql(
            &session,
            "CREATE INDEX persisted_cancel_body_idx ON persisted_cancel_docs USING fulltext (body)",
            vec![],
        )
        .expect("create cancellation index");
    let cancellation = QueryCancellationHandle::new();
    let query_cancellation = cancellation.clone();
    let query_cassie = Arc::clone(&cassie);
    let query = std::thread::spawn(move || {
        let session = query_cassie.create_session("tester", None);
        query_cassie.execute_sql_with_cancellation(
            &session,
            "SELECT id, search_score(body, $1) AS score FROM persisted_cancel_docs WHERE search(body, $1) ORDER BY score DESC LIMIT 20",
            vec![Value::String("alpha beta".to_string())],
            &query_cancellation,
        )
    });
    std::thread::sleep(Duration::from_millis(10));

    // Act
    cancellation.cancel();
    let error = query
        .join()
        .expect("query thread")
        .expect_err("persisted scoring should observe cancellation");

    // Assert
    assert!(matches!(error, CassieError::QueryCancelled));
    drop(cassie);
    let _ = std::fs::remove_dir_all(path);
}
