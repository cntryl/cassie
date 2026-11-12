//! Selected REPEATABLE READ first-committer-wins and retained-history resource bounds.

use cassie::app::{Cassie, CassieError, CassieSession, TransactionErrorKind};
use cassie::config::CassieRuntimeConfig;
use cassie::types::Value;

use crate::support_sql_fixture::sql_fixture;

fn start_rr_writer(fixture: &crate::support_sql_fixture::SqlFixture) -> CassieSession {
    let writer = fixture.cassie.create_session("tester", None);
    for sql in [
        "BEGIN ISOLATION LEVEL REPEATABLE READ READ WRITE",
        "SELECT id, v FROM rr_fcw_rows ORDER BY id",
    ] {
        fixture
            .cassie
            .execute_sql(&writer, sql, Vec::new())
            .expect("retained view");
    }
    writer
}

fn is_serialization_failure(error: &CassieError) -> bool {
    matches!(
        error,
        CassieError::Transaction {
            kind: TransactionErrorKind::SerializationFailure,
            ..
        }
    )
}

fn first_committer_wins_fixture() -> crate::support_sql_fixture::SqlFixture {
    sql_fixture(
        "rr-first-committer-wins",
        &[
            "CREATE TABLE rr_fcw_rows (id INT PRIMARY KEY, v INT)",
            "INSERT INTO rr_fcw_rows VALUES (1, 1), (2, 2)",
        ],
    )
}

#[test]
fn should_reject_repeatable_update_of_a_row_changed_after_the_retained_view() {
    // Arrange
    let fixture = first_committer_wins_fixture();
    let writer = start_rr_writer(&fixture);
    fixture
        .execute("UPDATE rr_fcw_rows SET v = 10 WHERE id = 1")
        .expect("competing commit");

    // Act
    let update = fixture
        .cassie
        .execute_sql(
            &writer,
            "UPDATE rr_fcw_rows SET v = 99 WHERE id = 1",
            Vec::new(),
        )
        .and_then(|_| fixture.cassie.execute_sql(&writer, "COMMIT", Vec::new()));
    let _ = fixture.cassie.execute_sql(&writer, "ROLLBACK", Vec::new());
    let visible = fixture.rows("SELECT id, v FROM rr_fcw_rows ORDER BY id");

    // Assert
    assert!(
        update.as_ref().is_err_and(is_serialization_failure),
        "a stale RR update must fail with 40001"
    );
    assert_eq!(
        visible,
        vec![
            vec![Value::Int64(1), Value::Int64(10)],
            vec![Value::Int64(2), Value::Int64(2)]
        ]
    );
}

#[test]
fn should_reject_repeatable_delete_of_a_row_changed_after_the_retained_view() {
    // Arrange
    let fixture = first_committer_wins_fixture();
    let writer = start_rr_writer(&fixture);
    fixture
        .execute("UPDATE rr_fcw_rows SET v = 10 WHERE id = 1")
        .expect("competing commit");

    // Act
    let delete = fixture
        .cassie
        .execute_sql(&writer, "DELETE FROM rr_fcw_rows WHERE id = 1", Vec::new())
        .and_then(|_| fixture.cassie.execute_sql(&writer, "COMMIT", Vec::new()));
    let _ = fixture.cassie.execute_sql(&writer, "ROLLBACK", Vec::new());
    let visible = fixture.rows("SELECT id, v FROM rr_fcw_rows ORDER BY id");

    // Assert
    assert!(
        delete.as_ref().is_err_and(is_serialization_failure),
        "a stale RR delete must fail with 40001"
    );
    assert_eq!(
        visible,
        vec![
            vec![Value::Int64(1), Value::Int64(10)],
            vec![Value::Int64(2), Value::Int64(2)]
        ]
    );
}

#[test]
fn should_reject_repeatable_update_of_a_row_deleted_after_the_retained_view() {
    // Arrange
    let fixture = first_committer_wins_fixture();
    let writer = start_rr_writer(&fixture);
    fixture
        .execute("DELETE FROM rr_fcw_rows WHERE id = 1")
        .expect("competing delete");

    // Act
    let update = fixture
        .cassie
        .execute_sql(
            &writer,
            "UPDATE rr_fcw_rows SET v = 99 WHERE id = 1",
            Vec::new(),
        )
        .and_then(|_| fixture.cassie.execute_sql(&writer, "COMMIT", Vec::new()));
    let _ = fixture.cassie.execute_sql(&writer, "ROLLBACK", Vec::new());
    let visible = fixture.rows("SELECT id, v FROM rr_fcw_rows ORDER BY id");

    // Assert
    assert!(
        update.as_ref().is_err_and(is_serialization_failure),
        "updating a row deleted since the retained view must fail with 40001"
    );
    assert_eq!(visible, vec![vec![Value::Int64(2), Value::Int64(2)]]);
}

#[test]
fn should_reject_repeatable_delete_of_a_row_deleted_after_the_retained_view() {
    // Arrange
    let fixture = first_committer_wins_fixture();
    let writer = start_rr_writer(&fixture);
    fixture
        .execute("DELETE FROM rr_fcw_rows WHERE id = 1")
        .expect("competing delete");

    // Act
    let delete = fixture
        .cassie
        .execute_sql(&writer, "DELETE FROM rr_fcw_rows WHERE id = 1", Vec::new())
        .and_then(|_| fixture.cassie.execute_sql(&writer, "COMMIT", Vec::new()));
    let _ = fixture.cassie.execute_sql(&writer, "ROLLBACK", Vec::new());
    let visible = fixture.rows("SELECT id, v FROM rr_fcw_rows ORDER BY id");

    // Assert
    assert!(
        delete.as_ref().is_err_and(is_serialization_failure),
        "deleting a row deleted since the retained view must fail with 40001"
    );
    assert_eq!(visible, vec![vec![Value::Int64(2), Value::Int64(2)]]);
}

#[test]
fn should_allow_repeatable_delete_of_an_unchanged_row() {
    // Arrange
    let fixture = first_committer_wins_fixture();
    let writer = start_rr_writer(&fixture);
    fixture
        .execute("UPDATE rr_fcw_rows SET v = 10 WHERE id = 1")
        .expect("competing commit on a disjoint row");

    // Act
    let outcome = ["DELETE FROM rr_fcw_rows WHERE id = 2", "COMMIT"]
        .into_iter()
        .try_for_each(|sql| {
            fixture
                .cassie
                .execute_sql(&writer, sql, Vec::new())
                .map(|_| ())
        });
    let visible = fixture.rows("SELECT id, v FROM rr_fcw_rows ORDER BY id");

    // Assert
    assert!(outcome.is_ok(), "a disjoint RR delete must publish");
    assert_eq!(visible, vec![vec![Value::Int64(1), Value::Int64(10)]]);
}

/// Outcome of one competing commit while an RR owner holds a retained view.
struct HistoryProbe {
    owner_captured: bool,
    competing: Result<(), CassieError>,
    rows_after: Option<i64>,
}

fn probe_history_budget(budget: usize) -> HistoryProbe {
    let mut config = CassieRuntimeConfig::default();
    config.limits.repeatable_read_history_memory_budget_bytes = budget;
    let Ok(cassie) = Cassie::new_in_memory_with_config(config) else {
        return HistoryProbe {
            owner_captured: false,
            competing: Ok(()),
            rows_after: None,
        };
    };
    let owner = cassie.create_session("tester", None);
    let writer = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &writer,
            "CREATE TABLE rr_history_rows (id INT PRIMARY KEY)",
            Vec::new(),
        )
        .expect("create table");
    let owner_captured = [
        "BEGIN ISOLATION LEVEL REPEATABLE READ READ WRITE",
        "SELECT count(*) FROM rr_history_rows",
    ]
    .into_iter()
    .all(|sql| cassie.execute_sql(&owner, sql, Vec::new()).is_ok());
    if !owner_captured {
        return HistoryProbe {
            owner_captured,
            competing: Ok(()),
            rows_after: None,
        };
    }
    let competing = cassie
        .execute_sql(
            &writer,
            "INSERT INTO rr_history_rows VALUES (1), (2), (3)",
            Vec::new(),
        )
        .map(|_| ());
    cassie
        .execute_sql(&owner, "ROLLBACK", Vec::new())
        .expect("retire the retained owner");
    let rows_after = cassie
        .execute_sql(&writer, "SELECT count(*) FROM rr_history_rows", Vec::new())
        .ok()
        .and_then(|result| match result.rows() {
            [row] => match row.as_slice() {
                [Value::Int64(count)] => Some(*count),
                _ => None,
            },
            _ => None,
        });
    HistoryProbe {
        owner_captured,
        competing,
        rows_after,
    }
}

fn smallest_budget(admits: impl Fn(&HistoryProbe) -> bool) -> usize {
    let mut low = 0_usize;
    let mut high = 10 * 1024 * 1024;
    assert!(admits(&probe_history_budget(high)), "default budget admits");
    while low + 1 < high {
        let middle = low + (high - low) / 2;
        if admits(&probe_history_budget(middle)) {
            high = middle;
        } else {
            low = middle;
        }
    }
    high
}

#[test]
fn should_bound_retained_history_at_the_configured_budget() {
    // Arrange
    let capture_budget = smallest_budget(|probe| probe.owner_captured);
    let commit_budget = smallest_budget(|probe| probe.owner_captured && probe.competing.is_ok());

    // Act
    let below_capture = probe_history_budget(capture_budget - 1);
    let below_commit = probe_history_budget(commit_budget - 1);
    let at_commit = probe_history_budget(commit_budget);

    // Assert
    assert!(!below_capture.owner_captured);
    assert!(
        commit_budget > capture_budget,
        "recording competing keys needs more than the capture header"
    );
    assert!(below_commit.owner_captured);
    let error = below_commit
        .competing
        .expect_err("history growth above the budget rejects the competing commit");
    assert!(
        matches!(error, CassieError::ResourceLimit(_)),
        "resource-limit errors map to SQLSTATE 54000"
    );
    assert_eq!(
        below_commit.rows_after,
        Some(0),
        "a rejected history admission publishes no rows"
    );
    assert!(at_commit.competing.is_ok());
    assert_eq!(at_commit.rows_after, Some(3));
}
