use super::Catalog;
use crate::app::CassieError;
use crate::config::CassieRuntimeLimits;
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};
use std::time::Instant;

#[test]
fn should_interrupt_the_controlled_index_namespace_lookup() {
    // Arrange
    let catalog = Catalog::new();
    catalog.register_collection("postgres.public.rows", Vec::new());
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    cancellation.cancel();

    // Act
    let result = catalog.index_relation_name_occupied(
        "postgres.public.idx",
        "postgres",
        false,
        Some(&controls),
    );

    // Assert
    assert!(matches!(result, Err(CassieError::QueryCanceled)));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_controlled_namespace_name_owners_when_admission_fails() {
    // Arrange
    let catalog = Catalog::new();
    catalog.register_collection("postgres.public.rows", Vec::new());
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: 1,
        ..CassieRuntimeLimits::default()
    };
    let controls = QueryExecutionControls::from_limits(&limits, Instant::now());

    // Act
    let result = catalog.index_relation_name_occupied(
        "postgres.public.idx",
        "postgres",
        false,
        Some(&controls),
    );

    // Assert
    assert!(matches!(result, Err(CassieError::ResourceLimit(_))));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_names_when_cancelled_during_collection_snapshot() {
    // Arrange
    let catalog = Catalog::new();
    catalog.register_collection("postgres.public.rows", Vec::new());
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );

    // Act
    let result = catalog.index_relation_name_occupied_with(
        "postgres.public.idx",
        "postgres",
        false,
        Some(&controls),
        || cancellation.cancel(),
        || {},
    );

    // Assert
    assert!(matches!(result, Err(CassieError::QueryCanceled)));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_interrupt_while_waiting_for_a_catalog_guard() {
    // Arrange
    let catalog = Catalog::new();
    let held = catalog.collections.write();
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let worker_catalog = catalog.clone();
    let worker_controls = controls.clone();
    let (started, received) = std::sync::mpsc::channel();
    let (finished, completion) = std::sync::mpsc::channel();

    // Act
    let worker = std::thread::spawn(move || {
        let mut started = Some(started);
        let result = worker_catalog.index_relation_name_occupied_with(
            "postgres.public.idx",
            "postgres",
            false,
            Some(&worker_controls),
            || {},
            || {
                if let Some(started) = started.take() {
                    started.send(()).expect("signal blocked worker");
                }
            },
        );
        finished.send(result).expect("report worker result");
    });
    received
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("worker waited for guard");
    cancellation.cancel();
    let result = completion.recv_timeout(std::time::Duration::from_secs(5));
    drop(held);
    worker.join().expect("worker stopped");

    // Assert
    assert!(matches!(
        result.expect("cancelled while guard held"),
        Err(CassieError::QueryCanceled)
    ));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_an_acquired_guard_when_cancelled_at_the_acquisition_boundary() {
    // Arrange
    let lock = parking_lot::RwLock::new(());
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );

    // Act
    let result = super::read(&lock, Some(&controls), &mut || {}, &mut || {
        cancellation.cancel()
    });

    // Assert
    assert!(matches!(result, Err(CassieError::QueryCanceled)));
    assert!(lock.try_write().is_some());
}

#[test]
fn should_fail_closed_for_two_materialized_owners_of_one_output() {
    // Arrange
    let catalog = Catalog::new();
    catalog.register_collection("postgres.public.physical", Vec::new());
    for name in ["postgres.public.first", "postgres.public.second"] {
        let mut projection = crate::catalog::ProjectionMeta::new(name, 1);
        projection.kind = crate::catalog::ProjectionKind::Materialized;
        projection
            .versions
            .push(crate::catalog::ProjectionVersionMeta {
                version_id: "v1".to_owned(),
                output_collection: "postgres.public.physical".to_owned(),
                definition_fingerprint: 0,
                source_schema_epoch: 0,
                state: crate::catalog::ProjectionVersionState::Active,
                created_ms: 0,
                activated_ms: None,
                retired_ms: None,
                last_error: None,
                verification: Default::default(),
            });
        catalog.register_projection_metadata(projection);
    }
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());

    // Act
    let result = catalog.index_relation_name_occupied(
        "postgres.public.idx",
        "postgres",
        false,
        Some(&controls),
    );

    // Assert
    assert!(result.is_err());
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
