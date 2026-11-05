//! Deferred publication, ownership, resource and native producer regressions.

use std::cell::{Cell, RefCell};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use super::*;
use crate::config::CassieRuntimeLimits;
use crate::midge::adapter::publication::PublicationCoordinator;
use crate::midge::Midge;
use crate::runtime::QueryExecutionControls;

thread_local! {
    static RETIRING: RefCell<Option<mpsc::Sender<()>>> = const { RefCell::new(None) };
}

pub(super) fn observe_retirement() {
    RETIRING.with(|slot| {
        if let Some(sender) = slot.borrow_mut().take() {
            sender.send(()).expect("retirement before lock");
        }
    });
}

fn controls() -> QueryExecutionControls {
    QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now())
}

fn publication(budget: usize) -> Arc<DatabasePublication> {
    let coordinator = PublicationCoordinator::default();
    coordinator
        .configure_history_budget(budget)
        .expect("budget before admission");
    coordinator.family(1)
}

fn keys(publication: &DatabasePublication, key: &[u8]) -> MutationKeys {
    let mut keys = publication.row_history.key_log().expect("key log");
    let prepared = keys.prepare(key).expect("key backing admitted");
    keys.install(prepared);
    keys
}

#[test]
fn should_track_a_writer_staged_before_the_first_repeatable_capture() {
    // Arrange
    let midge = Midge::new_in_memory("postgres").expect("memory engine");
    midge.ensure_families_ready().expect("families");
    let key = Midge::row_key(17, "one");
    let mut writer = midge
        .begin_raw_database_adapter_tx("postgres", cntryl_midge::TransactionMode::ReadWrite)
        .expect("private writer");
    writer
        .put(key.clone(), b"same".to_vec(), None)
        .expect("staged before snapshot");
    let owner = midge
        .capture_repeatable_row_history("postgres", &controls())
        .expect("capture");

    // Act
    writer
        .commit(midge.write_options_sync())
        .expect("confirmed publication");
    let result = owner.check_keys([key.as_slice()], &controls());

    // Assert
    assert_eq!(
        result
            .expect_err("newer current conflict target")
            .descriptor()
            .sql_state,
        "40001"
    );
}

#[test]
fn should_track_same_value_writes_for_each_authoritative_representation() {
    // Arrange
    for key in [
        Midge::row_key(17, "one"),
        Midge::column_store_row_key(17, "one"),
        Midge::doc_key("postgres.public.items", "one"),
    ] {
        let midge = Midge::new_in_memory("postgres").expect("memory engine");
        midge.ensure_families_ready().expect("families");
        let mut first = midge
            .begin_raw_database_adapter_tx("postgres", cntryl_midge::TransactionMode::ReadWrite)
            .expect("seed writer");
        first
            .put(key.clone(), b"same".to_vec(), None)
            .expect("seed");
        first
            .commit(midge.write_options_sync())
            .expect("seed commit");
        let owner = midge
            .capture_repeatable_row_history("postgres", &controls())
            .expect("capture");

        // Act
        midge
            .raw_put(crate::midge::StorageFamily::Data, &key, b"same")
            .expect("observed public raw rewrite");
        let result = owner.check_keys([key.as_slice()], &controls());

        // Assert
        assert_eq!(
            result
                .expect_err("same payload still mutated")
                .descriptor()
                .sql_state,
            "40001"
        );
    }
}

#[test]
fn should_ignore_a_native_stage_that_failed_before_recording_the_key() {
    // Arrange
    let midge = Midge::new_in_memory("postgres").expect("memory engine");
    midge.ensure_families_ready().expect("families");
    let owner = midge
        .capture_repeatable_row_history("postgres", &controls())
        .expect("capture");
    let key = Midge::row_key(17, "one");
    let mut writer = midge
        .begin_raw_database_adapter_tx("postgres", cntryl_midge::TransactionMode::ReadOnly)
        .expect("read-only writer control");

    // Act
    let stage = writer.put(key.clone(), b"value".to_vec(), None);
    writer
        .commit(midge.write_options_sync())
        .expect("read-only commit");
    let result = owner.check_keys([key.as_slice()], &controls());

    // Assert
    assert!(stage.is_err());
    assert!(result.is_ok());
}

#[test]
fn should_preserve_only_history_needed_by_the_oldest_remaining_owner() {
    // Arrange
    let publication = publication(1024 * 1024);
    let first = RowHistory::register(&publication).expect("first capture");
    let one = Midge::row_key(17, "one");
    publication
        .publish_rows_current(keys(&publication, &one), || Ok(()))
        .expect("first mutation");
    let second = RowHistory::register(&publication).expect("second capture");
    let two = Midge::row_key(17, "two");
    publication
        .publish_rows_current(keys(&publication, &two), || Ok(()))
        .expect("second mutation");

    // Act
    drop(first);
    let first_key = second.check_keys([one.as_slice()], &controls());
    let second_key = second.check_keys([two.as_slice()], &controls());
    let retained = publication.row_history.state.lock().entries.len();
    drop(second);

    // Assert
    assert!(first_key.is_ok());
    assert!(second_key.is_err());
    assert_eq!(retained, 1);
    let state = publication.row_history.state.lock();
    assert_eq!(state.entries.capacity(), 0);
    assert_eq!(state.active.capacity(), 0);
    assert_eq!(
        state
            .entries_memory
            .as_ref()
            .expect("admitted history directory")
            .bytes(),
        0
    );
    assert_eq!(
        state
            .active_memory
            .as_ref()
            .expect("admitted registration directory")
            .bytes(),
        0
    );
    assert_eq!(
        state
            .admission
            .as_ref()
            .expect("admitted tracker")
            .tracker_used_bytes(),
        state.admission.as_ref().expect("admitted tracker").bytes()
    );
}

#[test]
fn should_serialize_owner_retirement_with_native_confirmation() {
    // Arrange
    let publication = publication(1024 * 1024);
    let owner = RowHistory::register(&publication).expect("old capture");
    let key = Midge::row_key(17, "one");
    let staged = keys(&publication, &key);
    let (begin_tx, begin_rx) = mpsc::channel();
    let (retiring_tx, retiring_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        begin_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("native confirmation paused");
        RETIRING.with(|slot| slot.replace(Some(retiring_tx)));
        drop(owner);
        done_tx.send(()).expect("retirement completed");
    });

    // Act
    publication
        .publish_rows_current(staged, || {
            begin_tx
                .send(())
                .expect("start retirement while state locked");
            retiring_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("exact pre-lock retirement boundary");
            assert!(done_rx.try_recv().is_err());
            Ok(())
        })
        .expect("confirmation installs before retirement lock");
    done_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("retirement proceeds after confirmation");
    worker.join().expect("retirement worker");
    let newer = RowHistory::register(&publication).expect("new capture after confirmation");

    // Assert
    assert_eq!(newer.sequence, 1);
    assert!(newer.check_keys([key.as_slice()], &controls()).is_ok());
}

fn admitted_publication(budget: usize) -> Result<(usize, Arc<DatabasePublication>), CassieError> {
    let publication = publication(budget);
    let owner = RowHistory::register(&publication)?;
    let key = Midge::row_key(17, "one");
    let mut keys = publication.row_history.key_log()?;
    let prepared = keys.prepare(&key)?;
    keys.install(prepared);
    publication
        .publish_rows_current(keys, || Ok(()))
        .map_err(CassieError::from)?;
    let peak = publication
        .row_history
        .state
        .lock()
        .admission
        .as_ref()
        .expect("admitted tracker")
        .tracker_peak_bytes();
    drop(owner);
    Ok((peak, publication))
}

#[test]
fn should_enforce_the_complete_history_admission_boundary() {
    // Arrange
    let (peak, _) = admitted_publication(1024 * 1024).expect("measure complete backing overlap");

    // Act
    let denied = admitted_publication(peak - 1);
    let admitted = admitted_publication(peak);

    // Assert
    assert!(matches!(denied, Err(CassieError::ResourceLimit(_))));
    let (_, publication) = admitted.expect("exact complete budget");
    let state = publication.row_history.state.lock();
    assert_eq!(
        state
            .admission
            .as_ref()
            .expect("admitted tracker")
            .tracker_used_bytes(),
        state.admission.as_ref().expect("admitted tracker").bytes()
    );
}

#[test]
fn should_reject_sequence_exhaustion_before_native_submission() {
    // Arrange
    let publication = publication(1024 * 1024);
    let owner = RowHistory::register(&publication).expect("capture");
    let key = Midge::row_key(17, "one");
    let staged = keys(&publication, &key);
    publication.row_history.state.lock().sequence = u64::MAX;
    let submitted = Cell::new(false);

    // Act
    let result = publication.publish_rows_current(staged, || {
        submitted.set(true);
        Ok(())
    });

    // Assert
    assert_eq!(
        CassieError::from(result.expect_err("checked local counter"))
            .descriptor()
            .sql_state,
        "54000"
    );
    assert!(!submitted.get());
    assert!(owner.check_keys([key.as_slice()], &controls()).is_ok());
}

#[test]
fn should_install_no_history_after_a_known_failed_native_publication() {
    // Arrange
    let publication = publication(1024 * 1024);
    let owner = RowHistory::register(&publication).expect("capture");
    let key = Midge::row_key(17, "one");

    // Act
    let failed = publication.publish_rows_current(keys(&publication, &key), || {
        Err(cntryl_midge::MidgeError::WriteConflict(
            "known not applied".to_owned(),
        ))
    });
    let result = owner.check_keys([key.as_slice()], &controls());

    // Assert
    assert!(failed.is_err());
    assert!(result.is_ok());
    assert_eq!(publication.row_history.state.lock().sequence, 0);
}

#[test]
fn should_quarantine_history_after_an_unconfirmed_native_publication() {
    // Arrange
    let publication = publication(1024 * 1024);
    let owner = RowHistory::register(&publication).expect("capture");
    let key = Midge::row_key(17, "one");

    // Act
    let unknown = publication.publish_rows_current(keys(&publication, &key), || {
        Err(cntryl_midge::MidgeError::OutcomeUnknown(
            "may apply".to_owned(),
        ))
    });
    let result = owner.check_keys([key.as_slice()], &controls());

    // Assert
    assert!(matches!(
        unknown,
        Err(AdapterCommitError::Native(
            cntryl_midge::MidgeError::OutcomeUnknown(_)
        ))
    ));
    assert_eq!(
        result
            .expect_err("history is no longer authoritative")
            .descriptor()
            .sql_state,
        "58030"
    );
}

#[test]
fn should_classify_binary_relation_boundaries_before_column_row_markers() {
    // Arrange
    let relation = 0x0072_0000_0000_0000;
    let modern = Midge::row_key(relation, "one");
    let column = Midge::column_store_row_key(relation, "one");
    let field = crate::midge::adapter::key_encoding::column_store_field_key(relation, 3, "one");
    let legacy = Midge::doc_key("postgres.public.items", "one");
    let epoch = Midge::data_epoch_key();

    // Act
    let actual = [modern, column, field, legacy, epoch]
        .map(|key| crate::midge::adapter::key_encoding::is_authoritative_row_key(&key));

    // Assert
    assert_eq!(actual, [true, true, false, true, false]);
}

fn full_directory_update(budget: usize) -> Result<usize, CassieError> {
    let publication = publication(budget);
    let _owner = RowHistory::register(&publication)?;
    let mut staged = publication.row_history.key_log()?;
    for id in ["one", "two", "six", "ten"] {
        let key = Midge::row_key(17, id);
        let next = staged.prepare(&key)?;
        staged.install(next);
    }
    publication
        .publish_rows_current(staged, || Ok(()))
        .map_err(CassieError::from)?;
    publication
        .publish_rows_current(keys(&publication, &Midge::row_key(17, "one")), || Ok(()))
        .map_err(CassieError::from)?;
    let required_peak = publication
        .row_history
        .state
        .lock()
        .admission
        .as_ref()
        .expect("admitted tracker")
        .tracker_peak_bytes();
    Ok(required_peak)
}

#[test]
fn should_reuse_a_full_history_directory_for_existing_keys() {
    // Arrange
    let peak = full_directory_update(1024 * 1024).expect("measure initial directory overlap");

    // Act
    let updated = full_directory_update(peak);

    // Assert
    assert!(updated.is_ok());
}

#[test]
fn should_remember_a_delete_reinsert_of_the_same_payload() {
    // Arrange
    let midge = Midge::new_in_memory("postgres").expect("memory engine");
    midge.ensure_families_ready().expect("families");
    let key = Midge::row_key(17, "one");
    midge
        .raw_put(crate::midge::StorageFamily::Data, &key, b"same")
        .expect("seed");
    let owner = midge
        .capture_repeatable_row_history("postgres", &controls())
        .expect("capture");

    // Act
    midge
        .raw_delete(crate::midge::StorageFamily::Data, &key)
        .expect("delete");
    midge
        .raw_put(crate::midge::StorageFamily::Data, &key, b"same")
        .expect("same-payload reinsert");
    let result = owner.check_keys([key.as_slice()], &controls());

    // Assert
    assert_eq!(
        result
            .expect_err("ABA is a mutation")
            .descriptor()
            .sql_state,
        "40001"
    );
}

#[test]
fn should_reject_the_initial_header_before_creating_a_memory_tracker() {
    // Arrange
    let publication = publication(1);

    // Act
    let registered = RowHistory::register(&publication);

    // Assert
    assert_eq!(
        registered
            .expect_err("header exceeds budget")
            .descriptor()
            .sql_state,
        "54000"
    );
    let state = publication.row_history.state.lock();
    assert!(state.admission.is_none());
    assert!(state.active_memory.is_none());
    assert!(state.entries_memory.is_none());
    assert_eq!(state.active.capacity(), 0);
    assert_eq!(state.entries.capacity(), 0);
}

#[test]
fn should_complete_confirmed_history_with_no_remaining_admission() {
    // Arrange
    let budget = 1024 * 1024;
    let publication = publication(budget);
    let owner = RowHistory::register(&publication).expect("capture");
    let key = Midge::row_key(17, "one");
    let staged = keys(&publication, &key);
    let mut pressure = publication
        .row_history
        .state
        .lock()
        .admission
        .as_ref()
        .expect("admitted tracker")
        .try_sibling(0)
        .expect("separate pressure owner");

    // Act
    let published = publication.publish_rows_current(staged, || {
        let remaining = budget - pressure.tracker_used_bytes();
        pressure
            .try_grow(remaining)
            .expect("consume only after publication backing was admitted");
        assert_eq!(pressure.tracker_used_bytes(), budget);
        Ok(())
    });
    let changed = owner.check_keys([key.as_slice()], &controls());

    // Assert
    assert!(published.is_ok());
    assert_eq!(
        changed
            .expect_err("confirmed history installed without another reservation")
            .descriptor()
            .sql_state,
        "40001"
    );
}
