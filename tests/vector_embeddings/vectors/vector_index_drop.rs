use super::support_read_equivalence::{sql, with_fixture as with_vector_fixture};

fn seed(cassie: &cassie::app::Cassie, session: &cassie::app::CassieSession, table: &str) {
    sql(
        cassie,
        session,
        &format!("CREATE TABLE {table} (body TEXT,embedding VECTOR(3))"),
    );
    for (id, vector) in [
        ("a", [1.0, 0.0, 0.0]),
        ("b", [0.0, 1.0, 0.0]),
        ("c", [0.0, 0.0, 1.0]),
    ] {
        cassie
            .midge
            .put_document(
                table,
                Some(id.to_string()),
                serde_json::json!({"body":"alpha","embedding":vector}),
            )
            .expect("seed vector rows");
    }
}

#[test]
fn should_preserve_vector_top_k_results_after_schema_qualified_drop() {
    // Arrange
    with_vector_fixture("vector_index_drop", |cassie, session| {
        seed(cassie, session, "vector_index_drop_rows");
        sql(
            cassie,
            session,
            "CREATE INDEX vector_index_drop_idx \
             ON vector_index_drop_rows USING vector (embedding) \
             WITH (source_field = body, metric = l2)",
        );
        let query = "SELECT _id, vector_distance(embedding, '[1,0,0]') AS distance \
                     FROM vector_index_drop_rows ORDER BY distance ASC LIMIT 2";
        let before = sql(cassie, session, query);

        // Act
        sql(cassie, session, "DROP INDEX public.vector_index_drop_idx");
        let after = sql(cassie, session, query);
        let index_count = sql(
            cassie,
            session,
            "SELECT count(*) FROM pg_catalog.pg_indexes \
             WHERE schemaname = 'public' AND indexname = 'vector_index_drop_idx'",
        );

        // Assert
        assert_eq!(after.rows(), before.rows());
        assert_eq!(index_count.rows()[0][0], cassie::types::Value::Int64(0));
    });
}

#[test]
fn should_reject_unique_vector_index_before_publication() {
    // Arrange
    with_vector_fixture("vector_index_unsupported_clauses", |cassie, session| {
        sql(
            cassie,
            session,
            "CREATE TABLE vector_index_unique_rows (body TEXT, embedding VECTOR(3))",
        );
        // Act
        let unique = cassie
            .execute_sql(
                session,
                "CREATE UNIQUE INDEX vector_index_unique_idx \
                 ON vector_index_unique_rows USING vector (embedding) \
                 WITH (source_field = body)",
                vec![],
            )
            .expect_err("vector indexes do not enforce uniqueness");
        // Assert
        assert!(matches!(unique, cassie::app::CassieError::Unsupported(_)));
        assert!(cassie
            .catalog
            .get_index("vector_index_unique_rows", "vector_index_unique_idx")
            .is_none());
        assert!(cassie
            .midge
            .get_vector_index("vector_index_unique_rows", "embedding")
            .expect("check unique vector metadata")
            .is_none());
    });
}

#[test]
fn should_reject_partial_vector_index_before_publication() {
    // Arrange
    with_vector_fixture("vector_index_partial_clause", |cassie, session| {
        sql(
            cassie,
            session,
            "CREATE TABLE vector_index_partial_rows (body TEXT, embedding VECTOR(3))",
        );

        // Act
        let partial = cassie
            .execute_sql(
                session,
                "CREATE INDEX vector_index_partial_idx \
                 ON vector_index_partial_rows USING vector (embedding) \
                 WITH (source_field = body) WHERE body IS NOT NULL",
                vec![],
            )
            .expect_err("vector indexes do not apply partial predicates");

        // Assert
        assert!(matches!(partial, cassie::app::CassieError::Unsupported(_)));
        assert!(cassie
            .catalog
            .get_index("vector_index_partial_rows", "vector_index_partial_idx")
            .is_none());
        assert!(cassie
            .midge
            .get_vector_index("vector_index_partial_rows", "embedding")
            .expect("check partial vector metadata")
            .is_none());
    });
}
