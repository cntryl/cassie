use std::cell::RefCell;
use std::sync::mpsc;
use std::time::Duration;

use cntryl_midge::{TransactionMode, WriteOptions};

use super::Midge;

type CreatePause = Box<dyn FnOnce(&str)>;

thread_local! {
    static BEFORE_NATIVE_CREATE: RefCell<Option<CreatePause>> = const { RefCell::new(None) };
}

pub(super) fn observe_native_create(family: &str) {
    if let Some(observe) = BEFORE_NATIVE_CREATE.with(|slot| slot.take()) {
        observe(family);
    }
}

#[test]
fn should_preserve_an_issued_native_commit_during_database_creation() {
    // Arrange
    let path =
        std::env::temp_dir().join(format!("cassie-native-wal-create-{}", uuid::Uuid::new_v4()));
    let midge = Midge::new_strict_with_data_dir(&path).expect("open strict storage");
    let existing = "original";
    let created = "new_owner";
    let key = b"native-wal-sentinel";
    midge.create_database(existing, None).unwrap();
    let original_family = midge.database_family(existing).unwrap();
    let mut issued = midge
        .database_tx(existing, TransactionMode::ReadWrite)
        .unwrap();
    issued
        .put(key.to_vec(), existing.as_bytes().to_vec(), None)
        .unwrap();
    let (paused, reached) = mpsc::channel();
    let (release, resume) = mpsc::channel();

    // Act
    let pending_family = std::thread::scope(|scope| {
        let adapter = &midge;
        let creator = scope.spawn(move || {
            BEFORE_NATIVE_CREATE.with(|slot| {
                slot.replace(Some(Box::new(move |family| {
                    paused.send(family.to_owned()).unwrap();
                    resume
                        .recv_timeout(Duration::from_secs(15))
                        .expect("release create");
                })));
            });
            adapter.create_database(created, None)
        });
        let pending_family = reached
            .recv_timeout(Duration::from_secs(15))
            .expect("create reached the native publication boundary");
        assert!(midge.get_database(created).unwrap().is_none());
        assert!(midge.engine.get_column_family(&pending_family).is_none());
        issued
            .commit(WriteOptions::sync())
            .expect("issued native writer commits while CREATE is live");
        midge
            .flush_data_family_for_collection("original.public.probe")
            .expect("flush the committed writer family");
        release.send(()).unwrap();
        creator.join().unwrap().expect("create after native flush");
        pending_family
    });
    let mut sentinel = midge
        .database_tx(created, TransactionMode::ReadWrite)
        .unwrap();
    sentinel
        .put(key.to_vec(), created.as_bytes().to_vec(), None)
        .unwrap();
    sentinel.commit(WriteOptions::sync()).unwrap();

    // Assert
    assert_ne!(original_family.name(), pending_family);
    let owners = [
        (existing, original_family.name()),
        (created, pending_family.as_str()),
    ];
    let assert_owners = |storage: &Midge| {
        for (database, physical_family) in owners {
            let owner = storage
                .get_database(database)
                .unwrap()
                .expect("durable registry");
            assert_eq!(owner.physical_family, physical_family);
            assert_eq!(
                storage.database_family(database).unwrap().name(),
                physical_family
            );
            assert_eq!(
                storage.raw_get_database(database, key).unwrap().as_deref(),
                Some(database.as_bytes())
            );
        }
    };
    assert_owners(&midge);
    drop(midge);
    let reopened = Midge::new_strict_with_data_dir(&path).expect("reopen local store");
    assert_owners(&reopened);
    drop(reopened);
    std::fs::remove_dir_all(path).unwrap();
}
