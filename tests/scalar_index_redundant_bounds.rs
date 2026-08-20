use cassie::app::{Cassie, CassieSession};
use cassie::types::Value;

#[path = "support/sql.rs"]
mod support;
use support::*;

fn seed_score_table(cassie: &Cassie, session: &CassieSession, table: &str, indexed: bool) {
    cassie
        .execute_sql(
            session,
            &format!("CREATE TABLE {table} (score BIGINT, label TEXT)"),
            vec![],
        )
        .unwrap();
    for score in [4, 5, 6, 9, 10, 11, 12] {
        cassie
            .midge
            .put_document(
                table,
                Some(format!("row-{score}")),
                serde_json::json!({"score": score, "label": format!("label-{score}")}),
            )
            .unwrap();
    }
    if indexed {
        cassie
            .execute_sql(
                session,
                &format!("CREATE INDEX {table}_score_idx ON {table} USING btree (score)"),
                vec![],
            )
            .unwrap();
    }
}

fn seed_expression_table(cassie: &Cassie, session: &CassieSession, table: &str, indexed: bool) {
    cassie
        .execute_sql(
            session,
            &format!("CREATE TABLE {table} (title TEXT, label TEXT)"),
            vec![],
        )
        .unwrap();
    for title in ["alpha", "beta", "delta", "gamma", "omega"] {
        cassie
            .midge
            .put_document(
                table,
                Some(format!("row-{title}")),
                serde_json::json!({"title": title, "label": format!("label-{title}")}),
            )
            .unwrap();
    }
    if indexed {
        cassie
            .execute_sql(
                session,
                &format!("CREATE INDEX {table}_lower_idx ON {table} USING btree (lower(title))"),
                vec![],
            )
            .unwrap();
    }
}

fn query_rows(
    cassie: &Cassie,
    session: &CassieSession,
    table: &str,
    projection: &str,
    predicate: &str,
) -> Vec<Vec<cassie::types::Value>> {
    cassie
        .execute_sql(
            session,
            &format!("SELECT {projection} FROM {table} WHERE {predicate} ORDER BY {projection}"),
            vec![],
        )
        .unwrap()
        .rows
}

fn query_expression_rows(
    cassie: &Cassie,
    session: &CassieSession,
    table: &str,
    predicate: &str,
) -> Vec<Vec<Value>> {
    cassie
        .execute_sql(
            session,
            &format!("SELECT title FROM {table} WHERE {predicate} ORDER BY lower(title)"),
            vec![],
        )
        .unwrap()
        .rows
}

#[test]
fn should_match_full_scan_when_intersecting_repeated_column_bounds() {
    // Arrange
    use_local_storage();
    let path = data_dir("scalar_index_repeated_column_bounds");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        seed_score_table(&cassie, &session, "repeated_column_indexed", true);
        seed_score_table(&cassie, &session, "repeated_column_baseline", false);

        // Act
        let comparisons = [
            "score > 10 AND score > 5",
            "score >= 10 AND score >= 5",
            "score < 5 AND score < 10",
            "score <= 5 AND score <= 10",
            "score > 10 AND score >= 10",
            "score < 10 AND score <= 10",
        ]
        .map(|predicate| {
            (
                predicate,
                query_rows(
                    &cassie,
                    &session,
                    "repeated_column_indexed",
                    "score",
                    predicate,
                ),
                query_rows(
                    &cassie,
                    &session,
                    "repeated_column_baseline",
                    "score",
                    predicate,
                ),
            )
        });
        let explain = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT score FROM repeated_column_indexed WHERE score > 10 AND score > 5 ORDER BY score",
                vec![],
            )
            .unwrap();

        // Assert
        for (predicate, indexed, baseline) in comparisons {
            assert_eq!(indexed, baseline, "indexed result diverged for {predicate}");
        }
        let Value::String(plan) = &explain.rows[0][0] else {
            panic!("expected textual plan");
        };
        assert!(plan.contains("repeated_column_indexed_score_idx"));
        assert!(plan.contains("access_path_reason=scalar-index-range"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_match_full_scan_when_intersecting_repeated_expression_bounds() {
    // Arrange
    use_local_storage();
    let path = data_dir("scalar_index_repeated_expression_bounds");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        seed_expression_table(&cassie, &session, "repeated_expression_indexed", true);
        seed_expression_table(&cassie, &session, "repeated_expression_baseline", false);

        // Act
        let indexed_lower = query_expression_rows(
            &cassie,
            &session,
            "repeated_expression_indexed",
            "lower(title) > 'delta' AND lower(title) > 'alpha'",
        );
        let baseline_lower = query_expression_rows(
            &cassie,
            &session,
            "repeated_expression_baseline",
            "lower(title) > 'delta' AND lower(title) > 'alpha'",
        );
        let indexed_upper = query_expression_rows(
            &cassie,
            &session,
            "repeated_expression_indexed",
            "lower(title) < 'gamma' AND lower(title) <= 'omega'",
        );
        let baseline_upper = query_expression_rows(
            &cassie,
            &session,
            "repeated_expression_baseline",
            "lower(title) < 'gamma' AND lower(title) <= 'omega'",
        );
        let explain = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT title FROM repeated_expression_indexed WHERE lower(title) > 'delta' AND lower(title) > 'alpha' ORDER BY lower(title)",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(indexed_lower, baseline_lower);
        assert_eq!(indexed_upper, baseline_upper);
        let Value::String(plan) = &explain.rows[0][0] else {
            panic!("expected textual plan");
        };
        assert!(plan.contains("repeated_expression_indexed_lower_idx"));
        assert!(plan.contains("access_path_reason=scalar-index-range"));

        let _ = std::fs::remove_dir_all(path);
    });
}
