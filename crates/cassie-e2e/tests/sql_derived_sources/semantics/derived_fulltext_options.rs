use cassie::types::Value;

use crate::support_sql_fixture::sql_fixture;

#[test]
fn should_apply_fulltext_index_options_through_derived_sources() {
    // Arrange
    let fixture = sql_fixture(
        "derived_fulltext",
        &[
            "CREATE TABLE docs (title TEXT, body TEXT)",
            "CREATE INDEX idx_docs_body ON docs USING fulltext (body) \
                 WITH (stop_words = none, boost = 3.0)",
            "INSERT INTO docs (title, body) VALUES ('a', 'the quick brown fox')",
        ],
    );
    let score = |sql: &str| match fixture.rows(sql).first().and_then(|row| row.first()) {
        Some(Value::Float64(value)) => *value,
        other => panic!("expected a score, got {other:?}"),
    };

    // Act
    let base = fixture.rows("SELECT title FROM docs WHERE search(body, 'the')");
    let derived =
        fixture.rows("SELECT title FROM (SELECT * FROM docs) d WHERE search(body, 'the')");
    let cte = fixture
        .rows("WITH c AS (SELECT * FROM docs) SELECT title FROM c WHERE search(body, 'the')");
    let base_score = score("SELECT search_score(body, 'quick') FROM docs");
    let derived_score = score("SELECT search_score(body, 'quick') FROM (SELECT * FROM docs) d");

    // Assert
    assert_eq!(base.len(), 1);
    assert_eq!(derived, base);
    assert_eq!(cte, base);
    assert!(
        (base_score - derived_score).abs() < 1e-9,
        "scores must match"
    );
}
