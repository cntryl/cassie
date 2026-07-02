use cassie::types::Value;

#[path = "support/executor.rs"]
mod support;
use support::{cassie_temp, create_text_collection, put_document, put_fulltext_index};

fn assert_f64_close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() <= f64::EPSILON,
        "expected {actual} to equal {expected}"
    );
}

#[test]
fn should_apply_fulltext_index_params_during_search_score() {
    // Arrange
    let cassie = cassie_temp("fulltext_k1_b");
    let collection = "exec_fulltext_k1_b";
    create_text_collection(&cassie, collection, &["id", "body"]);
    put_document(
        &cassie,
        collection,
        "d1",
        serde_json::json!({"body": "alpha alpha alpha"}),
    );
    put_document(
        &cassie,
        collection,
        "d2",
        serde_json::json!({"body": "bravo"}),
    );

    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE INDEX idx_exec_fulltext_k1_b ON exec_fulltext_k1_b USING fulltext (body) WITH (k1 = 0, b = 0)",
            vec![],
        )
        .unwrap();

    // Act
    let result = cassie
        .execute_sql(
            &session,
            "SELECT search_score(body, 'alpha') AS score FROM exec_fulltext_k1_b WHERE id = 'd1'",
            vec![],
        )
        .expect("query should execute");

    // Assert
    let expected = cassie::search::bm25::bm25_score(3.0, 1.0, 2.0, 0.0, 0.0, 3.0, 2.0);
    assert_eq!(result.columns.len(), 1);
    assert_eq!(result.columns[0].name, "score");
    assert_eq!(result.rows.len(), 1);
    assert_eq!(result.rows[0].len(), 1);
    match &result.rows[0][0] {
        Value::Float64(score) => assert_f64_close(*score, expected),
        _ => panic!("expected float score"),
    }
}

#[test]
fn should_apply_fulltext_analyzer_stop_words_during_search_score() {
    // Arrange
    let cassie = cassie_temp("fulltext_analyzer_stop_words");
    let collection = "exec_fulltext_analyzer_stop_words";
    create_text_collection(&cassie, collection, &["id", "body"]);
    put_document(
        &cassie,
        collection,
        "d1",
        serde_json::json!({"body": "the the alpha"}),
    );

    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE INDEX idx_exec_fulltext_analyzer_stop_words ON exec_fulltext_analyzer_stop_words USING fulltext (body) WITH (analyzer = standard, stop_words = none)",
            vec![],
        )
        .unwrap();

    // Act
    let result = cassie
        .execute_sql(
            &session,
            "SELECT search_score(body, 'the') AS score FROM exec_fulltext_analyzer_stop_words",
            vec![],
        )
        .expect("query should execute");

    // Assert
    match &result.rows[0][0] {
        Value::Float64(score) => assert!(*score > 0.0),
        _ => panic!("expected float score"),
    }
}

#[test]
fn should_reject_non_finite_fulltext_index_options_during_search_score() {
    // Arrange
    let cassie = cassie_temp("fulltext_non_finite");
    let collection = "exec_fulltext_non_finite";
    create_text_collection(&cassie, collection, &["id", "body"]);
    put_document(
        &cassie,
        collection,
        "d1",
        serde_json::json!({"body": "alpha alpha alpha"}),
    );
    put_fulltext_index(
        &cassie,
        collection,
        "idx_exec_fulltext_non_finite",
        "body",
        &[("boost", "1.0"), ("k1", "1e999"), ("b", "0.75")],
    );
    cassie.hydrate_catalog().unwrap();

    // Act
    let session = cassie.create_session("tester", None);
    let result = cassie.execute_sql(
        &session,
        "SELECT search_score(body, 'alpha') AS score FROM exec_fulltext_non_finite WHERE id = 'd1'",
        vec![],
    );

    // Assert
    assert!(result.is_err());
}

#[test]
fn should_reject_duplicate_fulltext_indexes_during_search_score() {
    // Arrange
    let cassie = cassie_temp("fulltext_duplicate");
    let collection = "exec_fulltext_duplicate";
    create_text_collection(&cassie, collection, &["id", "body"]);
    put_document(
        &cassie,
        collection,
        "d1",
        serde_json::json!({"body": "alpha alpha alpha"}),
    );
    put_fulltext_index(
        &cassie,
        collection,
        "idx_exec_fulltext_duplicate_a",
        "body",
        &[("boost", "1.0"), ("k1", "1.2"), ("b", "0.75")],
    );
    put_fulltext_index(
        &cassie,
        collection,
        "idx_exec_fulltext_duplicate_b",
        "body",
        &[("boost", "2.0"), ("k1", "0.5"), ("b", "0.4")],
    );
    cassie.hydrate_catalog().unwrap();

    // Act
    let session = cassie.create_session("tester", None);
    let result = cassie.execute_sql(
        &session,
        "SELECT search_score(body, 'alpha') AS score FROM exec_fulltext_duplicate WHERE id = 'd1'",
        vec![],
    );

    // Assert
    assert!(result.is_err());
}

#[test]
fn should_allow_plain_select_with_non_finite_fulltext_metadata() {
    // Arrange
    let cassie = cassie_temp("plain_select_bad_fulltext");
    let collection = "exec_plain_select_bad_fulltext";
    create_text_collection(&cassie, collection, &["id", "body"]);
    put_document(
        &cassie,
        collection,
        "d1",
        serde_json::json!({"body": "alpha beta"}),
    );
    put_fulltext_index(
        &cassie,
        collection,
        "idx_exec_plain_select_bad_fulltext",
        "body",
        &[("boost", "1.0"), ("k1", "inf"), ("b", "0.75")],
    );
    cassie.hydrate_catalog().unwrap();

    // Act
    let session = cassie.create_session("tester", None);
    let result = cassie
        .execute_sql(
            &session,
            "SELECT id FROM exec_plain_select_bad_fulltext WHERE id = 'd1'",
            vec![],
        )
        .expect("plain select should execute");

    // Assert
    assert_eq!(result.columns.len(), 1);
    assert_eq!(result.rows.len(), 1);
    assert_eq!(result.rows[0][0], Value::String("d1".to_string()));
}

#[test]
fn should_project_snippet_function_output_for_text_matches() {
    // Arrange
    let cassie = cassie_temp("snippet_output");
    let collection = "exec_snippet_output";
    create_text_collection(&cassie, collection, &["title", "body"]);
    put_document(
        &cassie,
        collection,
        "d1",
        serde_json::json!({"title": "alpha", "body": "Rust enables fast query search"}),
    );

    // Act
    let session = cassie.create_session("tester", None);
    let result = cassie
        .execute_sql(
            &session,
            "SELECT snippet(body, 'query') AS excerpt FROM exec_snippet_output WHERE title = 'alpha'",
            vec![],
        )
        .expect("snippet query should execute");

    // Assert
    assert_eq!(result.columns[0].name, "excerpt");
    assert_eq!(result.rows.len(), 1);
    assert_eq!(result.rows[0].len(), 1);
    match &result.rows[0][0] {
        Value::String(excerpt) => {
            assert_eq!(excerpt, "Rust enables fast <mark>query</mark> search");
        }
        _ => panic!("expected string snippet output"),
    }
}
