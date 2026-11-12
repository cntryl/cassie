//! Required transaction output is admitted before changing transaction state.
use std::time::Instant;

use super::*;
use crate::config::{CassieRuntimeConfig, CassieRuntimeLimits};
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};
use crate::types::Value;

thread_local! {
    static CANCEL_BEFORE_INDEX_GUARD: std::cell::RefCell<Option<QueryCancellationHandle>> = std::cell::RefCell::new(None);
}

pub(super) fn cancel_before_index_guard_if_armed() {
    if let Some(cancellation) = CANCEL_BEFORE_INDEX_GUARD.with(|slot| slot.borrow_mut().take()) {
        cancellation.cancel();
    }
}

#[test]
fn should_fail_transaction_when_index_commit_guard_is_cancelled() {
    // Arrange
    let fixture = Fixture::new();
    let session = fixture.cassie.create_session("tester", None);
    for sql in [
        "CREATE TABLE guarded_commit (id INT)",
        "BEGIN",
        "CREATE INDEX guarded_idx ON guarded_commit (id)",
    ] {
        fixture
            .cassie
            .execute_sql(&session, sql, Vec::new())
            .expect("setup");
    }
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    CANCEL_BEFORE_INDEX_GUARD
        .with(|slot| assert!(slot.borrow_mut().replace(cancellation).is_none()));

    // Act
    let result = fixture
        .cassie
        .commit_transaction_with_controls(&session, &controls);

    // Assert
    assert!(matches!(result, Err(CassieError::QueryCancelled)));
    assert_eq!(session.transaction_status(), "failed");
    assert_eq!(controls.current_query_memory_bytes(), 0);
    session.rollback_transaction();
    assert!(fixture
        .cassie
        .midge
        .get_index("guarded_commit", "guarded_idx")
        .expect("read")
        .is_none());
}

struct Fixture {
    cassie: Cassie,
}

impl Fixture {
    fn new() -> Self {
        let cassie = Cassie::new_in_memory_with_config(CassieRuntimeConfig::default())
            .expect("ephemeral transaction fixture");
        cassie.startup().expect("startup");
        Self { cassie }
    }

    fn repeatable(
        &self,
        cancellation: QueryCancellationHandle,
    ) -> (CassieSession, QueryExecutionControls) {
        let session = self.cassie.create_session("tester", None);
        for sql in [
            "CREATE TABLE rr_commit_rows(id INT PRIMARY KEY, v INT)",
            "INSERT INTO rr_commit_rows VALUES (1, 1)",
            "BEGIN ISOLATION LEVEL REPEATABLE READ READ WRITE",
        ] {
            self.cassie
                .execute_sql(&session, sql, Vec::new())
                .expect("repeatable write fixture");
        }
        let origin = QueryExecutionControls::with_cancellation(
            &CassieRuntimeLimits::default(),
            Instant::now(),
            cancellation,
        );
        let parsed = crate::sql::parse_statement("SELECT 1").expect("snapshot-taking SELECT");
        self.cassie
            .prepare_transaction_read_view(&session, &parsed, &origin)
            .expect("retained first statement");
        self.cassie
            .execute_sql(
                &session,
                "UPDATE rr_commit_rows SET v = 2 WHERE id = 1",
                Vec::new(),
            )
            .expect("own staged replacement");
        assert!(origin.current_query_memory_bytes() > 0);
        (session, origin)
    }

    fn denied(
        &self,
        session: &CassieSession,
        action: TransactionAction,
    ) -> (Result<QueryResult, CassieError>, QueryExecutionControls) {
        let controls = QueryExecutionControls::from_limits(
            &CassieRuntimeLimits {
                query_memory_budget_bytes: 0,
                ..CassieRuntimeLimits::default()
            },
            Instant::now(),
        );
        let result = self.cassie.execute_transaction_statement(
            session,
            &TransactionStatement {
                action,
                isolation: None,
                modes: crate::sql::ast::TransactionModes::default(),
                chain: false,
            },
            &controls,
        );
        (result, controls)
    }
}

fn assert_denied(result: Result<QueryResult, CassieError>, controls: &QueryExecutionControls) {
    let Err(error @ CassieError::ResourceLimit(_)) = result else {
        panic!("required transaction output must return structured memory denial");
    };
    assert_eq!(error.descriptor().sql_state, "54000");
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_admit_transaction_output_before_beginning() {
    // Arrange
    let fixture = Fixture::new();
    let session = fixture.cassie.create_session("root", None);

    // Act
    let (result, controls) = fixture.denied(&session, TransactionAction::Begin);
    let status = session.transaction_status();
    session.rollback_transaction();
    drop((session, fixture));

    // Assert
    assert_denied(result, &controls);
    assert_eq!(status, "idle");
}

#[test]
fn should_admit_transaction_output_before_committing_staged_rows() {
    // Arrange
    let fixture = Fixture::new();
    let writer = fixture.cassie.create_session("root", None);
    let observer = fixture.cassie.create_session("root", None);
    let execute = |session: &CassieSession, sql: &str| {
        fixture
            .cassie
            .execute_sql(session, sql, vec![])
            .expect("SQL")
    };
    execute(&writer, "CREATE TABLE admission_rows(id BIGINT)");
    execute(&writer, "BEGIN");
    execute(&writer, "INSERT INTO admission_rows VALUES (1)");
    assert_eq!(
        execute(&writer, "SELECT id FROM admission_rows").rows(),
        &[vec![Value::Int64(1)]]
    );
    let empty: &[Vec<Value>] = &[];
    assert_eq!(
        execute(&observer, "SELECT id FROM admission_rows").rows(),
        empty
    );

    // Act
    let (result, controls) = fixture.denied(&writer, TransactionAction::Commit);
    let status = writer.transaction_status();
    let visible = execute(&observer, "SELECT id FROM admission_rows")
        .rows()
        .to_vec();
    writer.rollback_transaction();
    drop((writer, observer, fixture));

    // Assert
    assert_denied(result, &controls);
    assert_eq!(status, "in_transaction");
    assert_eq!(visible, Vec::<Vec<Value>>::new());
}

#[test]
fn should_admit_transaction_output_before_rolling_back() {
    // Arrange
    let fixture = Fixture::new();
    let session = fixture.cassie.create_session("root", None);
    session
        .set_setting("application_name", "before")
        .expect("initial setting");
    session.begin_transaction(None).expect("valid transaction");
    session
        .set_setting("application_name", "after")
        .expect("transaction setting");

    // Act
    let (result, controls) = fixture.denied(&session, TransactionAction::Rollback);
    let status = session.transaction_status();
    let setting = session
        .setting("application_name")
        .expect("setting readback");
    session.rollback_transaction();
    drop((session, fixture));

    // Assert
    assert_denied(result, &controls);
    assert_eq!(status, "in_transaction");
    assert_eq!(setting, "after");
}

#[test]
fn should_admit_transaction_output_before_creating_a_savepoint() {
    // Arrange
    let fixture = Fixture::new();
    let session = fixture.cassie.create_session("root", None);
    session.begin_transaction(None).expect("valid transaction");

    // Act
    let (result, controls) = fixture.denied(
        &session,
        TransactionAction::Savepoint {
            name: "denied".to_owned(),
        },
    );
    let created = session.release_savepoint("denied").is_ok();
    session.rollback_transaction();
    drop((session, fixture));

    // Assert
    assert_denied(result, &controls);
    assert!(!created, "denied command must not create a savepoint");
}

#[test]
fn should_admit_transaction_output_before_restoring_a_savepoint() {
    // Arrange
    let fixture = Fixture::new();
    let session = fixture.cassie.create_session("root", None);
    session.begin_transaction(None).expect("valid transaction");
    session
        .set_setting("application_name", "saved")
        .expect("saved setting");
    session.create_savepoint("kept").expect("valid savepoint");
    session
        .set_setting("application_name", "after")
        .expect("later setting");

    // Act
    let (result, controls) = fixture.denied(
        &session,
        TransactionAction::RollbackTo {
            name: "kept".to_owned(),
        },
    );
    let status = session.transaction_status();
    let setting = session
        .setting("application_name")
        .expect("setting readback");
    session.rollback_transaction();
    drop((session, fixture));

    // Assert
    assert_denied(result, &controls);
    assert_eq!(status, "in_transaction");
    assert_eq!(setting, "after");
}

#[test]
fn should_admit_transaction_output_before_releasing_a_savepoint() {
    // Arrange
    let fixture = Fixture::new();
    let session = fixture.cassie.create_session("root", None);
    session.begin_transaction(None).expect("valid transaction");
    session
        .set_setting("application_name", "saved")
        .expect("saved setting");
    session.create_savepoint("kept").expect("valid savepoint");
    session
        .set_setting("application_name", "after")
        .expect("later setting");

    // Act
    let (result, controls) = fixture.denied(
        &session,
        TransactionAction::Release {
            name: "kept".to_owned(),
        },
    );
    let retained = session.rollback_to_savepoint("kept").is_ok();
    let setting = session
        .setting("application_name")
        .expect("setting readback");
    session.rollback_transaction();
    drop((session, fixture));

    // Assert
    assert_denied(result, &controls);
    assert!(retained, "denied command must preserve its savepoint");
    assert_eq!(setting, "saved");
}

#[test]
fn should_retire_repeatable_writes_after_commit_output_admission_fails() {
    // Arrange
    let fixture = Fixture::new();
    let (session, origin) = fixture.repeatable(QueryCancellationHandle::new());

    // Act
    let (result, controls) = fixture.denied(&session, TransactionAction::Commit);
    let status = session.transaction_status();

    // Assert
    assert_denied(result, &controls);
    assert_eq!(status, "idle");
    assert_eq!(origin.current_query_memory_bytes(), 0);
    assert_eq!(
        fixture
            .cassie
            .execute_sql(&session, "SELECT v FROM rr_commit_rows", Vec::new())
            .expect("fresh unchanged value")
            .rows(),
        &[vec![Value::Int64(1)]]
    );
}

#[test]
fn should_retire_repeatable_writes_after_commit_input_admission_fails() {
    // Arrange
    let fixture = Fixture::new();
    let (session, origin) = fixture.repeatable(QueryCancellationHandle::new());
    let current = QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: 64,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    );

    // Act
    let result = fixture
        .cassie
        .commit_transaction_with_controls(&session, &current);

    // Assert
    assert!(matches!(result, Err(CassieError::ResourceLimit(_))));
    assert_eq!(session.transaction_status(), "idle");
    assert_eq!(origin.current_query_memory_bytes(), 0);
    assert_eq!(current.current_query_memory_bytes(), 0);
    assert_eq!(
        fixture
            .cassie
            .execute_sql(&session, "SELECT v FROM rr_commit_rows", Vec::new())
            .expect("fresh unchanged value")
            .rows(),
        &[vec![Value::Int64(1)]]
    );
}

#[test]
fn should_retire_repeatable_writes_after_a_same_payload_conflict() {
    // Arrange
    let fixture = Fixture::new();
    let (session, origin) = fixture.repeatable(QueryCancellationHandle::new());
    let row = fixture
        .cassie
        .midge
        .scan_documents("rr_commit_rows")
        .expect("committed row")
        .into_iter()
        .next()
        .expect("one row");
    fixture
        .cassie
        .midge
        .put_document(
            "rr_commit_rows",
            Some(row.id),
            serde_json::json!({"id":1,"v":1}),
        )
        .expect("same-payload competing native publication");
    let current = fixture.cassie.runtime.query_controls(Instant::now());

    // Act
    let result = fixture
        .cassie
        .commit_transaction_with_controls(&session, &current);

    // Assert
    assert_eq!(
        result
            .expect_err("native old-start FCW")
            .descriptor()
            .sql_state,
        "40001"
    );
    assert_eq!(session.transaction_status(), "idle");
    assert_eq!(origin.current_query_memory_bytes(), 0);
    assert_eq!(current.current_query_memory_bytes(), 0);
    assert_eq!(
        fixture
            .cassie
            .execute_sql(&session, "SELECT v FROM rr_commit_rows", Vec::new())
            .expect("competing value survives")
            .rows(),
        &[vec![Value::Int64(1)]]
    );
}

#[test]
fn should_ignore_the_retired_capture_requests_cancellation_during_commit() {
    // Arrange
    let fixture = Fixture::new();
    let canceled_origin = QueryCancellationHandle::new();
    let (session, origin) = fixture.repeatable(canceled_origin.clone());
    canceled_origin.cancel();
    let current = fixture.cassie.runtime.query_controls(Instant::now());

    // Act
    let result = fixture
        .cassie
        .commit_transaction_with_controls(&session, &current);

    // Assert
    assert!(result.is_ok());
    assert_eq!(session.transaction_status(), "idle");
    assert_eq!(origin.current_query_memory_bytes(), 0);
    assert_eq!(current.current_query_memory_bytes(), 0);
    assert_eq!(
        fixture
            .cassie
            .execute_sql(&session, "SELECT v FROM rr_commit_rows", Vec::new())
            .expect("confirmed own replacement")
            .rows(),
        &[vec![Value::Int64(2)]]
    );
}

#[test]
fn should_obey_the_current_commit_requests_cancellation() {
    // Arrange
    let fixture = Fixture::new();
    let (session, origin) = fixture.repeatable(QueryCancellationHandle::new());
    let canceled_current = QueryCancellationHandle::new();
    let current = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        canceled_current.clone(),
    );
    canceled_current.cancel();

    // Act
    let result = fixture
        .cassie
        .commit_transaction_with_controls(&session, &current);

    // Assert
    assert!(matches!(result, Err(CassieError::QueryCanceled)));
    assert_eq!(session.transaction_status(), "idle");
    assert_eq!(origin.current_query_memory_bytes(), 0);
    assert_eq!(current.current_query_memory_bytes(), 0);
    assert_eq!(
        fixture
            .cassie
            .execute_sql(&session, "SELECT v FROM rr_commit_rows", Vec::new())
            .expect("unpublished own replacement")
            .rows(),
        &[vec![Value::Int64(1)]]
    );
}

#[test]
fn should_preserve_confirmed_commit_success_after_request_cancellation() {
    // Arrange
    let fixture = Fixture::new();
    let (session, origin) = fixture.repeatable(QueryCancellationHandle::new());
    let cancellation = QueryCancellationHandle::new();
    let current = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let confirmed = std::rc::Rc::new(std::cell::Cell::new(false));
    let observed = std::rc::Rc::clone(&confirmed);
    let cancel_after_native_ok = cancellation.clone();
    let _observer = crate::midge::Midge::observe_next_row_commit_for_test(move || {
        observed.set(true);
        cancel_after_native_ok.cancel();
    });

    // Act
    let result = fixture
        .cassie
        .commit_transaction_with_controls(&session, &current);

    // Assert
    assert!(confirmed.get());
    assert!(cancellation.is_canceled());
    assert!(result.is_ok());
    assert_eq!(session.transaction_status(), "idle");
    assert_eq!(origin.current_query_memory_bytes(), 0);
    assert_eq!(current.current_query_memory_bytes(), 0);
    assert_eq!(
        fixture
            .cassie
            .execute_sql(&session, "SELECT v FROM rr_commit_rows", Vec::new())
            .expect("confirmed replacement remains")
            .rows(),
        &[vec![Value::Int64(2)]]
    );
}
