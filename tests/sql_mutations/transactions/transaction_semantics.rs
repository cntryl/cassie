use cassie::app::{Cassie, CassieError};
use cassie::sql::ast::{CopyFormat, CopyStatement};
use cassie::types::Value;

use super::support_sql as support;
use support::data_dir;

fn with_source_table<T>(
    label: &str,
    test: impl FnOnce(&Cassie, &cassie::app::CassieSession) -> T,
) -> T {
    let path = data_dir(label);
    let cassie = crate::support_sql::new_in_memory_cassie().expect("create Cassie");
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
        assert_eq!(result.command(), "BEGIN");
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
fn should_reject_unselected_ddl_in_active_transaction() {
    // Arrange
    with_source_table("transaction_semantics_ddl", |cassie, session| {
        for sql in [
            "CREATE VIEW transaction_semantics_new_view AS SELECT title FROM transaction_semantics_source",
            "CREATE SEQUENCE transaction_semantics_new_sequence",
            "CREATE ROLLUP transaction_semantics_new_rollup ON transaction_semantics_source USING time_bucket('1 hour', event_at) GROUP BY tenant AGGREGATES COUNT(*) AS total",
            "CREATE MATERIALIZED PROJECTION transaction_semantics_new_projection AS SELECT title FROM transaction_semantics_source",
        ] {
            reject_active_transaction_command(cassie, session, sql);
        }

        // Act
        // Assert
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
fn should_publish_transactional_table_in_its_schema() {
    // Arrange
    with_source_table("transaction_semantics_selected_ddl", |cassie, session| {
        let observer = cassie.create_session("observer", None);
        cassie
            .execute_sql(session, "BEGIN", vec![])
            .expect("begin transaction");

        // Act
        cassie
            .execute_sql(
                session,
                "CREATE SCHEMA transaction_semantics_schema",
                vec![],
            )
            .expect("create schema transactionally");
        cassie
            .execute_sql(
                session,
                "CREATE TABLE transaction_semantics_schema.transaction_semantics_new_table (value TEXT)",
                vec![],
            )
            .expect("create table transactionally");
        let own_table = cassie
            .execute_sql(
                session,
                "SELECT table_name FROM information_schema.tables \
                 WHERE table_schema = 'transaction_semantics_schema' \
                   AND table_name = 'transaction_semantics_new_table'",
                vec![],
            )
            .expect("read the staged table")
            .rows()
            .to_vec();
        let other_table = cassie
            .execute_sql(
                &observer,
                "SELECT table_name FROM information_schema.tables \
                 WHERE table_schema = 'transaction_semantics_schema' \
                   AND table_name = 'transaction_semantics_new_table'",
                vec![],
            )
            .expect("read committed table catalog")
            .rows()
            .to_vec();
        cassie
            .execute_sql(session, "COMMIT", vec![])
            .expect("publish transactional DDL");
        let published_table = cassie
            .execute_sql(
                &observer,
                "SELECT table_name FROM information_schema.tables \
                 WHERE table_schema = 'transaction_semantics_schema' \
                   AND table_name = 'transaction_semantics_new_table'",
                vec![],
            )
            .expect("read published table catalog")
            .rows()
            .to_vec();

        // Assert
        assert_eq!(
            own_table,
            vec![vec![Value::String(
                "transaction_semantics_new_table".to_owned()
            )]]
        );
        assert_eq!(other_table, [] as [Vec<Value>; 0]);
        assert_eq!(published_table, own_table);
    });
}

#[test]
fn should_publish_transactional_scalar_index() {
    // Arrange
    with_source_table("transaction_semantics_selected_index", |cassie, session| {
        let observer = cassie.create_session("observer", None);
        cassie
            .execute_sql(session, "BEGIN", vec![])
            .expect("begin transaction");

        // Act
        cassie
            .execute_sql(
                session,
                "CREATE INDEX transaction_semantics_new_index ON transaction_semantics_source (title)",
                vec![],
            )
            .expect("create scalar index transactionally");
        let other_index = cassie
            .execute_sql(
                &observer,
                "SELECT indexname FROM pg_indexes \
                 WHERE indexname = 'transaction_semantics_new_index'",
                vec![],
            )
            .expect("read committed index catalog")
            .rows()
            .to_vec();
        cassie
            .execute_sql(session, "COMMIT", vec![])
            .expect("publish transactional index");
        let published_index = cassie
            .execute_sql(
                &observer,
                "SELECT indexname FROM pg_indexes \
                 WHERE indexname = 'transaction_semantics_new_index'",
                vec![],
            )
            .expect("read published index catalog")
            .rows()
            .to_vec();

        // Assert
        assert_eq!(other_index, [] as [Vec<Value>; 0]);
        assert_eq!(
            published_index,
            vec![vec![Value::String(
                "transaction_semantics_new_index".to_owned()
            )]]
        );
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
                "CREATE VIEW transaction_semantics_rejected_view AS \
                 SELECT title FROM transaction_semantics_source",
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
        assert_eq!(rows.rows(), [] as [std::vec::Vec<cassie::types::Value>; 0]);
        assert!(cassie
            .catalog
            .get_view("transaction_semantics_rejected_view")
            .is_none());
    });
}

#[test]
fn should_stage_copy_in_active_transaction() {
    // Arrange
    with_source_table("transaction_semantics_copy", |cassie, session| {
        cassie.execute_sql(session, "BEGIN", vec![]).expect("begin");
        let statement = CopyStatement {
            table: "transaction_semantics_source".to_string(),
            columns: vec!["id".to_string(), "title".to_string()],
            format: CopyFormat::Csv,
            header: false,
        };

        // Act
        cassie
            .copy_from_csv_stdin(session, &statement, b"1,copied\n")
            .expect("stage COPY rows");

        // Assert
        assert_eq!(session.transaction_status(), "in_transaction");
        let staged = cassie
            .execute_sql(
                session,
                "SELECT title FROM transaction_semantics_source",
                vec![],
            )
            .expect("read staged COPY row");
        assert_eq!(staged.rows(), vec![vec![Value::String("copied".into())]]);
        cassie
            .execute_sql(session, "COMMIT", vec![])
            .expect("commit COPY");
        let rows = cassie
            .execute_sql(
                session,
                "SELECT title FROM transaction_semantics_source",
                vec![],
            )
            .expect("read source after COPY rollback");
        assert_eq!(rows.rows(), vec![vec![Value::String("copied".into())]]);
    });
}
