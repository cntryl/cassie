#[cfg(feature = "replay-benchmark-observation")]
use super::{observe_native_commits, NativeCommitObservation};
use crate::midge::adapter::Midge;
#[cfg(feature = "replay-benchmark-observation")]
use crate::midge::adapter::StorageFamily;

#[cfg(feature = "replay-benchmark-observation")]
#[test]
fn should_count_native_commits_only_within_observation_scope() {
    // Arrange
    let midge = Midge::new_in_memory("postgres").expect("memory engine");
    midge
        .raw_put(StorageFamily::Schema, b"before", b"value")
        .unwrap();

    // Act
    let (result, observed) = observe_native_commits(|| {
        midge.raw_put(StorageFamily::Schema, b"inside-1", b"value")?;
        midge.raw_put(StorageFamily::Schema, b"inside-2", b"value")
    });
    midge
        .raw_put(StorageFamily::Schema, b"after", b"value")
        .unwrap();
    let ((), empty) = observe_native_commits(|| ());

    // Assert
    result.unwrap();
    assert_eq!(observed.attempted, 2);
    assert_eq!(observed.succeeded, 2);
    assert_eq!(observed.failed, 0);
    assert_eq!(observed.outcome_unknown, 0);
    assert_eq!(empty, NativeCommitObservation::default());
}

#[cfg(feature = "replay-benchmark-observation")]
#[test]
fn should_restore_commit_observer_after_unwind() {
    // Arrange
    let midge = Midge::new_in_memory("postgres").expect("memory engine");
    midge.ensure_families_ready().expect("bootstrap families");

    // Act
    let panic = std::panic::catch_unwind(|| observe_native_commits(|| panic!("injected panic")));
    let (result, observation) =
        observe_native_commits(|| midge.raw_put(StorageFamily::Schema, b"after-panic", b"value"));

    // Assert
    assert!(panic.is_err());
    result.unwrap();
    assert_eq!(observation.attempted, 1);
    assert_eq!(observation.succeeded, 1);
}

#[cfg(feature = "replay-benchmark-observation")]
#[test]
fn should_preserve_outer_commit_observer_when_nested_scope_is_rejected() {
    // Arrange
    let midge = Midge::new_in_memory("postgres").expect("memory engine");
    midge.ensure_families_ready().expect("bootstrap families");

    // Act
    let (nested, observation) = observe_native_commits(|| {
        let nested = std::panic::catch_unwind(|| observe_native_commits(|| ()));
        midge
            .raw_put(StorageFamily::Schema, b"after-nested", b"value")
            .unwrap();
        nested
    });

    // Assert
    assert!(nested.is_err());
    assert_eq!(observation.attempted, 1);
    assert_eq!(observation.succeeded, 1);
}

#[test]
fn should_forward_engine_durability_counters_without_writes() {
    // Arrange
    let midge = Midge::new_in_memory("postgres").expect("memory engine");
    let before = midge.engine.metrics().get_runtime_metrics().unwrap();

    // Act
    let observed = midge.native_durability_metrics().unwrap();
    let after = midge.engine.metrics().get_runtime_metrics().unwrap();

    // Assert
    assert!(observed.wal_fsync_count >= before.wal_fsync_count);
    assert!(observed.wal_fsync_count <= after.wal_fsync_count);
    assert!(observed.wal_accounted_syncs >= before.wal_accounted_syncs);
    assert!(observed.wal_accounted_syncs <= after.wal_accounted_syncs);
    assert!(observed.current_sequence >= before.current_sequence);
    assert!(observed.current_sequence <= after.current_sequence);
}

#[cfg(feature = "replay-benchmark-observation")]
#[test]
fn should_distinguish_failed_native_commit_outcomes() {
    // Arrange
    use super::observe_native_commit;

    // Act
    let ((), observation) = observe_native_commits(|| {
        let _ = observe_native_commit::<()>(cntryl_midge::WriteOptions::sync(), || {
            Err(cntryl_midge::MidgeError::Internal("failed".into()))
        });
        let _ = observe_native_commit::<()>(cntryl_midge::WriteOptions::sync(), || {
            Err(cntryl_midge::MidgeError::OutcomeUnknown("unknown".into()))
        });
    });

    // Assert
    assert_eq!(observation.attempted, 2);
    assert_eq!(observation.succeeded, 0);
    assert_eq!(observation.failed, 1);
    assert_eq!(observation.outcome_unknown, 1);
}

#[cfg(feature = "replay-benchmark-observation")]
#[test]
fn should_report_native_commit_attempts_by_selected_policy() {
    // Arrange
    use super::observe_native_commit;
    use cntryl_midge::WriteOptions;

    // Act
    let ((), observation) = observe_native_commits(|| {
        for options in [
            WriteOptions::sync(),
            WriteOptions::buffered(),
            WriteOptions::cloud_async(),
            WriteOptions::cloud_strict(),
            WriteOptions::best_effort(),
        ] {
            observe_native_commit(options, || Ok::<_, cntryl_midge::MidgeError>(())).unwrap();
        }
    });

    // Assert
    assert_eq!(observation.attempted, 5);
    assert_eq!(observation.succeeded, 5);
    assert_eq!(observation.local_sync_attempts, 1);
    assert_eq!(observation.local_buffered_attempts, 1);
    assert_eq!(observation.cloud_async_attempts, 1);
    assert_eq!(observation.cloud_strict_attempts, 1);
    assert_eq!(observation.best_effort_attempts, 1);
}
