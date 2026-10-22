use cassie::types::Value;

#[test]
fn should_return_declared_id_from_search_score_top_k() {
    // Arrange
    let cassie = crate::support_sql::new_in_memory_cassie().expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    for statement in [
        "CREATE TABLE scored_declared_docs (id INT, body TEXT)",
        "INSERT INTO scored_declared_docs (id, body) VALUES (7, 'hello world')",
        "INSERT INTO scored_declared_docs (id, body) VALUES (8, 'goodbye world')",
        "CREATE INDEX scored_declared_docs_body_idx ON scored_declared_docs USING fulltext (body)",
    ] {
        cassie
            .execute_sql(&session, statement, vec![])
            .expect(statement);
    }

    // Act
    let declared = cassie
            .execute_sql(
                &session,
                "SELECT id, search_score(body, 'hello') AS score FROM scored_declared_docs ORDER BY score DESC LIMIT 1",
                vec![],
            )
            .expect("search top-k with declared id");
    let identity = cassie
            .execute_sql(
                &session,
                "SELECT _id, search_score(body, 'hello') AS score FROM scored_declared_docs ORDER BY score DESC LIMIT 1",
                vec![],
            )
            .expect("search top-k with row identity");

    // Assert
    assert_eq!(declared.rows()[0][0], Value::Int64(7));
    assert!(
        matches!(&identity.rows()[0][0], Value::String(value) if !value.is_empty()),
        "expected the internal identity, got {:?}",
        identity.rows()[0][0]
    );
}
