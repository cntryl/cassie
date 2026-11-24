//! Cancellations synchronize at the actual controlled write-gate wait.

use std::cell::{Cell, RefCell};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use super::*;
use crate::catalog::{Catalog, SequenceMeta};
use crate::config::CassieRuntimeLimits;
use crate::runtime::QueryCancellationHandle;
use crate::types::DataType;

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
fn should_serialize_sequence_cursor_publication_after_the_midge_update() {
    // Arrange
    let midge = Arc::new(Midge::new_in_memory("postgres").expect("memory engine"));
    let name = "public.cursor_publication_order";
    let mut sequence = SequenceMeta::new(name, DataType::BigInt);
    sequence.object_id = 91;
    sequence.is_called = true;
    midge.put_sequence(&sequence).expect("persist sequence");
    let catalog = Arc::new(Catalog::new());
    catalog.register_sequence(sequence);

    let (advanced_tx, advanced_rx) = mpsc::channel();
    let (resume_tx, resume_rx) = mpsc::channel();
    let first_midge = Arc::clone(&midge);
    let first_catalog = Arc::clone(&catalog);
    let first_name = name.to_owned();
    let first = std::thread::spawn(move || {
        let query_controls = controls(1024 * 1024, QueryCancellationHandle::new());
        let _query = PublicationControlScope::enter(Some(&query_controls));
        first_midge.with_sequence_update_gate(&first_name, || {
            let (_, updated) = first_midge.update_sequence_retrying(&first_name, |metadata| {
                metadata.advance().map_err(Into::into)
            })?;
            advanced_tx
                .send(updated.current_value)
                .expect("signal the durable cursor update");
            resume_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("resume cursor publication");
            first_catalog.update_sequence_cursor(&updated);
            Ok(())
        })
    });

    let first_value = advanced_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("first Midge cursor update");
    let second_midge = Arc::clone(&midge);
    let second_catalog = Arc::clone(&catalog);
    let second_name = name.to_owned();
    let (waiting_tx, waiting_rx) = mpsc::channel();
    let second = std::thread::spawn(move || {
        let query_controls = controls(1024 * 1024, QueryCancellationHandle::new());
        WAITING.with(|slot| slot.replace(Some(waiting_tx)));
        let _query = PublicationControlScope::enter(Some(&query_controls));
        second_midge.with_sequence_update_gate(&second_name, || {
            let (_, updated) = second_midge.update_sequence_retrying(&second_name, |metadata| {
                metadata.advance().map_err(Into::into)
            })?;
            second_catalog.update_sequence_cursor(&updated);
            Ok(())
        })
    });

    // Act
    let boundary = waiting_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("second update reaches the shared sequence gate");
    resume_tx
        .send(())
        .expect("release first cursor publication");
    let first_result = first.join().expect("first sequence update thread");
    let second_result = second.join().expect("second sequence update thread");

    // Assert
    assert_eq!(first_value, 2);
    assert_eq!(boundary, WaitBoundary::Collection);
    assert!(first_result.is_ok());
    assert!(second_result.is_ok());
    let published = catalog
        .get_sequence(name)
        .expect("catalog sequence remains published");
    let stored = midge
        .get_sequence(name)
        .expect("read stored sequence")
        .expect("stored sequence remains present");
    assert_eq!(published.current_value, 3);
    assert!(published.is_called);
    assert_eq!(stored.current_value, 3);
    assert!(stored.is_called);
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
