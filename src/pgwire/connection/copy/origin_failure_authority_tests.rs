//! Authored, unrun probes for COPY's originating failure authority.
//!
//! The test-only operation-authority accessor and post-owner-check pause are
//! future seams. They must observe the real binding and execution path; they
//! are not implemented by this test.

use super::{bind_copy_from_stdin, execute, filter, request, BoundCopyRequest};
use crate::app::{Cassie, CassieError};
use crate::config::CassieRuntimeConfig;
use std::sync::mpsc;
use std::time::Duration;

fn fixture() -> (Cassie, crate::app::CassieSession, request::CopyRequest) {
    let cassie =
        Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).expect("memory fixture");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(&session, "CREATE TABLE copy_origin (id INT)", vec![])
        .expect("table");
    cassie
        .execute_sql(&session, "BEGIN", vec![])
        .expect("begin A");
    let request =
        request::parse_copy_request("COPY copy_origin (id) FROM STDIN WHERE (id / (id - id) > 0)")
            .expect("parse COPY")
            .expect("COPY request");
    (cassie, session, request)
}

#[test]
fn should_preserve_replacement_when_old_copy_predicate_fails() {
    const CASE: &str = "copy_origin_old_predicate";
    if !crate::app::query_failure_watchdog::enter_child(
        "pgwire::connection::copy::origin_failure_authority_tests::should_preserve_replacement_when_old_copy_predicate_fails",
        CASE,
    ) {
        return;
    }

    // Arrange: the future observer reads the actual authority held by the binding.
    let (cassie, session, request) = fixture();
    let owner_a = session.transaction_unique_owner_id();
    let bound = bind_copy_from_stdin(&cassie, &session, request, None).expect("bind in A");
    let BoundCopyRequest::From(request, statement, binding) = bound else {
        panic!("COPY FROM binding")
    };
    assert_eq!(binding.failure_authority_owner_for_test(), Some(owner_a));
    let (entered_tx, entered_rx) = mpsc::sync_channel(1);
    let (resume_tx, resume_rx) = mpsc::sync_channel(1);
    let old_session = session.clone();
    let worker = std::thread::spawn(move || {
        let _pause = execute::authority_test_hooks::install_after_owner_check(
            &binding.cassie,
            &old_session,
            entered_tx,
            resume_rx,
        );
        let ambient = crate::runtime::QueryExecutionControls::from_limits(
            &crate::config::CassieRuntimeLimits::default(),
            std::time::Instant::now(),
        );
        let _ambient = crate::midge::adapter::PublicationControlScope::enter(Some(&ambient));
        let _controls =
            crate::midge::adapter::PublicationControlScope::enter(Some(&binding.controls));
        let current = crate::midge::adapter::PublicationControlScope::current_query_controls()
            .expect("ambient controls");
        let reservation = current.reserve_query_memory(0).expect("control identity");
        assert!(reservation.belongs_to(&ambient));
        assert!(!reservation.belongs_to(&binding.controls));
        drop(reservation);
        execute::execute_copy_payload_for_operation(
            &binding.cassie,
            &old_session,
            &request,
            &statement,
            b"41\n",
            None,
            (binding.owner.as_ref(), Some(&binding.authority)),
        )
    });
    entered_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("owner check completed before nested predicate");
    session.rollback_transaction();
    cassie
        .execute_sql(&session, "BEGIN", vec![])
        .expect("begin B");
    let owner_b = session.transaction_unique_owner_id();
    assert_ne!(owner_a, owner_b);

    // Act
    resume_tx.send(()).expect("resume nested predicate");
    let result = worker.join().expect("COPY worker");
    let healthy = !session.is_transaction_failed();
    let usable = cassie.execute_sql(&session, "SELECT 1", vec![]);
    let current_owner = session.transaction_unique_owner_id();
    session.rollback_transaction();

    // Assert
    assert!(
        matches!(result, Err(filter::CopyWhereError::Cassie(CassieError::Execution(ref message))) if message == "division by zero")
    );
    assert!(healthy);
    assert!(usable.is_ok());
    assert_eq!(current_owner, owner_b);
    drop(usable);
    drop(result);
    drop(session);
    drop(cassie);
    crate::app::query_failure_watchdog::complete_child(CASE);
}

#[test]
fn should_fail_origin_when_current_copy_predicate_fails() {
    // Arrange: the identical predicate must actually fail through the real COPY path.
    let (cassie, session, request) = fixture();
    let owner = session.transaction_unique_owner_id();
    let bound = bind_copy_from_stdin(&cassie, &session, request, None).expect("bind in A");
    let BoundCopyRequest::From(request, statement, binding) = bound else {
        panic!("COPY FROM binding")
    };
    assert_eq!(binding.failure_authority_owner_for_test(), Some(owner));

    // Act
    let controls = crate::midge::adapter::PublicationControlScope::enter(Some(&binding.controls));
    let result = execute::execute_copy_payload_for_operation(
        &binding.cassie,
        &session,
        &request,
        &statement,
        b"41\n",
        None,
        (binding.owner.as_ref(), Some(&binding.authority)),
    );
    drop(controls);
    let failed = session.is_transaction_failed();
    session.rollback_transaction();

    // Assert
    assert!(
        matches!(result, Err(filter::CopyWhereError::Cassie(CassieError::Execution(ref message))) if message == "division by zero")
    );
    assert!(failed);
}
