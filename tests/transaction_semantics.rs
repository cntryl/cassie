use cassie::app::{Cassie, CassieError};
use cassie::sql::ast::{CopyFormat, CopyStatement};
use cassie::types::Value;

#[path = "support/sql.rs"]
mod support;
use support::{data_dir, with_fallback};

fn with_source_table<T>(
    label: &str,
    test: impl FnOnce(&Cassie, &cassie::app::CassieSession) -> T,
) -> T {
    with_fallback();
    let path = data_dir(label);
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE transaction_semantics_source (id INT PRIMARY KEY, title TEXT, tenant TEXT, event_at TIMESTAMP, amount INT)",
            vec![],
        )
        .expect("create source table");
    let result = test(&cassie, &session);
    let _ = std::fs::remove_dir_all(path);
    result
}

fn assert_unsupported(error: &CassieError) {
    assert!(matches!(error, CassieError::Unsupported(_)));
}

fn reject_active_transaction_command(
    cassie: &Cassie,
    session: &cassie::app::CassieSession,
    sql: &str,
) {
    cassie.execute_sql(session, "BEGIN", vec![]).expect("begin");
    let error = cassie
        .execute_sql(session, sql, vec![])
        .expect_err("command should be rejected in an active transaction");
    assert_unsupported(&error);
    assert_eq!(session.transaction_status(), "failed");
    cassie
        .execute_sql(session, "ROLLBACK", vec![])
        .expect("rollback rejected command");
}

#[test]
fn should_reject_non_read_committed_begin() {
    // Arrange
    with_source_table("transaction_semantics_isolation", |cassie, session| {
        for sql in [
            "BEGIN ISOLATION LEVEL SERIALIZABLE",
            "BEGIN ISOLATION LEVEL REPEATABLE READ",
        ] {
            // Act
            let error = cassie
                .execute_sql(session, sql, vec![])
                .expect_err("unsupported isolation should fail before BEGIN");

            // Assert
            assert_unsupported(&error);
            assert_eq!(session.transaction_status(), "idle");
        }
    });
}

#[test]
fn should_allow_explicit_read_committed_begin() {
    // Arrange
    with_source_table("transaction_semantics_read_committed", |cassie, session| {
        // Act
        let result = cassie
            .execute_sql(session, "BEGIN ISOLATION LEVEL READ COMMITTED", vec![])
            .expect("read committed should be supported");

        // Assert
        assert_eq!(result.command, "BEGIN");
        assert_eq!(session.transaction_status(), "in_transaction");
        cassie
            .execute_sql(session, "ROLLBACK", vec![])
            .expect("rollback read committed transaction");
    });
}

#[test]
fn should_reject_set_transaction_in_active_transaction() {
    // Arrange
    with_source_table("transaction_semantics_set", |cassie, session| {
        cassie.execute_sql(session, "BEGIN", vec![]).expect("begin");

        // Act
        let error = cassie
            .execute_sql(
                session,
                "SET TRANSACTION ISOLATION LEVEL SERIALIZABLE",
                vec![],
            )
            .expect_err("SET TRANSACTION should be rejected");

        // Assert
        assert_unsupported(&error);
        assert_eq!(session.transaction_status(), "failed");
        cassie
            .execute_sql(session, "ROLLBACK", vec![])
            .expect("rollback SET TRANSACTION rejection");
    });
}

#[test]
fn should_reject_ddl_in_active_transaction() {
    // Arrange
    with_source_table("transaction_semantics_ddl", |cassie, session| {
        for sql in [
            "CREATE TABLE transaction_semantics_new_table (value TEXT)",
            "ALTER TABLE transaction_semantics_source ADD COLUMN rejected TEXT",
            "CREATE SCHEMA transaction_semantics_schema",
            "CREATE INDEX transaction_semantics_new_index ON transaction_semantics_source (title)",
            "CREATE VIEW transaction_semantics_new_view AS SELECT title FROM transaction_semantics_source",
            "CREATE SEQUENCE transaction_semantics_new_sequence",
            "CREATE ROLLUP transaction_semantics_new_rollup ON transaction_semantics_source USING time_bucket('1 hour', event_at) GROUP BY tenant AGGREGATES COUNT(*) AS total",
            "CREATE MATERIALIZED PROJECTION transaction_semantics_new_projection AS SELECT title FROM transaction_semantics_source",
        ] {
            reject_active_transaction_command(cassie, session, sql);
        }

        // Act
        let source = cassie
            .catalog
            .get_schema("transaction_semantics_source")
            .expect("source schema");

        // Assert
        assert!(!cassie
            .catalog
            .relation_exists("transaction_semantics_new_table"));
        assert!(!cassie
            .catalog
            .namespace_exists("transaction_semantics_schema"));
        assert!(!source.fields.iter().any(|field| field.name == "rejected"));
        assert!(cassie
            .catalog
            .get_index(
                "transaction_semantics_source",
                "transaction_semantics_new_index"
            )
            .is_none());
        assert!(cassie
            .catalog
            .get_view("transaction_semantics_new_view")
            .is_none());
        assert!(!cassie
            .catalog
            .sequence_exists("transaction_semantics_new_sequence"));
        assert!(cassie
            .catalog
            .get_rollup("transaction_semantics_new_rollup")
            .is_none());
        assert!(cassie
            .catalog
            .get_materialized_projection("transaction_semantics_new_projection")
            .is_none());
    });
}

#[test]
fn should_preserve_staged_data_after_ddl_rejection() {
    // Arrange
    with_source_table("transaction_semantics_ddl_state", |cassie, session| {
        cassie.execute_sql(session, "BEGIN", vec![]).expect("begin");
        cassie
            .execute_sql(
                session,
                "INSERT INTO transaction_semantics_source (id, title) VALUES (1, 'staged')",
                vec![],
            )
            .expect("stage source row");

        // Act
        let error = cassie
            .execute_sql(
                session,
                "CREATE TABLE transaction_semantics_rejected (value TEXT)",
                vec![],
            )
            .expect_err("DDL should fail after staged DML");

        // Assert
        assert_unsupported(&error);
        assert_eq!(session.transaction_status(), "failed");
        cassie
            .execute_sql(session, "ROLLBACK", vec![])
            .expect("rollback DDL rejection");
        let rows = cassie
            .execute_sql(
                session,
                "SELECT title FROM transaction_semantics_source",
                vec![],
            )
            .expect("read source after rollback");
        assert!(rows.rows.is_empty());
        assert!(!cassie
            .catalog
            .relation_exists("transaction_semantics_rejected"));
    });
}

#[test]
fn should_reject_copy_in_active_transaction() {
    // Arrange
    with_source_table("transaction_semantics_copy", |cassie, session| {
        cassie.execute_sql(session, "BEGIN", vec![]).expect("begin");
        let statement = CopyStatement {
            table: "transaction_semantics_source".to_string(),
            columns: vec!["title".to_string()],
            format: CopyFormat::Csv,
            header: false,
        };

        // Act
        let error = cassie
            .copy_from_csv_stdin(session, &statement, b"copied\n")
            .expect_err("COPY should be rejected in an active transaction");

        // Assert
        assert_unsupported(&error);
        assert_eq!(session.transaction_status(), "failed");
        cassie
            .execute_sql(session, "ROLLBACK", vec![])
            .expect("rollback COPY rejection");
        let rows = cassie
            .execute_sql(
                session,
                "SELECT title FROM transaction_semantics_source",
                vec![],
            )
            .expect("read source after COPY rollback");
        assert!(rows.rows.is_empty());
        assert!(!rows
            .rows
            .iter()
            .flatten()
            .any(|value| { matches!(value, Value::String(title) if title == "copied") }));
    });
}
