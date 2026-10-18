use super::support_read_equivalence::{sql, with_fixture};

#[test]
fn should_preserve_zero_limit_in_fulltext_top_k() {
    // Arrange
    with_fixture("fulltext_limit_zero", |cassie, session| {
        sql(cassie, session, "CREATE TABLE search_zero (body TEXT)");
        sql(
            cassie,
            session,
            "INSERT INTO search_zero VALUES ('alpha beta'),('alpha gamma')",
        );
        sql(
            cassie,
            session,
            "CREATE INDEX search_zero_body ON search_zero USING fulltext(body)",
        );
        // Act
        for tail in ["LIMIT 0", "LIMIT 0 OFFSET 1"] {
            let result = sql(cassie, session, &format!("SELECT _id,search_score(body,'alpha') AS score FROM search_zero WHERE search(body,'alpha') ORDER BY score DESC {tail}"));
            // Assert
            assert!(result.rows().is_empty(), "{tail}");
        }
    });
}

#[test]
fn should_preserve_fulltext_matches_with_unproven_partial_scalar_prefilters() {
    // Arrange
    with_fixture("partial_fulltext_prefilter", |cassie, session| {
        sql(
            cassie,
            session,
            "CREATE TABLE partial_search (body TEXT,status TEXT,tier TEXT)",
        );
        sql(cassie, session, "INSERT INTO partial_search VALUES ('alpha beta','active','gold'),('alpha gamma','active','silver'),('alpha delta','inactive','gold')");
        sql(
            cassie,
            session,
            "CREATE INDEX partial_search_body ON partial_search USING fulltext(body)",
        );
        let query = "SELECT body,status,search_score(body,'alpha') AS score FROM partial_search WHERE search(body,'alpha') AND status = 'active'";
        let baseline = sql(cassie, session, query);
        let mut baseline_rows = baseline.rows().to_vec();
        baseline_rows.sort_by_key(|row| format!("{row:?}"));
        sql(
            cassie,
            session,
            "CREATE INDEX partial_search_status ON partial_search(status) WHERE tier = 'gold'",
        );
        // Act
        let narrowed = sql(cassie, session, query);
        let mut narrowed_rows = narrowed.rows().to_vec();
        narrowed_rows.sort_by_key(|row| format!("{row:?}"));
        // Assert
        assert_eq!(baseline.rows().len(), 2);
        assert_eq!(narrowed_rows, baseline_rows);
    });
}

#[test]
fn should_preserve_fulltext_top_k_on_column_store_tables() {
    // Arrange
    with_fixture("column_fulltext_equivalence", |cassie, session| {
        for (table, suffix) in [
            ("search_rows", ""),
            ("search_columns", " WITH (storage = column_store)"),
        ] {
            sql(
                cassie,
                session,
                &format!("CREATE TABLE {table} (body TEXT){suffix}"),
            );
            for (id, body) in [("d1", "alpha alpha"), ("d2", "alpha beta"), ("d3", "gamma")] {
                cassie
                    .midge
                    .put_document(
                        table,
                        Some(id.to_string()),
                        serde_json::json!({"body":body}),
                    )
                    .expect("seed same identities");
            }
        }
        for indexed in [false, true] {
            if indexed {
                for table in ["search_rows", "search_columns"] {
                    sql(
                        cassie,
                        session,
                        &format!("CREATE INDEX {table}_body ON {table} USING fulltext(body)"),
                    );
                }
            }
            for transactional in [false, true] {
                if transactional {
                    sql(cassie, session, "BEGIN");
                }
                // Act
                for predicate in ["", " WHERE search(body,'alpha')"] {
                    let query = |table| {
                        format!("SELECT _id,search_score(body,'alpha') AS score FROM {table}{predicate} ORDER BY score DESC LIMIT 2")
                    };
                    let baseline = sql(cassie, session, &query("search_rows"));
                    let columns = sql(cassie, session, &query("search_columns"));
                    // Assert
                    assert_eq!(
                        columns.rows(),
                        baseline.rows(),
                        "{predicate}: indexed={indexed}, transactional={transactional}"
                    );
                }
                if transactional {
                    sql(cassie, session, "ROLLBACK");
                }
            }
        }
    });
}
#[test]
fn should_preserve_canonical_timestamp_text_matches_in_fulltext_prefilters() {
    // Arrange
    with_fixture("canonical_text_fulltext_prefilter", |cassie, session| {
        sql(
            cassie,
            session,
            "CREATE TABLE canonical_search (body TEXT,code TEXT)",
        );
        sql(cassie, session, "INSERT INTO canonical_search VALUES ('alpha beta','2024-01-01T00:00:00Z'),('alpha gamma','2024-01-01T00:00:00.000000Z'),('alpha delta','ordinary')");
        sql(
            cassie,
            session,
            "CREATE INDEX canonical_search_body ON canonical_search USING fulltext(body)",
        );
        let query = "SELECT body,search_score(body,'alpha') AS score FROM canonical_search WHERE search(body,'alpha') AND code = '2024-01-01T00:00:00Z'";
        let baseline = sql(cassie, session, query);
        let mut baseline_rows = baseline.rows().to_vec();
        baseline_rows.sort_by_key(|row| format!("{row:?}"));
        sql(
            cassie,
            session,
            "CREATE INDEX canonical_search_code ON canonical_search(code)",
        );
        // Act
        let narrowed = sql(cassie, session, query);
        let mut narrowed_rows = narrowed.rows().to_vec();
        narrowed_rows.sort_by_key(|row| format!("{row:?}"));
        // Assert
        assert_eq!(baseline.rows().len(), 2);
        assert_eq!(narrowed_rows, baseline_rows);
    });
}
