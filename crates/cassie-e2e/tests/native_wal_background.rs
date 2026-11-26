//! This harness owns the process-global Midge flush-worker failpoint.

use std::sync::{mpsc, Mutex};
use std::time::Duration;

use cassie::midge::adapter::{StorageError, StorageRetryKind};
use cassie::midge::Midge;
use cntryl_midge::{TransactionMode, WriteOptions};

#[path = "support/temp_dirs.rs"]
mod support_temp_dirs;

const GUARD: Duration = Duration::from_secs(15);
const BEFORE_BUILD: &str = "midge::flush_worker::before_build";
const EXISTING: &str = "original";
const CREATED: &str = "new_owner";
const KEY: &[u8] = b"native-wal-background-sentinel";

struct FlushBuildHold {
    entered: mpsc::Receiver<()>,
    release: mpsc::Sender<()>,
}

impl FlushBuildHold {
    fn install() -> Self {
        let (observed, entered) = mpsc::channel();
        let (release, resumed) = mpsc::channel();
        let resumed = Mutex::new(Some(resumed));
        fail::cfg_callback(BEFORE_BUILD, move || {
            let wait = resumed.lock().expect("pause lock").take();
            if let Some(wait) = wait {
                observed.send(()).expect("report flush worker entry");
                let _ = wait.recv();
            }
        })
        .expect("configure released Midge flush worker failpoint");
        Self { entered, release }
    }
}

impl Drop for FlushBuildHold {
    fn drop(&mut self) {
        let _ = self.release.send(());
        fail::remove(BEFORE_BUILD);
    }
}

fn put_strict(storage: &Midge, database: &str) {
    let mut tx = storage
        .database_tx(database, TransactionMode::ReadWrite)
        .expect("issue native writer");
    tx.put(KEY.to_vec(), database.as_bytes().to_vec(), None)
        .expect("stage database-specific sentinel");
    tx.commit(WriteOptions::sync())
        .expect("acknowledge strict native commit");
}

fn create_while_flush_is_held<'scope>(
    scope: &'scope std::thread::Scope<'scope, '_>,
    storage: &'scope Midge,
) -> Result<(), StorageError> {
    let (complete, outcome) = mpsc::channel();
    let create = scope.spawn(move || {
        let _ = complete.send(storage.create_database(CREATED, None));
    });
    let result = outcome
        .recv_timeout(GUARD)
        .expect("create completed while the old-family flush worker was paused");
    create.join().expect("join create");
    result
}

#[test]
fn should_recover_native_data_when_database_creation_overlaps_a_flush() {
    // Arrange
    let directory = support_temp_dirs::IsolatedDataDir::new("native-wal-background");
    let storage = Midge::new_strict_with_data_dir(directory.path()).expect("open strict storage");
    storage
        .create_database(EXISTING, None)
        .expect("existing owner");
    let original = storage.database_family(EXISTING).expect("original family");
    put_strict(&storage, EXISTING);

    // Act
    std::thread::scope(|scope| {
        let hold = FlushBuildHold::install();
        let flush =
            scope.spawn(|| storage.flush_data_family_for_collection("original.public.probe"));
        hold.entered
            .recv_timeout(GUARD)
            .expect("Midge background worker reached the build boundary");
        assert!(storage
            .get_database(CREATED)
            .expect("read registry")
            .is_none());
        if let Err(error) = create_while_flush_is_held(scope, &storage) {
            assert!(
                matches!(
                    error.storage_retry_kind(),
                    Some(StorageRetryKind::WriteConflict | StorageRetryKind::WriteStall)
                ),
                "unexpected storage error: {error}"
            );
            assert!(storage
                .get_database(CREATED)
                .expect("read registry")
                .is_none());
            create_while_flush_is_held(scope, &storage).expect("retry unapplied create");
        }
        drop(hold);
        flush
            .join()
            .expect("join flush")
            .expect("durable old-family flush");
    });
    let created = storage.database_family(CREATED).expect("created family");
    put_strict(&storage, CREATED);
    drop(storage);

    // Assert
    assert_ne!(original.id(), created.id());
    assert_ne!(original.name(), created.name());
    let reopened =
        Midge::new_strict_with_data_dir(directory.path()).expect("reopen strict storage");
    for (database, expected) in [(EXISTING, original), (CREATED, created)] {
        let physical = reopened
            .database_family(database)
            .expect("recovered family");
        assert_eq!(physical.id(), expected.id());
        assert_eq!(physical.name(), expected.name());
        assert_eq!(
            reopened
                .get_database(database)
                .expect("registry read")
                .expect("recovered owner")
                .physical_family,
            expected.name()
        );
        assert_eq!(
            reopened
                .raw_get_database(database, KEY)
                .expect("read strict sentinel")
                .as_deref(),
            Some(database.as_bytes())
        );
    }
}
