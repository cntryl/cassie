use cassie::types::Value;

use super::support_sql as support;

#[test]
fn should_preserve_fulltext_search_results_after_schema_qualified_drop() {
    // Arrange
    let cassie = support::new_in_memory_cassie().expect("create Cassie");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(&session, "CREATE SCHEMA index_drop_fulltext", vec![])
        .expect("create the index schema");
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE index_drop_fulltext.docs (id INT PRIMARY KEY, body TEXT)",
            vec![],
        )
        .expect("create the full-text table");
    cassie
        .execute_sql(
            &session,
            "INSERT INTO index_drop_fulltext.docs VALUES \
             (1, 'alpha beta'), (2, 'bravo'), (3, 'alpha delta')",
            vec![],
        )
        .expect("insert search rows");
    cassie
        .execute_sql(
            &session,
            "CREATE INDEX docs_body_idx ON index_drop_fulltext.docs USING fulltext (body)",
            vec![],
        )
        .expect("create the Cassie full-text index");
    let before = cassie
        .execute_sql(
            &session,
            "SELECT id FROM index_drop_fulltext.docs \
             WHERE search(body, 'alpha') ORDER BY id",
            vec![],
        )
        .expect("search through the full-text index");

    // Act
    cassie
        .execute_sql(
            &session,
            "DROP INDEX index_drop_fulltext.docs_body_idx",
            vec![],
        )
        .expect("drop a schema-qualified index without an ON clause");
    let after = cassie
        .execute_sql(
            &session,
            "SELECT id FROM index_drop_fulltext.docs \
             WHERE search(body, 'alpha') ORDER BY id",
            vec![],
        )
        .expect("search after the index is dropped");
    let index_count = cassie
        .execute_sql(
            &session,
            "SELECT count(*) FROM pg_catalog.pg_indexes \
             WHERE schemaname = 'index_drop_fulltext' AND indexname = 'docs_body_idx'",
            vec![],
        )
        .expect("inspect the dropped index catalog entry");

    // Assert
    assert_eq!(
        before.rows(),
        &[vec![Value::Int64(1)], vec![Value::Int64(3)]]
    );
    assert_eq!(after.rows(), before.rows());
    assert_eq!(index_count.rows(), &[vec![Value::Int64(0)]]);
}
