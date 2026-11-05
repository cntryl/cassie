use super::super::Midge;
use crate::app::CassieError;
use crate::config::CassieRuntimeLimits;
use crate::midge::adapter::PublicationControlScope;
use crate::runtime::QueryExecutionControls;
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

#[test]
fn should_exclude_native_family_creation_until_schema_publication_retires() {
    // Arrange
    for quarantine in [false, true] {
        let midge = Arc::new(Midge::new_in_memory("postgres").expect("memory adapter"));
        midge
            .ensure_families_ready()
            .expect("bootstrap schema family");
        let other = Arc::clone(&midge);
        let (waiting_tx, waiting_rx) = mpsc::channel();

        // Act
        let worker = midge
            .with_schema_publication(|| {
                let worker = std::thread::spawn(move || {
                    let controls = QueryExecutionControls::from_limits(
                        &CassieRuntimeLimits::default(),
                        Instant::now(),
                    );
                    let _request = PublicationControlScope::enter(Some(&controls));
                    let _wait = Midge::observe_next_publication_wait_for_test(move || {
                        waiting_tx
                            .send(())
                            .expect("physical family reached schema guard");
                    });
                    other.create_physical_family("guarded-database-family")
                });
                waiting_rx
                    .recv_timeout(Duration::from_secs(5))
                    .expect("actual publication wait");
                assert!(midge
                    .engine
                    .get_column_family("guarded-database-family")
                    .is_none());
                if quarantine {
                    midge.latch_storage_recovery_required_for_test();
                }
                Ok(worker)
            })
            .expect("held schema publication boundary");
        let outcome = worker.join().expect("physical publisher");

        // Assert
        if quarantine {
            assert!(
                matches!(outcome, Err(CassieError::Storage(message)) if message.contains("storage recovery required"))
            );
            assert!(midge
                .engine
                .get_column_family("guarded-database-family")
                .is_none());
        } else {
            assert_eq!(
                outcome.expect("released publisher").name(),
                "guarded-database-family"
            );
            assert!(midge
                .engine
                .get_column_family("guarded-database-family")
                .is_some());
        }
    }
}

#[test]
fn should_retire_an_unpublished_staged_family_after_request_cancellation() {
    // Arrange
    for quarantine in [false, true] {
        let midge = Midge::new_in_memory("postgres").expect("memory adapter");
        let staged = midge
            .stage_database_family("cancelled-stage")
            .expect("owned unpublished family");
        let cancellation = crate::runtime::QueryCancellationHandle::new();
        let controls = QueryExecutionControls::with_cancellation(
            &CassieRuntimeLimits::default(),
            Instant::now(),
            cancellation.clone(),
        );
        let _request = PublicationControlScope::enter(Some(&controls));
        cancellation.cancel();
        if quarantine {
            midge.latch_storage_recovery_required_for_test();
        }

        // Act
        let result = midge.abort_staged_database_family(&staged);
        let remains = midge
            .engine
            .get_column_family(&staged.metadata.physical_family)
            .is_some();

        // Assert
        if quarantine {
            assert!(
                matches!(result, Err(CassieError::Storage(message)) if message.contains("storage recovery required"))
            );
            assert!(remains, "unknown outcomes remain quarantined");
        } else {
            result.expect("known unpublished cleanup retires despite cancelled request");
            assert!(!remains);
            assert!(midge
                .get_database("cancelled-stage")
                .expect("metadata read")
                .is_none());
        }
    }
}
