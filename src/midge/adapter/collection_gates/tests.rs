//! Cancellations synchronize at the actual controlled write-gate wait.

use std::cell::{Cell, RefCell};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use super::*;
use crate::config::CassieRuntimeLimits;
use crate::runtime::QueryCancellationHandle;

thread_local! {
    static WAITING: RefCell<Option<mpsc::Sender<WaitBoundary>>> = const { RefCell::new(None) };
}

pub(super) fn observe_wait(boundary: WaitBoundary) {
    WAITING.with(|slot| {
        if let Some(sender) = slot.borrow_mut().take() {
            sender.send(boundary).expect("controlled waiting boundary");
        }
    });
}

fn controls(budget: usize, cancellation: QueryCancellationHandle) -> QueryExecutionControls {
    QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: budget,
            query_timeout_ms: 0,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
        cancellation,
    )
}

#[test]
fn should_cancel_a_commit_while_its_collection_gate_remains_held() {
    // Arrange
    let midge = Arc::new(Midge::new_in_memory("postgres").expect("memory engine"));
    let gate = midge.collection_write_gate("items");
    let held = gate.lock();
    let current = QueryCancellationHandle::new();
    let worker_cancel = current.clone();
    let other = Arc::clone(&midge);
    let (waiting_tx, waiting_rx) = mpsc::channel();
    let (result_tx, result_rx) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let controls = controls(1024 * 1024, worker_cancel);
        WAITING.with(|slot| slot.replace(Some(waiting_tx)));
        let result =
            other.with_transaction_write_gates(&["items".to_owned()], false, &controls, || Ok(()));
        result_tx
            .send((result, controls.current_query_memory_bytes()))
            .expect("canceled result");
    });

    // Act
    let boundary = waiting_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("actual collection wait");
    current.cancel();
    let (result, retained) = result_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("cancel before gate release");
    drop(held);
    worker.join().expect("controlled worker");

    // Assert
    assert_eq!(boundary, WaitBoundary::Collection);
    assert!(matches!(result, Err(CassieError::QueryCanceled)));
    assert_eq!(retained, 0);
}

#[test]
fn should_preserve_rest_cancellation_while_waiting_for_the_referential_gate() {
    // Arrange
    let midge = Arc::new(Midge::new_in_memory("postgres").expect("memory engine"));
    let cancellation = QueryCancellationHandle::new();
    let rest_cancellation = cancellation.clone();
    let (waiting_tx, waiting_rx) = mpsc::channel();
    let (result_tx, result_rx) = mpsc::channel();
    let mut worker = None;

    // Act
    midge.with_collection_write_gates(&[], || {
        let other = Arc::clone(&midge);
        worker = Some(std::thread::spawn(move || {
            let controls = controls(1024 * 1024, QueryCancellationHandle::new());
            let _rest = PublicationControlScope::enter_for_rest(
                &rest_cancellation,
                Instant::now() + Duration::from_secs(5),
            );
            WAITING.with(|slot| slot.replace(Some(waiting_tx)));
            let result = other.with_transaction_write_gates(&[], true, &controls, || Ok(()));
            result_tx
                .send((result, controls.current_query_memory_bytes()))
                .expect("REST wait result");
        }));
        assert_eq!(
            waiting_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("actual referential wait"),
            WaitBoundary::Referential
        );
        cancellation.cancel();
        let (result, retained) = result_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("cancel before referential release");

        // Assert
        assert!(matches!(result, Err(CassieError::QueryCanceled)));
        assert_eq!(retained, 0);
    });
    worker
        .expect("worker started")
        .join()
        .expect("controlled worker");
}

#[test]
fn should_admit_gate_directories_before_running_the_operation() {
    // Arrange
    let midge = Midge::new_in_memory("postgres").expect("memory engine");
    let name = ["items".to_owned()];
    let measured = controls(1024 * 1024, QueryCancellationHandle::new());
    midge
        .with_transaction_write_gates(&name, false, &measured, || Ok(()))
        .expect("measure directories");
    let peak = measured.peak_query_memory_bytes();
    let below = controls(peak - 1, QueryCancellationHandle::new());
    let exact = controls(peak, QueryCancellationHandle::new());
    let ran = Cell::new(false);

    // Act
    let denied = midge.with_transaction_write_gates(&name, false, &below, || {
        ran.set(true);
        Ok(())
    });
    let admitted = midge.with_transaction_write_gates(&name, false, &exact, || Ok(()));

    // Assert
    assert!(matches!(denied, Err(CassieError::ResourceLimit(_))));
    assert!(!ran.get());
    assert!(admitted.is_ok());
    assert_eq!(below.current_query_memory_bytes(), 0);
    assert_eq!(exact.current_query_memory_bytes(), 0);
}
