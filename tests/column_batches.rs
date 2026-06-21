#![allow(unused_imports, dead_code)]

use cassie::app::Cassie;
use cassie::catalog::IndexKind;
use cassie::sql::ast::QueryStatement;
use cassie::sql::parse_statement;
use cassie::types::Value;

#[path = "support/sql.rs"]
mod support;
use support::*;

#[test]
fn should_parse_column_index_with_segment_size() {
    // Arrange
    let sql =
        "CREATE INDEX idx_docs_column ON docs USING column (title, body) WITH (segment_size = 2)";

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    let QueryStatement::CreateIndex(statement) = parsed.statement else {
        panic!("expected create index statement");
    };
    assert_eq!(statement.name, "idx_docs_column");
    assert_eq!(
        statement.fields,
        vec!["title".to_string(), "body".to_string()]
    );
    assert_eq!(statement.kind, IndexKind::Column);
    assert_eq!(
        statement.options.get("segment_size"),
        Some(&"2".to_string())
    );
}

#[test]
fn should_read_covered_projection_from_column_batch_index() {
    // Arrange
    with_fallback();
    let path = data_dir("column_batch_projection");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE column_batch_projection (title TEXT, body TEXT, score INT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO column_batch_projection (title, body, score) VALUES ('alpha', 'one', 10)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO column_batch_projection (title, score) VALUES ('beta', 20)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX idx_column_batch_projection ON column_batch_projection USING column (title, body) WITH (segment_size = 1)",
                vec![],
            )
            .unwrap();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT title, body FROM column_batch_projection WHERE title = 'alpha'",
                vec![],
            )
            .unwrap();
        let explain = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT title, body FROM column_batch_projection WHERE title = 'alpha'",
                vec![],
            )
            .unwrap();
        let metrics = cassie.metrics();

        // Assert
        assert_eq!(result.rows, vec![vec![
            Value::String("alpha".to_string()),
            Value::String("one".to_string()),
        ]]);
        let Value::String(plan) = &explain.rows[0][0] else {
            panic!("expected textual plan");
        };
        assert!(plan.contains("column_batch_index=idx_column_batch_projection"));
        assert_eq!(metrics["column_batches"]["scans"], 1);
        assert_eq!(metrics["column_batches"]["row_fetches_avoided"], 1);
    });

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_persist_hydrate_drop_column_batch_metadata() {
    // Arrange
    with_fallback();
    let path = data_dir("column_batch_metadata");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            let session = cassie.create_session("tester", None);
            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE column_batch_metadata (title TEXT, body TEXT)",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO column_batch_metadata (title, body) VALUES ('alpha', 'one')",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "CREATE INDEX idx_column_batch_metadata ON column_batch_metadata USING column (title, body) WITH (segment_size = 1)",
                    vec![],
                )
                .unwrap();
        }

        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.hydrate_catalog().unwrap();
        let session = cassie.create_session("tester", None);

        // Act
        let metadata = cassie
            .midge
            .get_column_batch_metadata("column_batch_metadata", "idx_column_batch_metadata")
            .unwrap()
            .expect("column batch metadata should hydrate");
        let result = cassie
            .execute_sql(
                &session,
                "SELECT title, body FROM column_batch_metadata WHERE title = 'alpha'",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "DROP INDEX idx_column_batch_metadata ON column_batch_metadata",
                vec![],
            )
            .unwrap();
        let dropped = cassie
            .midge
            .get_column_batch_metadata("column_batch_metadata", "idx_column_batch_metadata")
            .unwrap();

        // Assert
        assert_eq!(metadata.fields, vec!["title".to_string(), "body".to_string()]);
        assert_eq!(metadata.segments.len(), 1);
        assert_eq!(result.rows.len(), 1);
        assert!(dropped.is_none());
    });

    let _ = std::fs::remove_dir_all(path);
}
