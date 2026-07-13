use cassie::app::Cassie;
use cassie::midge::adapter::set_fulltext_maintenance_failure_point;

#[path = "support/executor.rs"]
mod support;
use support::{
    cassie_temp, create_text_collection, data_dir, put_document, put_fulltext_index, with_fallback,
};

#[test]
fn should_replay_fulltext_publication_debt_after_restart() {
    // Arrange
    with_fallback();
    let path = data_dir("fulltext_publication_recovery");
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let collection = "fulltext_publication_recovery";
    create_text_collection(&cassie, collection, &["id", "body"]);
    put_document(
        &cassie,
        collection,
        "d1",
        serde_json::json!({"body": "alpha"}),
    );
    put_fulltext_index(&cassie, collection, "body_idx", "body", &[]);

    // Act
    set_fulltext_maintenance_failure_point(true);
    put_document(
        &cassie,
        collection,
        "d2",
        serde_json::json!({"body": "beta"}),
    );
    assert!(cassie
        .midge
        .has_fulltext_maintenance_debt(collection, "body_idx")
        .expect("read fulltext debt"));
    drop(cassie);
    let restarted = Cassie::new_with_data_dir(&path).expect("reopen Cassie");
    restarted.startup().expect("replay fulltext debt");

    // Assert
    assert!(!restarted
        .midge
        .has_fulltext_maintenance_debt(collection, "body_idx")
        .expect("read recovered fulltext debt"));
    let state = restarted
        .midge
        .get_persisted_fulltext_index_state(collection, "body_idx")
        .expect("read rebuilt fulltext state")
        .expect("state exists");
    assert!(state.postings.contains_key("beta"));

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_report_fulltext_retrieval_stage_metrics() {
    // Arrange
    let cassie = cassie_temp("fulltext_retrieval_metrics");
    let collection = "fulltext_retrieval_metrics";
    create_text_collection(&cassie, collection, &["id", "body"]);
    put_document(
        &cassie,
        collection,
        "d1",
        serde_json::json!({"body": "alpha"}),
    );
    put_fulltext_index(&cassie, collection, "body_idx", "body", &[]);
    let session = cassie.create_session("tester", None);
    let before = cassie.metrics();

    // Act
    cassie
        .execute_sql(
            &session,
            "SELECT id FROM fulltext_retrieval_metrics WHERE search(body, 'alpha')",
            vec![],
        )
        .expect("search");
    let after = cassie.metrics();

    // Assert
    assert!(
        after["search"]["retrieval_stage_queries_total"]
            .as_u64()
            .unwrap_or_default()
            > before["search"]["retrieval_stage_queries_total"]
                .as_u64()
                .unwrap_or_default()
    );
    assert!(
        after["search"]["row_scan_fallback_total"]
            .as_u64()
            .unwrap_or_default()
            > before["search"]["row_scan_fallback_total"]
                .as_u64()
                .unwrap_or_default()
    );
}
