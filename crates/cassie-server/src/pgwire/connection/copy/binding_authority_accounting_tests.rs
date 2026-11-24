//! Authored, unrun COPY binding accounting witnesses.
//!
//! The scoped admission observer and reservation fields below are future test
//! seams. The observer must retain a real filler reservation until the actual
//! binding charge has been attempted, not synthesize the result.

use super::{bind_copy_from_stdin, request, BoundCopyRequest};
use crate::app::{Cassie, CassieError};
use crate::config::CassieRuntimeConfig;
use std::time::Duration;

fn fixture(active: bool) -> (Cassie, crate::app::CassieSession, request::CopyRequest) {
    let cassie =
        Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).expect("memory fixture");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE copy_authority_budget (id INT)",
            vec![],
        )
        .expect("table");
    if active {
        cassie
            .execute_sql(&session, "BEGIN", vec![])
            .expect("begin");
    }
    let request = request::parse_copy_request("COPY copy_authority_budget (id) FROM STDIN")
        .expect("parse")
        .expect("COPY");
    (cassie, session, request)
}

fn expected_charge(active: bool) -> usize {
    let binding = std::mem::size_of::<crate::app::transport::copy::CopyFromOperation>();
    let owner = if active {
        std::mem::size_of::<crate::app::CopyCommandOwner>()
    } else {
        0
    };
    binding
        .checked_sub(owner)
        .and_then(|charge| charge.checked_add("postgres".len()))
        .expect("operation contains owner and database")
}

#[test]
fn should_retain_shared_copy_owner_memory_until_the_outer_operation_retires() {
    for active in [false, true] {
        // Arrange
        let (cassie, session, request) = fixture(active);
        let operation = session.with_new_wire_operation();
        let BoundCopyRequest::From(_, _, binding) =
            bind_copy_from_stdin(&cassie, &operation, request, None).unwrap()
        else {
            panic!("COPY FROM binding");
        };
        let controls = binding.controls.clone();
        assert!(binding.operation.shared_authority_reservation_matches_for_test());

        // Act
        drop(binding);
        let while_outer_lives = controls.current_query_memory_bytes();
        drop(operation);
        let after_retirement = controls.current_query_memory_bytes();

        // Assert
        assert_eq!(
            while_outer_lives.checked_sub(after_retirement),
            Some(crate::app::FailureAuthority::shared_owner_bytes())
        );
    }
}

#[test]
fn should_retain_shared_copy_owner_memory_after_the_awaiting_task_is_aborted() {
    // Arrange
    let (cassie, session, request) = fixture(false);
    let operation = session.with_new_wire_operation();
    let BoundCopyRequest::From(_, _, binding) =
        bind_copy_from_stdin(&cassie, &operation, request, None).unwrap()
    else {
        panic!("COPY FROM binding");
    };
    let controls = binding.controls.clone();
    let worker_authority = binding.authority.clone();
    let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
    let (retired_tx, retired_rx) = tokio::sync::oneshot::channel();
    let (resume_tx, resume_rx) = std::sync::mpsc::sync_channel(1);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    // Act
    let (while_worker_lives, after_retirement) = runtime.block_on(async {
        let awaiting = tokio::spawn(async move {
            tokio::task::spawn_blocking(move || {
                entered_tx.send(()).unwrap();
                resume_rx.recv_timeout(Duration::from_secs(5)).unwrap();
                drop(worker_authority);
                retired_tx.send(()).unwrap();
            })
            .await
            .unwrap();
        });
        entered_rx.await.unwrap();
        awaiting.abort();
        let _ = awaiting.await;
        drop(binding);
        drop(operation);
        let retained = controls.current_query_memory_bytes();
        resume_tx.send(()).unwrap();
        retired_rx.await.unwrap();
        (retained, controls.current_query_memory_bytes())
    });

    // Assert
    assert_eq!(
        while_worker_lives.checked_sub(after_retirement),
        Some(crate::app::FailureAuthority::shared_owner_bytes())
    );
}

#[test]
fn should_reject_copy_binding_with_one_byte_less_than_authority_charge() {
    for active in [false, true] {
        // Arrange: the future seam observes the real charge after validation.
        let (cassie, session, request) = fixture(active);
        let (guard, observed) = super::binding_accounting_test_hooks::leave_before_charge(
            &cassie,
            &session,
            super::binding_accounting_test_hooks::Remaining::OneByteShort,
        );

        // Act
        let result = bind_copy_from_stdin(&cassie, &session, request, None);
        let observation = observed
            .recv_timeout(Duration::from_secs(5))
            .expect("charge boundary");
        drop(guard);

        // Assert: an earlier failure cannot satisfy the observed boundary.
        assert!(observation.charge > 0);
        assert_eq!(observation.charge, expected_charge(active));
        assert_eq!(observation.remaining + 1, observation.charge);
        assert_eq!(observation.owner_present, active);
        assert!(observation.charge_attempted);
        let error = result.err().expect("binding must fail");
        let budget = observation.controls.query_memory_budget_bytes;
        let expected = format!(
            "query memory budget exceeded: {} > {budget}",
            budget.checked_add(1).expect("test budget")
        );
        assert!(matches!(error, CassieError::ResourceLimit(ref message) if message == &expected));
        assert_eq!(error.sql_state(), "54000");
        if active {
            session.rollback_transaction();
        }
    }
}

#[test]
fn should_retain_copy_binding_authority_at_exact_remaining_budget() {
    for active in [false, true] {
        // Arrange
        let (cassie, session, request) = fixture(active);
        let (guard, observed) = super::binding_accounting_test_hooks::leave_before_charge(
            &cassie,
            &session,
            super::binding_accounting_test_hooks::Remaining::Exact,
        );

        // Act
        let result = bind_copy_from_stdin(&cassie, &session, request, None);
        let observation = observed
            .recv_timeout(Duration::from_secs(5))
            .expect("charge boundary");
        drop(guard);
        let BoundCopyRequest::From(_, _, binding) = result.expect("binding admitted") else {
            panic!("COPY FROM binding")
        };
        let charged_tracker = binding
            .operation
            .admission_reservation_belongs_to_for_test(&binding.controls);
        let retained_bytes = binding
            .operation
            .admission_reservation_bytes_for_test();
        let owner_tracker = binding
            .operation
            .owner_reservation_matches_for_test();
        let before_drop = observation.controls.current_query_memory_bytes();
        drop(binding);
        let after_drop = observation.controls.current_query_memory_bytes();

        // Assert
        assert!(observation.charge > 0);
        assert_eq!(observation.charge, expected_charge(active));
        assert_eq!(observation.remaining, observation.charge);
        assert_eq!(observation.owner_present, active);
        assert!(observation.charge_attempted);
        assert!(charged_tracker);
        assert_eq!(retained_bytes, observation.charge);
        assert_eq!(owner_tracker, active.then_some(true));
        assert!(before_drop
            .checked_sub(after_drop)
            .is_some_and(|released| released >= observation.charge));
        if active {
            session.rollback_transaction();
        }
    }
}

#[test]
fn should_preserve_force_column_error_before_authority_admission() {
    // Arrange: FORCE references a column absent from the target table.
    let (cassie, session, _) = fixture(true);
    let request = request::parse_copy_request(
        "COPY copy_authority_budget (id) FROM STDIN WITH (FORMAT CSV, FORCE_NULL (missing))",
    )
    .expect("parse")
    .expect("COPY");
    let (guard, observed) = super::binding_accounting_test_hooks::leave_before_charge(
        &cassie,
        &session,
        super::binding_accounting_test_hooks::Remaining::OneByteShort,
    );

    // Act
    let result = bind_copy_from_stdin(&cassie, &session, request, None);
    let reached = observed.try_recv();
    drop(guard);
    session.rollback_transaction();

    // Assert
    assert_eq!(result.err().expect("invalid FORCE").sql_state(), "42703");
    assert!(matches!(reached, Err(std::sync::mpsc::TryRecvError::Empty)));
}

#[test]
fn should_prefer_post_validation_cancellation_to_authority_admission() {
    // Arrange: the future seam cancels after real validation, before admission.
    let (cassie, session, request) = fixture(true);
    let cancellation = crate::runtime::QueryCancellationHandle::new();
    let (guard, observed) = super::binding_accounting_test_hooks::cancel_after_validation(
        &cassie,
        &session,
        cancellation.clone(),
    );

    // Act
    let result = bind_copy_from_stdin(&cassie, &session, request, Some(cancellation));
    let observation = observed
        .recv_timeout(Duration::from_secs(5))
        .expect("validation boundary");
    drop(guard);
    session.rollback_transaction();

    // Assert
    assert!(observation.validated);
    assert!(!observation.charge_attempted);
    assert!(matches!(result, Err(CassieError::QueryCanceled)));
}

#[test]
fn should_prefer_post_validation_deadline_to_authority_admission() {
    // Arrange: the future seam expires the actual binding controls at the boundary.
    let (cassie, session, request) = fixture(true);
    let (guard, observed) =
        super::binding_accounting_test_hooks::expire_after_validation(&cassie, &session);

    // Act
    let result = bind_copy_from_stdin(&cassie, &session, request, None);
    let observation = observed
        .recv_timeout(Duration::from_secs(5))
        .expect("validation boundary");
    drop(guard);
    session.rollback_transaction();

    // Assert
    assert!(observation.validated);
    assert!(!observation.charge_attempted);
    assert!(matches!(result, Err(CassieError::DeadlineExceeded)));
}

#[test]
fn should_report_malformed_payload_after_binding_is_admitted() {
    // Arrange
    let (cassie, session, request) = fixture(true);
    let bound = bind_copy_from_stdin(&cassie, &session, request, None).expect("binding admitted");
    let BoundCopyRequest::From(request, _statement, binding) = bound else {
        panic!("COPY FROM binding")
    };
    // Act
    let result = super::execute::execute_bound_copy_payload(&binding, &session, &request, &[0xff], None);
    drop(binding);
    session.rollback_transaction();

    // Assert
    assert!(
        matches!(result, Err(super::filter::CopyWhereError::Cassie(CassieError::Parse(ref message))) if message == "COPY payload must be UTF-8")
    );
}

#[test]
fn should_charge_binding_tracker_with_distinct_ambient_controls() {
    // Arrange
    let (cassie, session, request) = fixture(true);
    let ambient = crate::runtime::QueryExecutionControls::from_limits(
        &crate::config::CassieRuntimeLimits::default(),
        std::time::Instant::now(),
    );
    let ambient_scope = crate::midge::adapter::PublicationControlScope::enter(Some(&ambient));
    let (guard, observed) = super::binding_accounting_test_hooks::leave_before_charge(
        &cassie,
        &session,
        super::binding_accounting_test_hooks::Remaining::Exact,
    );

    // Act
    let result = bind_copy_from_stdin(&cassie, &session, request, None);
    let observation = observed
        .recv_timeout(Duration::from_secs(5))
        .expect("charge boundary");
    drop(guard);
    let BoundCopyRequest::From(_, _, binding) = result.expect("binding admitted") else {
        panic!("COPY FROM binding")
    };
    let binding_tracker = binding
        .operation
        .admission_reservation_belongs_to_for_test(&binding.controls);
    let ambient_tracker = binding
        .operation
        .admission_reservation_belongs_to_for_test(&ambient);
    drop(binding);
    drop(ambient_scope);
    session.rollback_transaction();

    // Assert
    assert!(observation.charge_attempted);
    assert!(binding_tracker);
    assert!(!ambient_tracker);
}

#[test]
fn should_retain_copy_binding_charge_until_blocking_worker_finishes() {
    const CASE: &str = "copy_authority_worker_reservation";
    if !crate::app::query_failure_watchdog::enter_child(
        "pgwire::connection::copy::binding_authority_accounting_tests::should_retain_copy_binding_charge_until_blocking_worker_finishes",
        CASE,
    ) {
        return;
    }

    // Arrange
    let (cassie, session, request) = fixture(true);
    let (guard, observed) = super::binding_accounting_test_hooks::leave_before_charge(
        &cassie,
        &session,
        super::binding_accounting_test_hooks::Remaining::Exact,
    );
    let result = bind_copy_from_stdin(&cassie, &session, request, None);
    let observation = observed
        .recv_timeout(Duration::from_secs(5))
        .expect("charge boundary");
    drop(guard);
    let BoundCopyRequest::From(_, _, binding) = result.expect("binding admitted") else {
        panic!("COPY FROM binding")
    };
    let (entered_tx, entered_rx) = std::sync::mpsc::sync_channel(1);
    let (resume_tx, resume_rx) = std::sync::mpsc::sync_channel(1);
    let tracker = observation.controls.clone();
    let charge = observation.charge;
    let worker = std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        let cassie = std::sync::Arc::clone(&binding.cassie);
        runtime.block_on(super::super::blocking::run_pgwire_blocking(
            cassie,
            "copy_authority_reservation_test",
            move |_| {
                let retained = binding
                    .operation
                    .admission_reservation_bytes_for_test();
                let on_tracker = binding
                    .operation
                    .admission_reservation_belongs_to_for_test(&tracker);
                entered_tx.send(()).expect("worker entered");
                resume_rx
                    .recv_timeout(Duration::from_secs(5))
                    .expect("resume worker");
                let super::CopyFromBinding {
                    cassie,
                    owner,
                    controls,
                    authority,
                    operation,
                } = *binding;
                drop((cassie, owner, controls, authority));
                let before = tracker.current_query_memory_bytes();
                drop(operation);
                let after = tracker.current_query_memory_bytes();
                Ok((retained, on_tracker, before, after))
            },
        ))
    });
    entered_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("worker entered");
    let while_worker = observation.controls.current_query_memory_bytes();

    // Act
    resume_tx.send(()).expect("resume worker");
    let result = worker.join().expect("worker thread");
    let after_worker = observation.controls.current_query_memory_bytes();
    session.rollback_transaction();

    // Assert
    assert!(observation.charge_attempted);
    let (retained, on_tracker, before_release, after_release) = result.expect("worker result");
    assert_eq!(retained, charge);
    assert!(on_tracker);
    assert_eq!(before_release.checked_sub(after_release), Some(charge));
    assert!(while_worker
        .checked_sub(after_worker)
        .is_some_and(|released| released >= observation.charge));
    drop(observation);
    drop(session);
    drop(cassie);
    crate::app::query_failure_watchdog::complete_child(CASE);
}

#[test]
fn should_reject_copy_query_target_before_waiting_for_transaction_lock() {
    const CASE: &str = "copy_invalid_query_target";
    if !crate::app::query_failure_watchdog::enter_child(
        "pgwire::connection::copy::binding_authority_accounting_tests::should_reject_copy_query_target_before_waiting_for_transaction_lock",
        CASE,
    ) {
        return;
    }

    // Arrange: parse the real invalid FROM request before holding the lock.
    let (cassie, session, _) = fixture(true);
    let request = request::parse_copy_request("COPY (SELECT 1) FROM STDIN")
        .expect("parse")
        .expect("COPY request");
    let (held_tx, held_rx) = std::sync::mpsc::sync_channel(1);
    let (release_tx, release_rx) = std::sync::mpsc::sync_channel(1);
    let held = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let holder_flag = std::sync::Arc::clone(&held);
    let holding_session = session.clone();
    let holder = std::thread::spawn(move || {
        holding_session.hold_transaction_for_test(&held_tx, &release_rx, holder_flag);
    });
    held_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("transaction lock held");
    let (result_tx, result_rx) = std::sync::mpsc::sync_channel(1);
    let old_cassie = cassie.clone();
    let old_session = session.clone();
    let worker = std::thread::spawn(move || {
        let result = bind_copy_from_stdin(&old_cassie, &old_session, request, None).map(|_| ());
        result_tx.send(result).expect("bind result");
    });

    // Act: the deadline is a liveness guard; release before joining on failure.
    let before_unlock = result_rx.recv_timeout(Duration::from_secs(5));
    let held_at_result = held.load(std::sync::atomic::Ordering::Acquire);
    release_tx.send(()).expect("release transaction lock");
    holder.join().expect("holder");
    worker.join().expect("bind worker");
    session.rollback_transaction();

    // Assert: this is the bind boundary, not the outer pgwire error response.
    assert!(
        matches!(before_unlock, Ok(Err(CassieError::Parse(ref message))) if message == "COPY FROM STDIN requires a table target")
    );
    assert!(held_at_result);
    drop(session);
    drop(cassie);
    crate::app::query_failure_watchdog::complete_child(CASE);
}
