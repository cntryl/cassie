use std::time::Instant;

use super::{Key, ObjectIndex, Progress};
use crate::app::CassieError;
use crate::config::CassieRuntimeLimits;
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};

fn controls() -> QueryExecutionControls {
    QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now())
}

#[test]
fn should_reject_a_key_owned_by_another_query_tracker() {
    // Arrange
    let owner_controls = controls();
    let other_controls = controls();
    let mut object: ObjectIndex<usize> = ObjectIndex::new(&owner_controls).expect("object");
    let key = Key::new("key", &other_controls).expect("key");

    // Act
    let result = object.insert(key, 1);

    // Assert
    assert!(matches!(result, Err(CassieError::Execution(_))));
    assert_eq!(object.test_order(), Vec::<&str>::new());
    assert_eq!(other_controls.current_query_memory_bytes(), 0);
    assert_eq!(owner_controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_keep_avl_order_with_balance_for_all_rotation_shapes() {
    // Arrange
    let controls = controls();
    let shapes = [
        ["c", "b", "a"],
        ["a", "b", "c"],
        ["c", "a", "b"],
        ["a", "c", "b"],
    ];
    // Act / Assert
    for keys in shapes {
        let mut object: ObjectIndex<usize> = ObjectIndex::new(&controls).expect("object");
        for (value, key) in keys.into_iter().enumerate() {
            let owned = Key::new(key, &controls).expect("key");
            // Assert
            assert_eq!(object.insert(owned, value).expect("insert"), None);
        }
        assert_eq!(object.test_order(), ["a", "b", "c"]);
        assert!(object.test_invariants());
        drop(object);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_keep_existing_key_with_return_old_value_on_duplicate() {
    // Arrange
    let controls = controls();
    let mut object: ObjectIndex<usize> = ObjectIndex::new(&controls).expect("object");
    object
        .insert(Key::new("key", &controls).expect("key"), 1)
        .expect("insert");
    let retained = controls.current_query_memory_bytes();
    let duplicate = Key::new("key", &controls).expect("key");
    // Act
    let previous = object.insert(duplicate, 2).expect("replace");
    // Assert
    assert_eq!(previous, Some(1));
    assert_eq!(object.test_value("key"), Some(2));
    assert_eq!(controls.current_query_memory_bytes(), retained);
    assert!(object.test_invariants());
    drop(object);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_cancel_checked_key_comparison_without_mutating_object() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let mut object: ObjectIndex<usize> = ObjectIndex::new(&controls).expect("object");
    let first = format!("{}a", "x".repeat(2048));
    let second = format!("{}b", "x".repeat(2048));
    object
        .insert(Key::new(&first, &controls).expect("key"), 1)
        .expect("insert");
    let baseline = controls.current_query_memory_bytes();
    let candidate = Key::new(&second, &controls).expect("key");
    // Act
    let result = object.insert_observed(candidate, 2, |progress| {
        if matches!(progress, Progress::Compare) {
            cancellation.cancel();
        }
        Ok(())
    });
    // Assert
    assert!(matches!(result, Err(CassieError::QueryCancelled)));
    assert!(!object.is_poisoned());
    assert_eq!(object.test_value(&first), Some(1));
    assert_eq!(object.test_value(&second), None);
    assert_eq!(controls.current_query_memory_bytes(), baseline);
    drop(object);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_preserve_tree_when_cancelled_during_chunk_directory_growth() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let mut object: ObjectIndex<usize> = ObjectIndex::new(&controls).expect("object");
    for key in [
        "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p",
    ] {
        object
            .insert(Key::new(key, &controls).expect("key"), 1)
            .expect("insert");
    }
    let candidate = Key::new("q", &controls).expect("key");
    // Act
    let result = object.insert_observed(candidate, 2, |progress| {
        if matches!(progress, Progress::Move) {
            cancellation.cancel();
        }
        Ok(())
    });
    // Assert
    assert!(matches!(result, Err(CassieError::QueryCancelled)));
    assert!(!object.is_poisoned());
    assert_eq!(object.test_value("a"), Some(1));
    assert_eq!(object.test_value("p"), Some(1));
    assert_eq!(object.test_value("q"), None);
    assert!(object.test_invariants());
    drop(object);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_resume_object_insertion_after_interrupted_directory_migration() {
    // Arrange
    let controls = controls();
    let mut object: ObjectIndex<usize> = ObjectIndex::new(&controls).expect("object");
    for key in [
        "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p",
    ] {
        object
            .insert(Key::new(key, &controls).expect("key"), 1)
            .expect("insert");
    }
    let result = object.insert_observed(Key::new("q", &controls).expect("key"), 2, |progress| {
        if matches!(progress, Progress::Move) {
            return Err(CassieError::QueryCancelled);
        }
        Ok(())
    });
    assert!(matches!(result, Err(CassieError::QueryCancelled)));

    // Act
    object
        .insert(Key::new("q", &controls).expect("key"), 2)
        .expect("resume");

    // Assert
    assert_eq!(object.test_value("a"), Some(1));
    assert_eq!(object.test_value("p"), Some(1));
    assert_eq!(object.test_value("q"), Some(2));
    assert!(object.test_invariants());
    drop(object);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_reuse_an_entry_chunk_published_before_interrupted_insertion() {
    // Arrange
    let controls = controls();
    let mut object: ObjectIndex<usize> = ObjectIndex::new(&controls).expect("object");
    for key in ["a", "b", "c", "d"] {
        object
            .insert(Key::new(key, &controls).expect("key"), 1)
            .expect("insert");
    }
    let result = object.insert_observed(Key::new("e", &controls).expect("key"), 2, |progress| {
        if matches!(progress, Progress::ChunkPublished) {
            return Err(CassieError::QueryCancelled);
        }
        Ok(())
    });
    assert!(matches!(result, Err(CassieError::QueryCancelled)));
    let retained = controls.current_query_memory_bytes();

    // Act
    object
        .insert(Key::new("e", &controls).expect("key"), 2)
        .expect("resume");

    // Assert
    assert_eq!(object.test_value("e"), Some(2));
    assert!(object.test_invariants());
    assert_eq!(controls.current_query_memory_bytes(), retained + 1);
    assert!(object.entries.get(2).is_none());
    drop(object);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_balance_monotonic_object_insertions_without_count_cap() {
    // Arrange
    let controls = controls();
    let mut object: ObjectIndex<usize> = ObjectIndex::new(&controls).expect("object");
    // Act / Assert
    for index in 0..200 {
        let key = format!("key-{index:04}");
        object
            .insert(Key::new(&key, &controls).expect("key"), index)
            .expect("insert");
        // Assert
        assert!(object.test_invariants(), "index {index}");
    }
    let order = object.test_order();
    assert_eq!(order.len(), 200);
    assert_eq!(order.first(), Some(&"key-0000"));
    assert_eq!(order.last(), Some(&"key-0199"));
    drop(order);
    drop(object);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_preserve_object_when_directory_growth_is_denied() {
    // Arrange
    let directory = 4 * std::mem::size_of::<Option<super::EntryChunk<usize>>>();
    let chunks = 5 * super::CHUNK_SIZE * std::mem::size_of::<Option<super::Entry<usize>>>();
    let path = 12 * std::mem::size_of::<usize>();
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: directory + chunks + path + 17,
        ..CassieRuntimeLimits::default()
    };
    let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
    let mut object: ObjectIndex<usize> = ObjectIndex::new(&controls).expect("object");
    for key in [
        "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p",
    ] {
        object
            .insert(Key::new(key, &controls).expect("key"), 1)
            .expect("insert");
    }
    let baseline = controls.current_query_memory_bytes();
    let candidate = Key::new("q", &controls).expect("key");
    // Act
    let result = object.insert(candidate, 2);
    // Assert
    assert!(matches!(result, Err(CassieError::ResourceLimit(_))));
    assert!(!object.is_poisoned());
    assert_eq!(
        object.test_order(),
        ["a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p"]
    );
    assert!(object.test_invariants());
    assert_eq!(controls.current_query_memory_bytes(), baseline);
    drop(object);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_order_decoded_unicode_keys_by_rust_string_order() {
    // Arrange
    let controls = controls();
    let mut object: ObjectIndex<usize> = ObjectIndex::new(&controls).expect("object");
    let keys = ["é", "e\u{301}", "", "e", "🙂", "z"];
    // Act
    for (index, key) in keys.into_iter().enumerate() {
        object
            .insert(Key::new(key, &controls).expect("key"), index)
            .expect("insert");
    }
    let mut expected = keys.to_vec();
    expected.sort_unstable();
    // Assert
    assert_eq!(object.test_order(), expected);
    assert!(object.test_invariants());
    drop(object);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
