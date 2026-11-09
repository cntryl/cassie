use std::time::Instant;

use super::{NodeId, Progress, ValueSlots};
use crate::app::CassieError;
use crate::config::CassieRuntimeLimits;
use crate::runtime::{accounted::Accounted, QueryCancellationHandle, QueryExecutionControls};

fn controls() -> QueryExecutionControls {
    QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now())
}

fn value(number: usize, controls: &QueryExecutionControls) -> Accounted<usize> {
    Accounted::try_new(controls, std::mem::size_of::<usize>(), || number).expect("value")
}

#[test]
fn should_reuse_slots_without_revalidating_stale_handles() {
    // Arrange
    let controls = controls();
    let mut slots = ValueSlots::new(&controls).expect("slots");
    let first = slots.insert(value(1, &controls)).expect("first");
    let old = slots.remove(first).expect("remove");
    // Act
    let second = slots.insert(value(2, &controls)).expect("second");
    // Assert
    assert_eq!(first.index(), second.index());
    assert_ne!(first.generation(), second.generation());
    assert!(slots.get(first).is_err());
    assert_eq!(*slots.get(second).expect("new"), 2);
    assert_eq!(*old.get(), 1);
    drop(slots);
    assert_eq!(
        controls.current_query_memory_bytes(),
        std::mem::size_of::<usize>()
    );
    drop(old);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_retire_a_slot_instead_of_wrapping_its_generation() {
    // Arrange
    let controls = controls();
    let mut slots = ValueSlots::new(&controls).expect("slots");
    let initial = slots.insert(value(1, &controls)).expect("insert");
    let exhausted = slots.test_set_generation(initial, u64::MAX);
    drop(slots.remove(exhausted).expect("remove"));
    // Act
    let next = slots.insert(value(2, &controls)).expect("next");
    // Assert
    assert_ne!(exhausted.index(), next.index());
    assert!(slots.get(exhausted).is_err());
    assert_eq!(*slots.get(next).expect("next"), 2);
    drop(slots);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_reject_stale_and_out_of_range_slot_removal() {
    // Arrange
    let controls = controls();
    let mut slots = ValueSlots::new(&controls).expect("slots");
    let first = slots.insert(value(1, &controls)).expect("insert");
    drop(slots.remove(first).expect("remove"));
    // Act / Assert
    assert!(slots.remove(first).is_err());
    assert!(slots
        .remove(NodeId::new(first.namespace, usize::MAX, 1))
        .is_err());
    drop(slots);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_preserve_live_slots_when_cancelled_during_chunk_directory_growth() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let mut slots = ValueSlots::new(&controls).expect("slots");
    let mut ids = Vec::new();
    for number in 0..16 {
        ids.push(slots.insert(value(number, &controls)).expect("insert"));
    }
    let candidate = value(16, &controls);
    // Act
    let result = slots.insert_observed(candidate, |progress| {
        if matches!(progress, Progress::Move) {
            cancellation.cancel();
        }
        Ok(())
    });
    // Assert
    assert!(matches!(result, Err(CassieError::QueryCancelled)));
    assert!(!slots.is_poisoned());
    for (number, id) in ids.into_iter().enumerate() {
        assert_eq!(*slots.get(id).expect("live"), number);
    }
    drop(slots);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_keep_an_empty_published_chunk_owned_when_cancelled_before_slot_insert() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let mut slots = ValueSlots::new(&controls).expect("slots");
    let first = slots.insert(value(0, &controls)).expect("first");
    for number in 1..4 {
        slots.insert(value(number, &controls)).expect("insert");
    }

    // Act
    let result = slots.insert_observed(value(4, &controls), |progress| {
        if matches!(progress, Progress::ChunkPublished) {
            cancellation.cancel();
        }
        Ok(())
    });

    // Assert
    assert!(matches!(result, Err(CassieError::QueryCancelled)));
    assert_eq!(*slots.get(first).expect("live"), 0);
    assert_eq!(slots.length, 4);
    assert!(slots.chunks.get(1).is_some());
    drop(slots);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_reuse_slots_on_either_side_of_an_interrupted_chunk_migration() {
    // Arrange
    let controls = controls();
    let mut slots = ValueSlots::new(&controls).expect("slots");
    let mut ids = Vec::new();
    for number in 0..16 {
        ids.push(slots.insert(value(number, &controls)).expect("insert"));
    }
    let result = slots.insert_observed(value(16, &controls), |progress| match progress {
        Progress::Move => Err(CassieError::QueryCancelled),
        Progress::ChunkPublished => Ok(()),
    });
    assert!(matches!(result, Err(CassieError::QueryCancelled)));

    // Act
    drop(slots.remove(ids[0]).expect("moved"));
    drop(slots.remove(ids[15]).expect("unmoved"));
    let last = slots.insert(value(115, &controls)).expect("reuse unmoved");
    let first = slots.insert(value(100, &controls)).expect("reuse moved");
    let added = slots.insert(value(16, &controls)).expect("resume");

    // Assert
    assert_eq!(last.index(), ids[15].index());
    assert_eq!(first.index(), ids[0].index());
    assert!(slots.get(ids[0]).is_err());
    assert!(slots.get(ids[15]).is_err());
    assert_eq!(*slots.get(last).expect("last"), 115);
    assert_eq!(*slots.get(first).expect("first"), 100);
    assert_eq!(*slots.get(added).expect("added"), 16);
    drop(slots);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_reuse_an_empty_published_chunk_after_interruption() {
    // Arrange
    let controls = controls();
    let mut slots = ValueSlots::new(&controls).expect("slots");
    for number in 0..4 {
        slots.insert(value(number, &controls)).expect("insert");
    }
    let result = slots.insert_observed(value(4, &controls), |progress| match progress {
        Progress::ChunkPublished => Err(CassieError::QueryCancelled),
        Progress::Move => Ok(()),
    });
    assert!(matches!(result, Err(CassieError::QueryCancelled)));
    let retained = controls.current_query_memory_bytes();

    // Act
    let id = slots.insert(value(4, &controls)).expect("reuse chunk");

    // Assert
    assert_eq!(id.index(), 4);
    assert_eq!(*slots.get(id).expect("live"), 4);
    assert_eq!(
        controls.current_query_memory_bytes(),
        retained + std::mem::size_of::<usize>()
    );
    assert!(slots.chunks.get(2).is_none());
    drop(slots);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_reject_a_value_owned_by_another_query_tracker() {
    // Arrange
    let owner_controls = controls();
    let other = controls();
    let mut slots = ValueSlots::new(&owner_controls).expect("slots");
    let candidate = value(1, &other);
    // Act
    let result = slots.insert(candidate);
    // Assert
    assert!(matches!(result, Err(CassieError::Execution(_))));
    assert_eq!(other.current_query_memory_bytes(), 0);
    assert_eq!(owner_controls.current_query_memory_bytes(), 0);
    drop(slots);
}

#[test]
fn should_preserve_live_handles_when_slot_growth_is_denied() {
    // Arrange
    let directory = 4 * std::mem::size_of::<Option<super::Chunk<usize>>>();
    let chunks = 5 * super::CHUNK_SIZE * std::mem::size_of::<Option<super::Slot<usize>>>();
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: directory + chunks + 17 * std::mem::size_of::<usize>(),
        ..CassieRuntimeLimits::default()
    };
    let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
    let mut slots = ValueSlots::new(&controls).expect("slots");
    let mut ids = Vec::new();
    for number in 0..16 {
        ids.push(slots.insert(value(number, &controls)).expect("insert"));
    }
    let baseline = controls.current_query_memory_bytes();
    let candidate = value(16, &controls);
    // Act
    let result = slots.insert(candidate);
    // Assert
    assert!(matches!(result, Err(CassieError::ResourceLimit(_))));
    assert!(!slots.is_poisoned());
    for (number, id) in ids.into_iter().enumerate() {
        assert_eq!(*slots.get(id).expect("live"), number);
    }
    assert_eq!(controls.current_query_memory_bytes(), baseline);
    drop(slots);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_reject_handles_from_another_arena_on_the_same_tracker() {
    // Arrange
    let controls = controls();
    let mut first = ValueSlots::new(&controls).expect("first");
    let mut second = ValueSlots::new(&controls).expect("second");
    let first_id = first.insert(value(1, &controls)).expect("first value");
    let second_id = second.insert(value(2, &controls)).expect("second value");
    // Act / Assert
    assert_ne!(first_id, second_id);
    assert!(second.get(first_id).is_err());
    assert!(second.remove(first_id).is_err());
    assert_eq!(*first.get(first_id).expect("first"), 1);
    assert_eq!(*second.get(second_id).expect("second"), 2);
    drop(first);
    drop(second);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_fail_closed_when_arena_namespace_is_exhausted() {
    // Arrange
    let counter = std::sync::atomic::AtomicU64::new(u64::MAX - 1);
    // Act
    let last = super::allocate_namespace(&counter).expect("last namespace");
    let exhausted = super::allocate_namespace(&counter);
    // Assert
    assert_eq!(last, u64::MAX - 1);
    assert!(matches!(exhausted, Err(CassieError::ResourceLimit(_))));
    assert_eq!(counter.load(std::sync::atomic::Ordering::Relaxed), u64::MAX);
}

#[test]
fn should_reject_handle_after_original_arena_has_been_dropped() {
    // Arrange
    let controls = controls();
    let mut first = ValueSlots::new(&controls).expect("first");
    let old = first.insert(value(1, &controls)).expect("value");
    drop(first);
    let mut second = ValueSlots::new(&controls).expect("second");
    let new = second.insert(value(2, &controls)).expect("value");
    // Act / Assert
    assert!(second.get(old).is_err());
    assert!(second.remove(old).is_err());
    assert_eq!(*second.get(new).expect("live"), 2);
    drop(second);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_allocate_unique_arena_namespaces_concurrently() {
    // Arrange
    let counter = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(1));
    let mut threads = Vec::new();
    // Act
    for _ in 0..4 {
        let counter = counter.clone();
        threads.push(std::thread::spawn(move || {
            (0..100)
                .map(|_| super::allocate_namespace(&counter).expect("namespace"))
                .collect::<Vec<_>>()
        }));
    }
    let mut names = threads
        .into_iter()
        .flat_map(|thread| thread.join().expect("thread"))
        .collect::<Vec<_>>();
    names.sort_unstable();
    names.dedup();
    // Assert
    assert_eq!(names.len(), 400);
}
