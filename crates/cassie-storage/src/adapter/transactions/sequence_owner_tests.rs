//! Transaction-owned sequence cursor and fork lease invariants.

use super::{decode, CheckingReader, StorageOverlay, CURSOR_BYTES};
use crate::catalog::SequenceMeta;
use crate::adapter::request_test_support::TestRequest;
use crate::adapter::NativePublicationControlScope;
use cassie_core::request::{RequestCancellation, RequestMemory};
use cassie_core::types::DataType;

fn controls() -> TestRequest {
    TestRequest::with_budget(CURSOR_BYTES)
}

#[test]
fn should_bound_with_retire_transaction_cursor_owners_across_statements() {
    // Arrange
    let overlay = StorageOverlay::default();
    let owner = &overlay.sequence_cursors;
    let mut first = SequenceMeta::new("first", DataType::BigInt);
    first.object_id = 11;
    let mut second = SequenceMeta::new("second", DataType::BigInt);
    second.object_id = 12;
    let statement_one = controls();
    let statement_two = controls();

    // Act
    let first_value = {
        let _scope = NativePublicationControlScope::enter(Some(&statement_one.context()));
        owner.advance(first, 0, None, |sequence| {
            sequence.advance().map_err(Into::into)
        })
    };
    let rejected = {
        let _scope = NativePublicationControlScope::enter(Some(&statement_two.context()));
        owner.advance(second.clone(), 0, None, |sequence| {
            sequence.advance().map_err(Into::into)
        })
    };
    {
        let _scope = NativePublicationControlScope::enter(Some(&statement_two.context()));
        overlay
            .prune_sequence_cursors_with(std::iter::empty(), &statement_two.context())
            .expect("prune retired identity");
    }
    let second_value = {
        let _scope = NativePublicationControlScope::enter(Some(&statement_two.context()));
        owner.advance(second, 0, None, |sequence| {
            sequence.advance().map_err(Into::into)
        })
    };
    let retained = owner
        .state
        .lock()
        .admission
        .as_ref()
        .expect("transaction admission")
        .bytes();
    {
        let _scope = NativePublicationControlScope::enter(Some(&statement_two.context()));
        overlay
            .prune_sequence_cursors_with(std::iter::empty(), &statement_two.context())
            .expect("retire final identity");
    }
    let released = owner
        .state
        .lock()
        .admission
        .as_ref()
        .expect("stable admission")
        .bytes();

    // Assert
    assert_eq!(first_value.expect("first cursor").0, 1);
    assert!(rejected.is_err());
    assert_eq!(second_value.expect("second cursor").0, 1);
    assert_eq!(retained, CURSOR_BYTES);
    assert_eq!(released, 0);
    assert_eq!(statement_one.current_query_memory_bytes(), 0);
    assert_eq!(statement_two.current_query_memory_bytes(), 0);
}

#[test]
fn should_admit_with_interrupt_sequence_metadata_decode_at_its_boundary() {
    // Arrange
    let metadata = SequenceMeta::new("sequence", DataType::BigInt);
    let raw = serde_json::to_vec(&metadata).expect("sequence metadata");
    let bytes =
        cassie_core::request::accounted::json::serialized_decode_bytes(raw.len()).expect("decode bound");
    let below = TestRequest::with_budget(bytes - 1);
    let exact = TestRequest::with_budget(bytes);
    let cancellation = RequestCancellation::new();
    let interrupted = TestRequest::with_budget(bytes).using_cancellation(cancellation.clone());
    cancellation.cancel();

    // Act
    let rejected = decode(&raw, &below.context());
    let admitted = decode(&raw, &exact.context());
    let canceled = decode(&raw, &interrupted.context());
    let held = exact.current_query_memory_bytes();
    drop(admitted);

    // Assert
    assert!(rejected.is_err());
    assert!(matches!(
        canceled,
        Err( crate::adapter::CassieError::QueryCanceled)
    ));
    assert_eq!(held, bytes);
    assert_eq!(exact.current_query_memory_bytes(), 0);
}

#[test]
fn should_keep_a_cursor_referenced_only_by_a_retained_savepoint() {
    // Arrange
    let controls = TestRequest::with_budget(1024 * 1024);
    let mut current = StorageOverlay::default();
    let mut sequence = SequenceMeta::new("saved", DataType::BigInt);
    sequence.object_id = 91;
    let _scope = NativePublicationControlScope::enter(Some(&controls.context()));
    let first = current
        .sequence_cursors
        .advance(sequence.clone(), 0, None, |value| {
            value.advance().map_err(Into::into)
        });
    let key = super::super::super::key_encoding::sequence_key("saved");
    current.changes.insert(
        key,
        super::super::StorageMutation {
            value: Some(serde_json::to_vec(&sequence).expect("sequence metadata")),
            ttl_seconds: None,
            expected: super::super::ExpectedStorageValue::Missing,
        },
    );
    let savepoint = current.clone();
    current.changes.clear();

    // Act
    current
        .prune_sequence_cursors_with(std::iter::once(&savepoint), &controls.context())
        .expect("preserve savepoint");
    let second = current
        .sequence_cursors
        .advance(sequence, 0, None, |value| {
            value.advance().map_err(Into::into)
        });
    current
        .prune_sequence_cursors_with(std::iter::empty(), &controls.context())
        .expect("retire released owner");
    let retained = current
        .sequence_cursors
        .state
        .lock()
        .admission
        .as_ref()
        .expect("owner")
        .bytes();

    // Assert
    assert_eq!(first.expect("first").0, 1);
    assert_eq!(second.expect("savepoint cursor").0, 2);
    assert_eq!(retained, 0);
}

#[test]
fn should_interrupt_metadata_reader_at_a_deterministic_chunk_boundary() {
    // Arrange
    use std::io::Read;
    let cancellation = RequestCancellation::new();
    let controls = TestRequest::with_cancellation(cancellation.clone());
    let input = vec![b'x'; 4097];
    let mut reader = CheckingReader {
        bytes: &input,
        position: 0,
        controls: &controls.context(),
    };
    let mut chunk = [0; 4096];
    reader.read_exact(&mut chunk).expect("first chunk");
    cancellation.cancel();

    // Act
    let result = reader.read(&mut [0]);

    // Assert
    assert!(result.is_err());
    assert_eq!(reader.position, 4096);
}

#[test]
fn should_retain_an_inflight_fork_cursor_until_the_fork_retires() {
    // Arrange
    let controls = TestRequest::with_budget(1024 * 1024);
    let _scope = NativePublicationControlScope::enter(Some(&controls.context()));
    let parent = StorageOverlay::default();
    let mut fork = parent.clone();
    fork.attach_sequence_fork_lease(Some(&controls.context()))
        .expect("fork admission");
    let mut sequence = SequenceMeta::new("fork_only", DataType::BigInt);
    sequence.object_id = 111;
    fork.advance_private_for_test(sequence, 17, &controls.context())
        .expect("fork allocation");

    // Act
    parent
        .prune_sequence_cursors_with(std::iter::empty(), &controls.context())
        .expect("concurrent prune");
    let retained_during_fork = parent
        .sequence_cursors
        .state
        .lock()
        .cursors
        .contains_key(&(111, 17));
    drop(fork);
    parent
        .prune_sequence_cursors_with(std::iter::empty(), &controls.context())
        .expect("post-fork prune");
    let retained_after_fork = parent
        .sequence_cursors
        .state
        .lock()
        .cursors
        .contains_key(&(111, 17));

    // Assert
    assert!(retained_during_fork);
    assert!(!retained_after_fork);
}

#[test]
fn should_reference_count_duplicate_fork_cursor_leases() {
    // Arrange
    let controls = TestRequest::with_budget(1024 * 1024);
    let _scope = NativePublicationControlScope::enter(Some(&controls.context()));
    let parent = StorageOverlay::default();
    let mut sequence = SequenceMeta::new("shared_fork", DataType::BigInt);
    sequence.object_id = 112;
    parent
        .sequence_cursors
        .advance(sequence, 0, None, |value| {
            value.advance().map_err(Into::into)
        })
        .expect("cursor");
    let mut first = parent.clone();
    let mut second = parent.clone();
    first
        .attach_sequence_fork_lease(Some(&controls.context()))
        .expect("first fork");
    second
        .attach_sequence_fork_lease(Some(&controls.context()))
        .expect("second fork");

    // Act
    drop(first);
    parent
        .prune_sequence_cursors_with(std::iter::empty(), &controls.context())
        .expect("one fork remains");
    let retained = parent
        .sequence_cursors
        .state
        .lock()
        .inflight
        .get(&(112, 0))
        .copied();
    drop(second);
    parent
        .prune_sequence_cursors_with(std::iter::empty(), &controls.context())
        .expect("forks retired");
    let state = parent.sequence_cursors.state.lock();
    let remaining = state.cursors.len();
    let used = state
        .admission
        .as_ref()
        .expect("admission")
        .tracker_used_bytes();

    // Assert
    assert_eq!(retained, Some(1));
    assert_eq!(remaining, 0);
    assert_eq!(used, 0);
}

#[test]
fn should_keep_fork_only_sequence_epoch_through_prune_with_publication() {
    // Arrange
    use std::sync::Arc;
    let controls = TestRequest::with_budget(1024 * 1024);
    let _controls = NativePublicationControlScope::enter(Some(&controls.context()));
    let midge = crate::adapter::Midge::new_in_memory("postgres").expect("memory Midge");
    midge.ensure_families_ready().expect("families");
    let parent = Arc::new(parking_lot::Mutex::new(StorageOverlay::default()));
    let _parent_scope = midge
        .enter_transactional_ddl_scope("postgres", Arc::clone(&parent), true)
        .expect("parent scope");
    let mut sequence = SequenceMeta::new("fork_epoch", DataType::BigInt);
    sequence.object_id = 113;
    midge.put_sequence(&sequence).expect("stage sequence");
    let first = midge
        .update_sequence_retrying("fork_epoch", |value| value.advance().map_err(Into::into))
        .expect("first")
        .0;
    let mut fork_value = parent.lock().clone();
    fork_value
        .attach_sequence_fork_lease(Some(&controls.context()))
        .expect("fork lease");
    let fork = Arc::new(parking_lot::Mutex::new(fork_value));
    let fork_scope = midge
        .enter_transactional_ddl_scope("postgres", Arc::clone(&fork), true)
        .expect("fork scope");
    midge
        .update_sequence("fork_epoch", |value| {
            value.assign(10, false)?;
            Ok(())
        })
        .expect("restart");
    let fork_value = midge
        .update_sequence_retrying("fork_epoch", |value| value.advance().map_err(Into::into))
        .expect("fork allocation")
        .0;

    // Act
    parent
        .lock()
        .prune_sequence_cursors_with(std::iter::empty(), &controls.context())
        .expect("concurrent prune");
    let mut published = fork.lock().clone();
    published.clear_sequence_fork_lease();
    *parent.lock() = published;
    drop(fork_scope);
    drop(fork);
    let after = midge
        .update_sequence_retrying("fork_epoch", |value| value.advance().map_err(Into::into))
        .expect("after publish")
        .0;

    // Assert
    assert_eq!(first, 1);
    assert_eq!(fork_value, 10);
    assert_eq!(after, 11);
}

#[test]
fn should_not_prune_parent_cursors_from_an_older_fork_view() {
    // Arrange
    let controls = TestRequest::with_budget(1024 * 1024);
    let _scope = NativePublicationControlScope::enter(Some(&controls.context()));
    let parent = StorageOverlay::default();
    let mut fork = parent.clone();
    fork.attach_sequence_fork_lease(Some(&controls.context()))
        .expect("fork");
    let mut later = SequenceMeta::new("later_parent", DataType::BigInt);
    later.object_id = 114;
    parent
        .sequence_cursors
        .advance(later, 0, None, |value| value.advance().map_err(Into::into))
        .expect("parent allocation");

    // Act
    fork.prune_sequence_cursors_with(std::iter::empty(), &controls.context())
        .expect("fork preflight");
    let retained = parent
        .sequence_cursors
        .state
        .lock()
        .cursors
        .contains_key(&(114, 0));

    // Assert
    assert!(retained);
}

#[test]
fn should_leave_the_previous_private_epoch_unchanged_when_restart_is_denied() {
    // Arrange
    use std::sync::Arc;
    let budget = 1024 * 1024;
    let controls = TestRequest::with_budget(budget);
    let _controls = NativePublicationControlScope::enter(Some(&controls.context()));
    let midge = crate::adapter::Midge::new_in_memory("postgres").expect("memory Midge");
    midge.ensure_families_ready().expect("families");
    let parent = Arc::new(parking_lot::Mutex::new(StorageOverlay::default()));
    let _scope = midge
        .enter_transactional_ddl_scope("postgres", Arc::clone(&parent), true)
        .expect("scope");
    let mut sequence = SequenceMeta::new("denied_restart", DataType::BigInt);
    sequence.object_id = 115;
    midge.put_sequence(&sequence).expect("stage sequence");
    midge
        .update_sequence_retrying("denied_restart", |value| {
            value.advance().map_err(Into::into)
        })
        .expect("first");
    {
        let overlay = parent.lock();
        let mut state = overlay.sequence_cursors.state.lock();
        state.admission = Some(
            RequestMemory::new(CURSOR_BYTES).reserve(CURSOR_BYTES)
                .expect("exact owner budget"),
        );
    }

    // Act
    let denied = midge.update_sequence("denied_restart", |value| {
        value.assign(10, false)?;
        Ok(())
    });
    let next = midge.update_sequence_retrying("denied_restart", |value| {
        value.advance().map_err(Into::into)
    });
    let overlay = parent.lock();
    let state = overlay.sequence_cursors.state.lock();

    // Assert
    assert!(denied.is_err());
    assert_eq!(next.expect("original epoch").0, 2);
    assert_eq!(state.next_epoch, 0);
    assert_eq!(state.cursors.len(), 1);
    assert!(overlay.sequence_epochs.is_none());
}

#[test]
fn should_leave_the_previous_private_epoch_unchanged_when_restart_is_canceled() {
    // Arrange
    use std::sync::Arc;
    let budget = 1024 * 1024;
    let controls = TestRequest::with_budget(budget);
    let controls_scope = NativePublicationControlScope::enter(Some(&controls.context()));
    let midge = crate::adapter::Midge::new_in_memory("postgres").expect("memory Midge");
    midge.ensure_families_ready().expect("families");
    let parent = Arc::new(parking_lot::Mutex::new(StorageOverlay::default()));
    let _scope = midge
        .enter_transactional_ddl_scope("postgres", Arc::clone(&parent), true)
        .expect("scope");
    let mut sequence = SequenceMeta::new("canceled_restart", DataType::BigInt);
    sequence.object_id = 116;
    midge.put_sequence(&sequence).expect("stage sequence");
    midge
        .update_sequence_retrying("canceled_restart", |value| {
            value.advance().map_err(Into::into)
        })
        .expect("first");
    drop(controls_scope);
    let cancellation = RequestCancellation::new();
    let canceled_controls =
        TestRequest::with_budget(budget).using_cancellation(cancellation.clone());
    cancellation.cancel();

    // Act
    let canceled = {
        let _canceled = NativePublicationControlScope::enter(Some(&canceled_controls.context()));
        midge.update_sequence("canceled_restart", |value| {
            value.assign(10, false)?;
            Ok(())
        })
    };
    let _normal_controls = NativePublicationControlScope::enter(Some(&controls.context()));
    let next = midge.update_sequence_retrying("canceled_restart", |value| {
        value.advance().map_err(Into::into)
    });

    // Assert
    assert!(matches!(
        canceled,
        Err( crate::adapter::CassieError::QueryCanceled)
    ));
    assert_eq!(next.expect("original epoch").0, 2);
    assert!(parent.lock().sequence_epochs.is_none());
}

#[test]
fn should_transfer_inner_epoch_protection_to_an_unpublished_parent_fork() {
    // Arrange
    let controls = TestRequest::with_budget(1024 * 1024);
    let _scope = NativePublicationControlScope::enter(Some(&controls.context()));
    let parent = StorageOverlay::default();
    let mut outer = parent.clone();
    outer
        .attach_sequence_fork_lease(Some(&controls.context()))
        .expect("outer fork");
    let mut inner = outer.clone();
    inner
        .attach_sequence_fork_lease(Some(&controls.context()))
        .expect("inner fork");
    let mut sequence = SequenceMeta::new("inner_only", DataType::BigInt);
    sequence.object_id = 117;
    inner
        .advance_private_for_test(sequence, 23, &controls.context())
        .expect("inner epoch");

    // Act
    let mut published = inner.clone();
    published
        .inherit_sequence_fork_lease(&outer, Some(&controls.context()))
        .expect("transfer lease");
    outer = published;
    drop(inner);
    parent
        .prune_sequence_cursors_with(std::iter::empty(), &controls.context())
        .expect("parent prune");
    let retained = parent
        .sequence_cursors
        .state
        .lock()
        .cursors
        .contains_key(&(117, 23));
    drop(outer);
    parent
        .prune_sequence_cursors_with(std::iter::empty(), &controls.context())
        .expect("outer retired");
    let released = !parent
        .sequence_cursors
        .state
        .lock()
        .cursors
        .contains_key(&(117, 23));

    // Assert
    assert!(retained);
    assert!(released);
}

#[test]
fn should_discard_a_fork_restart_when_the_fork_is_aborted() {
    // Arrange
    use std::sync::Arc;
    let controls = TestRequest::with_budget(1024 * 1024);
    let _controls = NativePublicationControlScope::enter(Some(&controls.context()));
    let midge = crate::adapter::Midge::new_in_memory("postgres").expect("memory Midge");
    midge.ensure_families_ready().expect("families");
    let parent = Arc::new(parking_lot::Mutex::new(StorageOverlay::default()));
    let _parent_scope = midge
        .enter_transactional_ddl_scope("postgres", Arc::clone(&parent), true)
        .expect("parent scope");
    let mut sequence = SequenceMeta::new("aborted_fork_epoch", DataType::BigInt);
    sequence.object_id = 118;
    midge.put_sequence(&sequence).expect("stage sequence");
    midge
        .update_sequence_retrying("aborted_fork_epoch", |value| {
            value.advance().map_err(Into::into)
        })
        .expect("first");
    let mut fork_value = parent.lock().clone();
    fork_value
        .attach_sequence_fork_lease(Some(&controls.context()))
        .expect("fork lease");
    let fork = Arc::new(parking_lot::Mutex::new(fork_value));
    {
        let _fork_scope = midge
            .enter_transactional_ddl_scope("postgres", Arc::clone(&fork), true)
            .expect("fork scope");
        midge
            .update_sequence("aborted_fork_epoch", |value| {
                value.assign(10, false)?;
                Ok(())
            })
            .expect("restart");
        midge
            .update_sequence_retrying("aborted_fork_epoch", |value| {
                value.advance().map_err(Into::into)
            })
            .expect("fork allocation");
    }

    // Act
    drop(fork);
    parent
        .lock()
        .prune_sequence_cursors_with(std::iter::empty(), &controls.context())
        .expect("retire aborted epoch");
    let next = midge.update_sequence_retrying("aborted_fork_epoch", |value| {
        value.advance().map_err(Into::into)
    });

    // Assert
    assert_eq!(next.expect("parent epoch").0, 2);
    assert!(parent.lock().sequence_epochs.is_none());
}

#[test]
fn should_interrupt_metadata_writer_at_a_deterministic_chunk_boundary() {
    // Arrange
    use std::io::Write;
    let cancellation = RequestCancellation::new();
    let controls = TestRequest::with_cancellation(cancellation.clone());
    let mut output = Vec::with_capacity(4097);
    let mut writer = super::CheckingWriter {
        output: &mut output,
        position: 0,
        controls: &controls.context(),
    };
    let chunk = [b'x'; 4096];
    writer.write_all(&chunk).expect("first chunk");
    cancellation.cancel();

    // Act
    let result = writer.write(b"x");

    // Assert
    assert!(result.is_err());
    assert_eq!(writer.position, 4096);
}

#[test]
fn should_encode_sequence_metadata_like_the_existing_serde_format() {
    // Arrange
    let controls =
        TestRequest::default();
    let mut metadata = SequenceMeta::new("unicode-é-\\\"-\n-\u{1}", DataType::BigInt);
    metadata.object_id = 999;
    metadata.owned_by = Some(crate::catalog::SequenceOwner {
        table: "table\\name".into(),
        column: "column\t名".into(),
    });
    let expected = serde_json::to_vec(&metadata).expect("existing format");
    let mut actual = Vec::new();

    // Act
    let result = super::encode_sequence_metadata(&mut actual, &metadata, &controls.context());

    // Assert
    result.expect("encode metadata");
    assert_eq!(actual, expected);
}

#[test]
fn should_treat_private_next_sequence_api_as_nontransactional_advance() {
    // Arrange
    use std::sync::Arc;
    let controls =
        TestRequest::default();
    let _controls = NativePublicationControlScope::enter(Some(&controls.context()));
    let midge = crate::adapter::Midge::new_in_memory("postgres").expect("memory Midge");
    midge.ensure_families_ready().expect("families");
    let parent = Arc::new(parking_lot::Mutex::new(StorageOverlay::default()));
    let _scope = midge
        .enter_transactional_ddl_scope("postgres", Arc::clone(&parent), true)
        .expect("scope");
    let mut metadata = SequenceMeta::new("public_next_value", DataType::BigInt);
    metadata.object_id = 119;
    midge.put_sequence(&metadata).expect("stage sequence");
    let first = midge
        .next_sequence_value("public_next_value")
        .expect("first");
    let saved = parent.lock().clone();
    let second = midge
        .next_sequence_value("public_next_value")
        .expect("second");
    *parent.lock() = saved;

    // Act
    let third = midge
        .next_sequence_value("public_next_value")
        .expect("third");

    // Assert
    assert_eq!((first, second, third), (1, 2, 3));
}

#[test]
fn should_interrupt_a_long_unescaped_metadata_name_during_serialization() {
    // Arrange
    use std::io::{self, Write};
    struct CancelAfterFirstChunk {
        cancellation: RequestCancellation,
        bytes: usize,
    }
    impl Write for CancelAfterFirstChunk {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.bytes += bytes.len();
            if self.bytes >= 4096 {
                self.cancellation.cancel();
            }
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let cancellation = RequestCancellation::new();
    let controls = TestRequest::with_cancellation(cancellation.clone());
    let metadata = SequenceMeta::new("x".repeat(8192), DataType::BigInt);
    let mut writer = CancelAfterFirstChunk {
        cancellation,
        bytes: 0,
    };

    // Act
    let result = super::encode_sequence_metadata(&mut writer, &metadata, &controls.context());

    // Assert
    assert!(matches!(
        result,
        Err( crate::adapter::CassieError::QueryCanceled)
    ));
    assert!(writer.bytes < metadata.name.len());
}
