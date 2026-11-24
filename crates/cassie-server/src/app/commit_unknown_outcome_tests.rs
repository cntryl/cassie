//! A SQL `COMMIT` whose native outcome is unknown fences storage until restart.

use crate::app::{Cassie, CassieError, CassieSession};
use crate::config::CassieRuntimeConfig;
use crate::types::Value;

/// What a client can observe after an unknown commit outcome, before and after
/// the store is reopened.
#[derive(Debug, PartialEq)]
struct Observed {
    commit: Option<&'static str>,
    transaction: &'static str,
    same_session_read: Option<&'static str>,
    same_session_begin: Option<&'static str>,
    same_session_rollback: Option<&'static str>,
    other_session_read: Option<&'static str>,
    other_session_write: Option<&'static str>,
    rows_after_restart: Vec<Vec<Value>>,
    write_after_restart: Option<&'static str>,
}

fn sql_state(cassie: &Cassie, session: &CassieSession, sql: &str) -> Option<&'static str> {
    cassie
        .execute_sql(session, sql, vec![])
        .err()
        .as_ref()
        .map(CassieError::sql_state)
}

fn open(path: &std::path::Path) -> Cassie {
    let cassie =
        Cassie::new_local_with_data_dir_and_config(path, CassieRuntimeConfig::default()).unwrap();
    cassie.startup().unwrap();
    cassie
}

fn observe_unknown_commit(inject: fn()) -> Observed {
    let path = std::env::temp_dir().join(format!("cassie-commit-unknown-{}", uuid::Uuid::new_v4()));
    let cassie = open(&path);
    let session = cassie.create_session("owner", None);
    let other = cassie.create_session("other", None);
    for sql in [
        "CREATE TABLE unknown_rows (value INT)",
        "BEGIN",
        "INSERT INTO unknown_rows (value) VALUES (1)",
    ] {
        cassie.execute_sql(&session, sql, vec![]).unwrap();
    }
    inject();
    let commit = sql_state(&cassie, &session, "COMMIT");
    let transaction = if session.is_transaction_active() {
        "active"
    } else if session.is_transaction_failed() {
        "failed"
    } else {
        "idle"
    };
    let same_session_read = sql_state(&cassie, &session, "SELECT value FROM unknown_rows");
    let same_session_begin = sql_state(&cassie, &session, "BEGIN");
    let same_session_rollback = sql_state(&cassie, &session, "ROLLBACK");
    let other_session_read = sql_state(&cassie, &other, "SELECT value FROM unknown_rows");
    let other_session_write = sql_state(
        &cassie,
        &other,
        "INSERT INTO unknown_rows (value) VALUES (2)",
    );
    cassie.shutdown();
    drop(cassie);

    let recovered = open(&path);
    let reader = recovered.create_session("owner", None);
    let rows_after_restart = recovered
        .execute_sql(
            &reader,
            "SELECT value FROM unknown_rows ORDER BY value",
            vec![],
        )
        .unwrap()
        .rows()
        .to_vec();
    let write_after_restart = sql_state(
        &recovered,
        &reader,
        "INSERT INTO unknown_rows (value) VALUES (3)",
    );
    recovered.shutdown();
    drop(recovered);
    std::fs::remove_dir_all(path).unwrap();
    Observed {
        commit,
        transaction,
        same_session_read,
        same_session_begin,
        same_session_rollback,
        other_session_read,
        other_session_write,
        rows_after_restart,
        write_after_restart,
    }
}

fn fenced(rows_after_restart: Vec<Vec<Value>>) -> Observed {
    Observed {
        commit: Some("58030"),
        transaction: "idle",
        same_session_read: Some("58030"),
        same_session_begin: Some("58030"),
        same_session_rollback: None,
        other_session_read: Some("58030"),
        other_session_write: Some("58030"),
        rows_after_restart,
        write_after_restart: None,
    }
}

#[test]
fn should_fence_storage_then_recover_the_row_when_an_unknown_commit_applied() {
    // Arrange
    let inject = crate::midge::adapter::inject_native_commit_unknown_after_success;

    // Act
    let observed = observe_unknown_commit(inject);

    // Assert
    assert_eq!(observed, fenced(vec![vec![Value::Int64(1)]]));
}

#[test]
fn should_fence_storage_then_recover_no_row_when_an_unknown_commit_did_not_apply() {
    // Arrange
    let inject = crate::midge::adapter::inject_native_commit_unknown_before_commit;

    // Act
    let observed = observe_unknown_commit(inject);

    // Assert
    assert_eq!(observed, fenced(Vec::new()));
}
