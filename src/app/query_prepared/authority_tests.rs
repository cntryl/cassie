//! Focused pgwire worker failure-authority regressions.

use super::*;
use crate::app::FailureAuthority;
use crate::config::CassieRuntimeConfig;
use std::sync::mpsc;
use std::time::Duration;

fn fixture() -> (Cassie, CassieSession) {
    let cassie =
        Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).expect("memory fixture");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    (cassie, session)
}

fn execute_statement(
    cassie: &Cassie,
    session: &CassieSession,
    sql: &str,
    controls: &QueryExecutionControls,
    authority: Option<&FailureAuthority>,
) -> Result<QueryResult, CassieError> {
    let parsed = crate::sql::parse_statement(sql).expect("parse statement");
    let fingerprint = crate::runtime::sql_fingerprint(&parsed);
    cassie.execute_pgwire_statement_core_for_operation(
        session,
        parsed,
        fingerprint,
        (Vec::new(), &[]),
        ExecutionMode::SimpleQuery,
        (controls, authority),
    )
}

#[test]
fn should_keep_replacement_healthy_after_old_worker_error() {
    const CASE: &str = "private_stale_worker";
    if !crate::app::query_failure_watchdog::enter_child(
        "app::query_prepared::authority_tests::should_keep_replacement_healthy_after_old_worker_error", CASE,
    ) {
        return;
    }
    {
        // Arrange: A's actual bound statement errors before its completion pauses.
        let (cassie, session) = fixture();
        session.begin_transaction(None).expect("begin A");
        let owner_a = session.transaction_unique_owner_id();
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let (resume_tx, resume_rx) = mpsc::sync_channel(1);
        let old_cassie = cassie.clone();
        let old_session = session.clone();
        let worker = std::thread::spawn(move || {
            old_cassie.execute_pgwire_query(
                &old_session,
                &QueryCancellationHandle::new(),
                |controls, authority| {
                    let result = execute_statement(
                        &old_cassie,
                        &old_session,
                        "SELECT 1 / 0",
                        controls,
                        authority,
                    );
                    ready_tx.send(()).expect("worker completed statement");
                    resume_rx
                        .recv_timeout(Duration::from_secs(5))
                        .expect("resume worker");
                    result
                },
            )
        });
        ready_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("old error reached completion");
        session.rollback_transaction();
        session.begin_transaction(None).expect("begin B");
        let owner_b = session.transaction_unique_owner_id();

        // Act
        resume_tx.send(()).expect("resume worker");
        let result = worker.join().expect("worker");
        let healthy = session.is_transaction_active() && !session.is_transaction_failed();
        let current = session.transaction_unique_owner_id();
        let usable = cassie.execute_sql(&session, "SELECT 1", vec![]);
        session.rollback_transaction();

        // Assert
        assert!(
            matches!(result, Err(CassieError::Execution(ref message)) if message == "division by zero")
        );
        assert_ne!(owner_a, owner_b);
        assert_eq!(current, owner_b);
        assert!(healthy);
        assert!(usable.is_ok());
    }
    crate::app::query_failure_watchdog::complete_child(CASE);
}

#[test]
fn should_fail_current_owner_after_worker_error() {
    // Arrange
    let (cassie, session) = fixture();
    session.begin_transaction(None).expect("begin");
    let owner = session.transaction_unique_owner_id();

    // Act
    let result = cassie.execute_pgwire_query(
        &session,
        &QueryCancellationHandle::new(),
        |controls, authority| {
            execute_statement(&cassie, &session, "SELECT 1 / 0", controls, authority)
        },
    );
    let failed = session.is_transaction_failed();
    let current = session.transaction_unique_owner_id();
    session.rollback_transaction();

    // Assert
    assert!(
        matches!(result, Err(CassieError::Execution(ref message)) if message == "division by zero")
    );
    assert!(failed);
    assert_eq!(current, owner);
}

#[test]
fn should_fail_transaction_started_by_worker_on_later_worker_error() {
    // Arrange
    let (cassie, session) = fixture();

    // Act: inject completion error only after the real BEGIN dispatch succeeds.
    let result = cassie.execute_pgwire_query(
        &session,
        &QueryCancellationHandle::new(),
        |controls, authority| {
            execute_statement(&cassie, &session, "BEGIN", controls, authority)?;
            Err(CassieError::Execution("after begin".into()))
        },
    );
    let failed = session.is_transaction_failed();
    session.rollback_transaction();

    // Assert
    assert!(matches!(result, Err(CassieError::Execution(ref message)) if message == "after begin"));
    assert!(failed);
}

#[test]
fn should_fail_recovered_epoch_after_own_savepoint_rollback_error() {
    // Arrange
    let (cassie, session) = fixture();
    session.begin_transaction(None).expect("begin");
    cassie
        .execute_sql(&session, "SAVEPOINT s", vec![])
        .expect("savepoint");
    let owner = session.transaction_unique_owner_id();
    let before = session.rollback_epoch_for_test();

    // Act: the real rollback advances the epoch before the simulated later error.
    let result = cassie.execute_pgwire_query(
        &session,
        &QueryCancellationHandle::new(),
        |controls, authority| {
            execute_statement(
                &cassie,
                &session,
                "ROLLBACK TO SAVEPOINT s",
                controls,
                authority,
            )?;
            Err(CassieError::Execution("after rollback to".into()))
        },
    );
    let after = session.rollback_epoch_for_test();
    let current = session.transaction_unique_owner_id();
    let failed = session.is_transaction_failed();
    session.rollback_transaction();

    // Assert
    assert!(
        matches!(result, Err(CassieError::Execution(ref message)) if message == "after rollback to")
    );
    assert_eq!(current, owner);
    assert_eq!(after, before + 1);
    assert!(failed);
}

#[test]
fn should_keep_recovered_epoch_healthy_after_old_worker_error() {
    const CASE: &str = "private_stale_epoch";
    if !crate::app::query_failure_watchdog::enter_child(
        "app::query_prepared::authority_tests::should_keep_recovered_epoch_healthy_after_old_worker_error", CASE,
    ) {
        return;
    }
    {
        // Arrange: the old worker finishes its bound statement in the first epoch.
        let (cassie, session) = fixture();
        session.begin_transaction(None).expect("begin");
        cassie
            .execute_sql(&session, "SAVEPOINT s", vec![])
            .expect("savepoint");
        let owner = session.transaction_unique_owner_id();
        let epoch = session.rollback_epoch_for_test();
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let (resume_tx, resume_rx) = mpsc::sync_channel(1);
        let old_cassie = cassie.clone();
        let old_session = session.clone();
        let worker = std::thread::spawn(move || {
            old_cassie.execute_pgwire_query(
                &old_session,
                &QueryCancellationHandle::new(),
                |controls, authority| {
                    let result = execute_statement(
                        &old_cassie,
                        &old_session,
                        "SELECT 1 / 0",
                        controls,
                        authority,
                    );
                    ready_tx.send(()).expect("worker completed statement");
                    resume_rx
                        .recv_timeout(Duration::from_secs(5))
                        .expect("resume worker");
                    result
                },
            )
        });
        ready_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("old error reached completion");
        cassie
            .execute_sql(&session, "ROLLBACK TO SAVEPOINT s", vec![])
            .expect("restore savepoint");
        let recovered_epoch = session.rollback_epoch_for_test();

        // Act
        resume_tx.send(()).expect("resume worker");
        let result = worker.join().expect("worker");
        let current = session.transaction_unique_owner_id();
        let healthy = session.is_transaction_active() && !session.is_transaction_failed();
        let usable = cassie.execute_sql(&session, "SELECT 1", vec![]);
        session.rollback_transaction();

        // Assert
        assert!(
            matches!(result, Err(CassieError::Execution(ref message)) if message == "division by zero")
        );
        assert_eq!(current, owner);
        assert_eq!(recovered_epoch, epoch + 1);
        assert!(healthy);
        assert!(usable.is_ok());
    }
    crate::app::query_failure_watchdog::complete_child(CASE);
}

#[test]
fn should_preserve_independent_implicit_owner_after_worker_promotion() {
    const CASE: &str = "private_promotion";
    if !crate::app::query_failure_watchdog::enter_child(
        "app::query_prepared::authority_tests::should_preserve_independent_implicit_owner_after_worker_promotion", CASE,
    ) {
        return;
    }
    {
        // Arrange: the closure begins only after this worker's actual Idle capture.
        let (cassie, session) = fixture();
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let (resume_tx, resume_rx) = mpsc::sync_channel(1);
        let old_cassie = cassie.clone();
        let old_session = session.clone();
        let worker = std::thread::spawn(move || {
            old_cassie.execute_pgwire_query(
                &old_session,
                &QueryCancellationHandle::new(),
                |controls, authority| {
                    let captured = authority
                        .expect("healthy worker authority")
                        .owner_for_test();
                    ready_tx.send(captured).expect("worker captured authority");
                    resume_rx
                        .recv_timeout(Duration::from_secs(5))
                        .expect("resume worker");
                    execute_statement(&old_cassie, &old_session, "BEGIN", controls, authority)?;
                    Err(CassieError::Execution("after promotion".into()))
                },
            )
        });
        let captured = ready_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("worker capture");
        session
            .begin_implicit_transaction()
            .expect("independent implicit B");
        let owner = session.transaction_unique_owner_id();

        // Act
        resume_tx.send(()).expect("resume promotion");
        let result = worker.join().expect("worker");
        let current = session.transaction_unique_owner_id();
        let healthy = session.is_transaction_active() && !session.is_transaction_failed();
        let explicit = !session.is_implicit_transaction();
        session.rollback_transaction();

        // Assert: promotion itself remains existing behavior; only completion is scoped.
        assert_eq!(captured, None);
        assert!(
            matches!(result, Err(CassieError::Execution(ref message)) if message == "after promotion")
        );
        assert_eq!(current, owner);
        assert!(healthy);
        assert!(explicit);
    }
    crate::app::query_failure_watchdog::complete_child(CASE);
}

#[test]
fn should_fail_current_owner_when_pgwire_worker_is_canceled() {
    // Arrange
    let (cassie, session) = fixture();
    session.begin_transaction(None).expect("begin");
    let cancellation = QueryCancellationHandle::new();
    cancellation.cancel();

    // Act: use the real text pgwire entry point and its existing checks.
    let result = cassie.execute_pgwire_sql_with_cancellation(
        &session,
        ("SELECT 1", "SELECT 1"),
        &cancellation,
    );
    let failed = session.is_transaction_failed();
    session.rollback_transaction();

    // Assert
    assert!(matches!(result, Err(CassieError::QueryCanceled)));
    assert!(failed);
}

#[test]
fn should_preserve_pgwire_admission_exhaustion_before_cancellation() {
    // Arrange
    let mut config = CassieRuntimeConfig::default();
    config.limits.max_query_workers = 1;
    let cassie = Cassie::new_in_memory_with_config(config).expect("memory fixture");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    session.begin_transaction(None).expect("begin");
    let occupied = cassie
        .runtime
        .try_begin_running_query()
        .expect("occupy only slot");
    let cancellation = QueryCancellationHandle::new();
    cancellation.cancel();

    // Act
    let result = cassie.execute_pgwire_sql_with_cancellation(
        &session,
        ("SELECT 1", "SELECT 1"),
        &cancellation,
    );
    let healthy = session.is_transaction_active() && !session.is_transaction_failed();
    drop(occupied);
    session.rollback_transaction();

    // Assert
    assert!(
        matches!(result, Err(CassieError::Execution(ref message)) if message == "query admission exhausted")
    );
    assert!(healthy);
}

#[test]
fn should_preserve_pgwire_quarantine_error_before_cancellation() {
    // Arrange
    let (cassie, session) = fixture();
    session.begin_transaction(None).expect("begin");
    cassie.midge.latch_storage_recovery_required_for_test();
    let cancellation = QueryCancellationHandle::new();
    cancellation.cancel();

    // Act
    let result = cassie.execute_pgwire_sql_with_cancellation(
        &session,
        ("SELECT 1", "SELECT 1"),
        &cancellation,
    );
    let failed = session.is_transaction_failed();
    session.rollback_transaction();

    // Assert
    assert!(
        matches!(result, Err(CassieError::Storage(ref message)) if message == "storage recovery required after an earlier unconfirmed write; this operation was not submitted; recover and reconcile before retrying")
    );
    assert!(failed);
}

#[derive(Clone, Copy)]
enum PgwireEntry {
    Text,
    Parsed,
}

fn run_real_entry(
    cassie: &Cassie,
    session: &CassieSession,
    entry: PgwireEntry,
) -> Result<QueryResult, CassieError> {
    run_entry_sql(cassie, session, entry, "SELECT 1 / 0")
}

fn run_entry_sql(
    cassie: &Cassie,
    session: &CassieSession,
    entry: PgwireEntry,
    sql: &str,
) -> Result<QueryResult, CassieError> {
    let cancellation = QueryCancellationHandle::new();
    match entry {
        PgwireEntry::Text => {
            cassie.execute_pgwire_sql_with_cancellation(session, (sql, sql), &cancellation)
        }
        PgwireEntry::Parsed => {
            let parsed = crate::sql::parse_statement(sql).expect("parse statement");
            let fingerprint = crate::runtime::sql_fingerprint(&parsed);
            cassie.execute_parsed_sql_with_cancellation(
                session,
                (parsed, sql),
                fingerprint,
                (Vec::new(), &[]),
                ExecutionMode::ExtendedQuery,
                &cancellation,
            )
        }
    }
}

fn assert_real_entry_preserves_replacement(entry: PgwireEntry) {
    let (cassie, session) = fixture();
    session.begin_transaction(None).expect("begin A");
    let owner_a = session.transaction_unique_owner_id();
    let epoch_a = session.rollback_epoch_for_test();
    let (ready_tx, ready_rx) = mpsc::sync_channel(1);
    let (resume_tx, resume_rx) = mpsc::sync_channel(1);
    let old_cassie = cassie.clone();
    let old_session = session.clone();
    let worker = std::thread::spawn(move || {
        // This scoped hook must report the actual worker error and captured
        // authority after execution and before its failure completion.
        let _hook = super::authority_test_hooks::install_after_worker_error(
            &old_cassie,
            &old_session,
            ready_tx,
            resume_rx,
        );
        run_real_entry(&old_cassie, &old_session, entry)
    });
    let captured = ready_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("actual worker error");
    session.rollback_transaction();
    session.begin_transaction(None).expect("begin B");
    let owner_b = session.transaction_unique_owner_id();
    resume_tx.send(()).expect("resume worker completion");
    let result = worker.join().expect("worker");
    let current = session.transaction_unique_owner_id();
    let healthy = session.is_transaction_active() && !session.is_transaction_failed();
    let usable = cassie.execute_sql(&session, "SELECT 1", vec![]);
    session.rollback_transaction();
    assert_eq!(captured, Some((owner_a, epoch_a)));
    assert!(
        matches!(result, Err(CassieError::Execution(ref message)) if message == "division by zero")
    );
    assert_ne!(owner_a, owner_b);
    assert_eq!(current, owner_b);
    assert!(healthy);
    assert!(usable.is_ok());
}

#[test]
fn should_preserve_replacement_after_text_pgwire_worker_error() {
    const CASE: &str = "text_stale_worker";
    if !crate::app::query_failure_watchdog::enter_child(
        "app::query_prepared::authority_tests::should_preserve_replacement_after_text_pgwire_worker_error", CASE,
    ) {
        return;
    }
    {
        // Arrange: the helper creates and later replaces transaction A.
        // Act: run the real text worker and release its observed error.
        // Assert: the helper verifies B survives with the expected error returned.
        assert_real_entry_preserves_replacement(PgwireEntry::Text);
    }
    crate::app::query_failure_watchdog::complete_child(CASE);
}

#[test]
fn should_preserve_replacement_after_parsed_pgwire_worker_error() {
    const CASE: &str = "parsed_stale_worker";
    if !crate::app::query_failure_watchdog::enter_child(
        "app::query_prepared::authority_tests::should_preserve_replacement_after_parsed_pgwire_worker_error", CASE,
    ) {
        return;
    }
    {
        // Arrange: the helper creates and later replaces transaction A.
        // Act: run the real parsed worker and release its observed error.
        // Assert: the helper verifies B survives with the expected error returned.
        assert_real_entry_preserves_replacement(PgwireEntry::Parsed);
    }
    crate::app::query_failure_watchdog::complete_child(CASE);
}

fn assert_real_entry_fails_current_owner(entry: PgwireEntry) {
    let (cassie, session) = fixture();
    session.begin_transaction(None).expect("begin");
    let owner = session.transaction_unique_owner_id();
    let result = run_real_entry(&cassie, &session, entry);
    let failed = session.is_transaction_failed();
    let current = session.transaction_unique_owner_id();
    session.rollback_transaction();
    assert!(
        matches!(result, Err(CassieError::Execution(ref message)) if message == "division by zero")
    );
    assert_eq!(current, owner);
    assert!(failed);
}

#[test]
fn should_fail_current_owner_after_text_pgwire_worker_error() {
    // Arrange: the helper starts a transaction.
    // Act: run the real text worker with an erroring statement.
    // Assert: the helper verifies the same owner is failed.
    assert_real_entry_fails_current_owner(PgwireEntry::Text);
}

#[test]
fn should_fail_current_owner_after_parsed_pgwire_worker_error() {
    // Arrange: the helper starts a transaction.
    // Act: run the real parsed worker with an erroring statement.
    // Assert: the helper verifies the same owner is failed.
    assert_real_entry_fails_current_owner(PgwireEntry::Parsed);
}

fn assert_worker_transition(
    entry: PgwireEntry,
    setup: Option<&str>,
    sql: &str,
    expected_new_owner: bool,
    expected_epoch_advance: bool,
) {
    let (cassie, session) = fixture();
    if let Some(setup) = setup {
        session.begin_transaction(None).expect("begin setup");
        if !setup.is_empty() {
            cassie
                .execute_sql(&session, setup, vec![])
                .expect("setup statement");
        }
    }
    let old_owner = session.transaction_unique_owner_id();
    let old_epoch = session.rollback_epoch_for_test();
    let (observed_tx, observed_rx) = mpsc::sync_channel(1);
    let _hook =
        super::authority_test_hooks::install_after_worker_result(&cassie, &session, observed_tx);
    let result = run_entry_sql(&cassie, &session, entry, sql);
    let observed = observed_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("actual worker result");
    let new_owner = session.transaction_unique_owner_id();
    let new_epoch = session.rollback_epoch_for_test();
    let healthy = session.is_transaction_active() && !session.is_transaction_failed();
    session.rollback_transaction();
    assert!(result.is_ok());
    assert_eq!(observed, (Some((new_owner, new_epoch)), true));
    if expected_new_owner {
        assert_ne!(old_owner, new_owner);
    } else {
        assert_eq!(old_owner, new_owner);
    }
    if expected_epoch_advance {
        assert_eq!(new_epoch, old_epoch + 1);
    }
    assert!(healthy);
}

#[test]
fn should_adopt_own_begin_in_real_pgwire_worker() {
    // Arrange: each helper starts with an idle session.
    // Act: each real worker dispatches BEGIN.
    // Assert: the observed worker authority matches the new owner.
    for entry in [PgwireEntry::Text, PgwireEntry::Parsed] {
        assert_worker_transition(entry, None, "BEGIN", true, false);
    }
}

#[test]
fn should_adopt_own_chain_in_real_pgwire_worker() {
    // Arrange: each helper starts an explicit transaction.
    // Act: each real worker dispatches COMMIT AND CHAIN.
    // Assert: the observed worker authority matches the replacement owner.
    for entry in [PgwireEntry::Text, PgwireEntry::Parsed] {
        assert_worker_transition(entry, Some(""), "COMMIT AND CHAIN", true, false);
    }
}

#[test]
fn should_adopt_own_savepoint_restore_in_real_pgwire_worker() {
    // Arrange: each helper starts a transaction with a savepoint.
    // Act: each real worker rolls back to that savepoint.
    // Assert: observed authority retains the owner and the advanced epoch.
    for entry in [PgwireEntry::Text, PgwireEntry::Parsed] {
        assert_worker_transition(
            entry,
            Some("SAVEPOINT s"),
            "ROLLBACK TO SAVEPOINT s",
            false,
            true,
        );
    }
}

#[test]
fn should_forward_pgwire_authority_to_idle_procedure() {
    // Arrange: CALL is supported without an active transaction.
    let (cassie, session) = fixture();
    cassie
        .execute_sql(
            &session,
            r#"CREATE PROCEDURE worker_error() AS "SELECT 1 / 0""#,
            vec![],
        )
        .expect("create procedure");
    let (observed_tx, observed_rx) = mpsc::sync_channel(1);
    let _hook = super::authority_test_hooks::install_on_nested(&cassie, &session, observed_tx);

    // Act
    let result = run_entry_sql(&cassie, &session, PgwireEntry::Text, "CALL worker_error()");
    let observed = observed_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("nested operation authority");
    let status = session.transaction_status();

    // Assert: a present authority with no owner differs from no authority.
    assert!(
        matches!(&result, Err(CassieError::Execution(message)) if message == "execution error: division by zero"),
        "nested result: {result:?}"
    );
    assert_eq!(observed, (true, None));
    assert_eq!(status, "idle");
}

#[test]
fn should_reject_procedure_call_in_active_transaction() {
    // Arrange
    let (cassie, session) = fixture();
    cassie
        .execute_sql(
            &session,
            r#"CREATE PROCEDURE worker_error() AS "SELECT 1 / 0""#,
            vec![],
        )
        .expect("create procedure");
    session.begin_transaction(None).expect("begin");
    let owner = session.transaction_unique_owner_id();

    // Act
    let result = run_entry_sql(&cassie, &session, PgwireEntry::Text, "CALL worker_error()");
    let failed = session.is_transaction_failed();
    let current = session.transaction_unique_owner_id();
    session.rollback_transaction();

    // Assert
    assert!(
        matches!(&result, Err(CassieError::Unsupported(message)) if message == "this DDL or catalog operation is not supported inside an active transaction"),
        "active CALL result: {result:?}"
    );
    assert_eq!(current, owner);
    assert!(failed);
}

fn assert_timeout_snapshot_during_capture(initial: &str, changed: &str, expected_deadline: bool) {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    let mut config = CassieRuntimeConfig::default();
    config.limits.query_timeout_ms = 0;
    let cassie = Cassie::new_in_memory_with_config(config).expect("memory fixture");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    session
        .set_setting("statement_timeout", initial)
        .expect("initial timeout");
    session.begin_transaction(None).expect("begin");
    let (held_tx, held_rx) = mpsc::sync_channel(1);
    let (release_tx, release_rx) = mpsc::sync_channel(1);
    let held = Arc::new(AtomicBool::new(false));
    let holder_flag = Arc::clone(&held);
    let holding_session = session.clone();
    let holder = std::thread::spawn(move || {
        holding_session.hold_transaction_for_test(&held_tx, &release_rx, holder_flag);
    });
    held_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("root mutex held");
    let (contended_tx, contended_rx) = mpsc::sync_channel(1);
    let (deadline_tx, deadline_rx) = mpsc::sync_channel(1);
    let old_cassie = cassie.clone();
    let old_session = session.clone();
    let worker = std::thread::spawn(move || {
        // Future hook must signal on this capture's real failed try-lock.
        let _hook = super::authority_test_hooks::install_on_capture_contention(
            &old_cassie,
            &old_session,
            contended_tx,
        );
        old_cassie.execute_pgwire_query(
            &old_session,
            &QueryCancellationHandle::new(),
            |controls, authority| {
                deadline_tx
                    .send(controls.deadline.is_some())
                    .expect("worker controls deadline");
                execute_statement(&old_cassie, &old_session, "SELECT 1", controls, authority)
            },
        )
    });
    contended_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("actual capture contention");
    let still_held = held.load(Ordering::Acquire);
    session
        .set_setting("statement_timeout", changed)
        .expect("change timeout");
    release_tx.send(()).expect("release root mutex");
    holder.join().expect("holder");
    let actual_deadline = deadline_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("actual worker controls");
    let result = worker.join().expect("worker");
    session.rollback_transaction();
    assert!(still_held);
    assert_eq!(actual_deadline, expected_deadline);
    assert!(result.is_ok());
}

#[test]
fn should_preserve_disabled_pgwire_timeout_during_capture_wait() {
    const CASE: &str = "worker_timeout_disabled";
    if !crate::app::query_failure_watchdog::enter_child("app::query_prepared::authority_tests::should_preserve_disabled_pgwire_timeout_during_capture_wait", CASE) { return; }
    // Arrange: the helper holds the transaction mutex after setting timeout to zero.
    // Act: it changes the setting after the actual capture contends.
    // Assert: the worker controls retain the earlier disabled timeout.
    assert_timeout_snapshot_during_capture("0", "600000", false);
    crate::app::query_failure_watchdog::complete_child(CASE);
}

#[test]
fn should_preserve_enabled_pgwire_timeout_during_capture_wait() {
    const CASE: &str = "worker_timeout_enabled";
    if !crate::app::query_failure_watchdog::enter_child("app::query_prepared::authority_tests::should_preserve_enabled_pgwire_timeout_during_capture_wait", CASE) { return; }
    // Arrange: the helper holds the transaction mutex after setting a long timeout.
    // Act: it changes the setting after the actual capture contends.
    // Assert: the worker controls retain the earlier enabled timeout.
    assert_timeout_snapshot_during_capture("600000", "0", true);
    crate::app::query_failure_watchdog::complete_child(CASE);
}

#[test]
fn should_reject_saturated_pgwire_worker_before_waiting_for_transaction_lock() {
    const CASE: &str = "worker_saturated_lock";
    if !crate::app::query_failure_watchdog::enter_child("app::query_prepared::authority_tests::should_reject_saturated_pgwire_worker_before_waiting_for_transaction_lock", CASE) { return; }
    {
        // Arrange: occupy the only slot and hold the actual transaction mutex.
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;
        let mut config = CassieRuntimeConfig::default();
        config.limits.max_query_workers = 1;
        let cassie = Cassie::new_in_memory_with_config(config).expect("memory fixture");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        session.begin_transaction(None).expect("begin");
        let occupied = cassie
            .runtime
            .try_begin_running_query()
            .expect("occupy slot");
        let (held_tx, held_rx) = mpsc::sync_channel(1);
        let (release_tx, release_rx) = mpsc::sync_channel(1);
        let held = Arc::new(AtomicBool::new(false));
        let holder_flag = Arc::clone(&held);
        let holding_session = session.clone();
        let holder = std::thread::spawn(move || {
            holding_session.hold_transaction_for_test(&held_tx, &release_rx, holder_flag);
        });
        held_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("root mutex held");
        let (result_tx, result_rx) = mpsc::sync_channel(1);
        let request_cassie = cassie.clone();
        let request_session = session.clone();
        let request = std::thread::spawn(move || {
            let cancellation = QueryCancellationHandle::new();
            cancellation.cancel();
            let result = request_cassie.execute_pgwire_sql_with_cancellation(
                &request_session,
                ("SELECT 1", "SELECT 1"),
                &cancellation,
            );
            result_tx.send(result).expect("request result");
        });

        // Act: preserve lock ownership until the observed request result.
        let result = result_rx.recv_timeout(Duration::from_secs(5));
        let held_at_result = held.load(Ordering::Acquire);
        release_tx.send(()).expect("release root mutex");
        holder.join().expect("holder");
        request.join().expect("request");
        drop(occupied);
        session.rollback_transaction();

        // Assert
        assert!(
            matches!(result, Ok(Err(CassieError::Execution(ref message))) if message == "query admission exhausted")
        );
        assert!(held_at_result);
    }
    crate::app::query_failure_watchdog::complete_child(CASE);
}

#[test]
fn should_recheck_pgwire_recovery_when_saturated_after_healthy_check() {
    const CASE: &str = "worker_saturated_recovery";
    if !crate::app::query_failure_watchdog::enter_child("app::query_prepared::authority_tests::should_recheck_pgwire_recovery_when_saturated_after_healthy_check", CASE) { return; }
    {
        // Arrange: pause immediately after the actual first healthy check.
        let mut config = CassieRuntimeConfig::default();
        config.limits.max_query_workers = 1;
        let cassie = Cassie::new_in_memory_with_config(config).expect("memory fixture");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        session.begin_transaction(None).expect("begin");
        let occupied = cassie
            .runtime
            .try_begin_running_query()
            .expect("occupy slot");
        let (checked_tx, checked_rx) = mpsc::sync_channel(1);
        let (resume_tx, resume_rx) = mpsc::sync_channel(1);
        let old_cassie = cassie.clone();
        let old_session = session.clone();
        let worker = std::thread::spawn(move || {
            let _hook = super::authority_test_hooks::install_after_healthy_check(
                &old_cassie,
                &old_session,
                checked_tx,
                resume_rx,
            );
            old_cassie.execute_pgwire_sql_with_cancellation(
                &old_session,
                ("SELECT 1", "SELECT 1"),
                &QueryCancellationHandle::new(),
            )
        });
        checked_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("actual healthy check");

        // Act: latch recovery while the existing saturated admission recheck remains ahead.
        cassie.midge.latch_storage_recovery_required_for_test();
        resume_tx.send(()).expect("resume admission");
        let result = worker.join().expect("worker");
        let failed = session.is_transaction_failed();
        drop(occupied);
        session.rollback_transaction();

        // Assert
        assert!(
            matches!(result, Err(CassieError::Storage(ref message)) if message == "storage recovery required after an earlier unconfirmed write; this operation was not submitted; recover and reconcile before retrying")
        );
        assert!(failed);
    }
    crate::app::query_failure_watchdog::complete_child(CASE);
}

#[test]
fn should_forward_worker_authority_to_implicit_table_creation() {
    // Arrange: create an actual implicit owner before the real wire worker runs.
    let (cassie, session) = fixture();
    session
        .begin_implicit_transaction()
        .expect("implicit owner");
    let owner = session.transaction_unique_owner_id();
    let epoch = session.rollback_epoch_for_test();
    let (observed_tx, observed_rx) = mpsc::sync_channel(1);
    let _hook =
        super::authority_test_hooks::install_on_implicit_ddl(&cassie, &session, observed_tx);

    // Act: the hook must observe the actual authority received by implicit DDL.
    let result = run_entry_sql(
        &cassie,
        &session,
        PgwireEntry::Text,
        "CREATE TABLE worker_authority (id BIGINT PRIMARY KEY)",
    );
    let observed = observed_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("implicit DDL authority");
    let schema = cassie.catalog.get_schema("worker_authority");

    // Assert
    assert!(result.is_ok());
    assert_eq!(observed, Some((owner, epoch)));
    assert!(schema.is_some());
    assert_eq!(session.transaction_status(), "idle");
}
