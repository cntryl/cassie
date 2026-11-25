use std::cell::RefCell;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use cassie_core::types::{DataType, FieldSchema, Schema};

use super::{CassieError, Midge, COLUMN_BATCH_ARTIFACT};

type Pause = Box<dyn FnOnce()>;

thread_local! {
    static AFTER_DEBT_READ: RefCell<Option<Pause>> = const { RefCell::new(None) };
}

pub(super) fn pause_after_debt_read() {
    let callback = AFTER_DEBT_READ.with(|slot| slot.borrow_mut().take());
    if let Some(callback) = callback {
        callback();
    }
}

#[test]
fn should_preserve_newer_debt_when_old_maintenance_succeeds_or_fails() {
    // Arrange
    for fail_old_generation in [false, true] {
        let path = std::env::temp_dir().join(format!("cassie-stale-debt-{}", uuid::Uuid::new_v4()));
        let midge = Midge::new_strict_with_data_dir(&path).expect("open local fixture");
        let collection = midge.canonical_collection_name("stale_debt");
        midge
            .create_collection(
                &collection,
                Schema {
                    fields: vec![FieldSchema {
                        name: "value".to_owned(),
                        data_type: DataType::Int,
                        nullable: false,
                    }],
                },
            )
            .expect("create source");
        put_row(&midge, &collection, 1);
        let old = midge
            .collection_generation(&collection)
            .expect("generation G");

        // Act
        let newer_debt =
            advance_while_debt_writer_is_paused(&midge, &collection, old, fail_old_generation);
        let actual = midge
            .maintenance_debt_for(&collection, COLUMN_BATCH_ARTIFACT)
            .expect("durable debt after prior worker resumes");
        drop(midge);
        let reopened = Midge::new_strict_with_data_dir(&path).expect("reopen local fixture");
        let durable = reopened
            .maintenance_debt_for(&collection, COLUMN_BATCH_ARTIFACT)
            .expect("read durable debt");

        // Assert
        assert_eq!(newer_debt.target_generation, old + 1);
        assert_eq!(newer_debt.retry_count, 1);
        assert_eq!(actual.as_ref(), Some(&newer_debt));
        assert_eq!(durable.as_ref(), Some(&newer_debt));
        reopened
            .retry_maintenance_debt()
            .expect("repair latest generation");
        assert!(reopened
            .maintenance_debt_for(&collection, COLUMN_BATCH_ARTIFACT)
            .expect("debt after repair")
            .is_none());
        assert_eq!(
            reopened
                .get_document(&collection, "row")
                .expect("read authoritative row")
                .expect("row survived reopen")
                .payload,
            serde_json::json!({"value": 2})
        );
        drop(reopened);
        std::fs::remove_dir_all(&path).expect("remove local fixture");
    }
}

fn put_row(midge: &Midge, collection: &str, value: i32) {
    midge
        .put_document(
            collection,
            Some("row".to_owned()),
            serde_json::json!({"value": value}),
        )
        .expect("publish authoritative row");
}

fn advance_while_debt_writer_is_paused(
    midge: &Midge,
    collection: &str,
    old: u64,
    fail_old_generation: bool,
) -> super::MaintenanceDebt {
    let (paused, after_read) = mpsc::channel();
    let (release, proceed) = mpsc::channel();
    thread::scope(|scope| {
        let worker = scope.spawn(move || {
            AFTER_DEBT_READ.with(|slot| {
                slot.replace(Some(Box::new(move || {
                    paused.send(()).expect("old generation read");
                    proceed
                        .recv_timeout(Duration::from_secs(5))
                        .expect("generation G+1 committed");
                })));
            });
            if fail_old_generation {
                midge.record_maintenance_failure(
                    collection,
                    COLUMN_BATCH_ARTIFACT,
                    old,
                    &CassieError::Execution("old maintenance failure".to_owned()),
                )
            } else {
                midge.clear_maintenance_debt(collection, COLUMN_BATCH_ARTIFACT, old)
            }
        });
        after_read
            .recv_timeout(Duration::from_secs(5))
            .expect("old debt mutation paused");
        crate::adapter::set_column_batch_maintenance_failure_point(true);
        put_row(midge, collection, 2);
        let saved = midge
            .maintenance_debt_for(collection, COLUMN_BATCH_ARTIFACT)
            .expect("read generation G+1 debt")
            .expect("newer retry recorded");
        release.send(()).expect("resume old generation");
        worker
            .join()
            .expect("old debt worker")
            .expect("stale outcome is harmless");
        saved
    })
}
