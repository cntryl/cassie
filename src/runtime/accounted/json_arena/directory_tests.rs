use std::time::Instant;

use super::{Directory, Progress};
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
fn should_preserve_values_and_route_mutation_after_interrupted_migration() {
    // Arrange
    let controls = controls();
    let mut directory = Directory::new(&controls).expect("directory");
    for number in 0..4 {
        directory.push(value(number, &controls)).expect("push");
    }
    let candidate = value(4, &controls);

    // Act
    let result = directory.push_observed(candidate, |progress| match progress {
        Progress::Moved(2) => Err(CassieError::QueryCancelled),
        _ => Ok(()),
    });

    // Assert
    assert!(matches!(result, Err(CassieError::QueryCancelled)));
    assert_eq!(directory.test_frontier(), Some(2));
    for number in 0..4 {
        assert_eq!(*directory.get(number).expect("live").get(), number);
    }
    let retired = directory.take(3).expect("unmoved item");
    assert_eq!(*retired.get(), 3);
    directory.push(value(4, &controls)).expect("resume");
    assert_eq!(directory.test_frontier(), None);
    assert!(directory.get(3).is_none());
    assert_eq!(*directory.get(4).expect("new").get(), 4);
    drop(retired);
    drop(directory);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_leave_old_directory_unchanged_when_replacement_initialization_is_interrupted() {
    // Arrange
    let controls = controls();
    let mut directory = Directory::new(&controls).expect("directory");
    for number in 0..4 {
        directory.push(value(number, &controls)).expect("push");
    }
    let baseline = controls.current_query_memory_bytes();
    let candidate = value(4, &controls);

    // Act
    let result = directory.push_observed(candidate, |progress| match progress {
        Progress::Initialized(2) => Err(CassieError::QueryCancelled),
        _ => Ok(()),
    });

    // Assert
    assert!(matches!(result, Err(CassieError::QueryCancelled)));
    assert_eq!(directory.test_frontier(), None);
    for number in 0..4 {
        assert_eq!(*directory.get(number).expect("live").get(), number);
    }
    assert_eq!(controls.current_query_memory_bytes(), baseline);
    drop(directory);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_both_directories_and_every_item_after_interrupted_migration() {
    // Arrange
    let controls = controls();
    let mut directory = Directory::new(&controls).expect("directory");
    for number in 0..4 {
        directory.push(value(number, &controls)).expect("push");
    }

    // Act
    let result = directory.push_observed(value(4, &controls), |progress| match progress {
        Progress::Moved(1) => Err(CassieError::QueryCancelled),
        _ => Ok(()),
    });

    // Assert
    assert!(matches!(result, Err(CassieError::QueryCancelled)));
    assert_eq!(directory.test_frontier(), Some(1));
    drop(directory);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_preserve_items_when_directory_growth_is_denied() {
    // Arrange
    let bytes =
        4 * std::mem::size_of::<Option<Accounted<usize>>>() + 5 * std::mem::size_of::<usize>();
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: bytes,
        ..CassieRuntimeLimits::default()
    };
    let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
    let mut directory = Directory::new(&controls).expect("directory");
    for number in 0..4 {
        directory.push(value(number, &controls)).expect("push");
    }
    let baseline = controls.current_query_memory_bytes();

    // Act
    let result = directory.push(value(4, &controls));

    // Assert
    assert!(matches!(result, Err(CassieError::ResourceLimit(_))));
    assert_eq!(directory.test_frontier(), None);
    for number in 0..4 {
        assert_eq!(*directory.get(number).expect("live").get(), number);
    }
    assert_eq!(controls.current_query_memory_bytes(), baseline);
    drop(directory);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_keep_both_sides_owned_after_actual_mid_move_cancellation() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let mut directory = Directory::new(&controls).expect("directory");
    for number in 0..4 {
        directory.push(value(number, &controls)).expect("push");
    }

    // Act
    let result = directory.push_observed(value(4, &controls), |progress| {
        if matches!(progress, Progress::Moved(2)) {
            cancellation.cancel();
        }
        Ok(())
    });

    // Assert
    assert!(matches!(result, Err(CassieError::QueryCancelled)));
    assert_eq!(directory.test_frontier(), Some(2));
    for number in 0..4 {
        assert_eq!(*directory.get(number).expect("live").get(), number);
    }
    drop(directory);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_route_mutation_to_either_side_of_the_frontier() {
    // Arrange
    let controls = controls();
    let mut directory = Directory::new(&controls).expect("directory");
    for number in 0..4 {
        directory.push(number).expect("push");
    }
    let result = directory.push_observed(4, |progress| match progress {
        Progress::Moved(2) => Err(CassieError::QueryCancelled),
        _ => Ok(()),
    });
    assert!(matches!(result, Err(CassieError::QueryCancelled)));

    // Act
    *directory.get_mut(0).expect("moved") = 10;
    *directory.get_mut(3).expect("unmoved") = 30;
    directory.push(4).expect("resume");

    // Assert
    assert_eq!(directory.get(0), Some(&10));
    assert_eq!(directory.get(3), Some(&30));
    assert_eq!(directory.get(4), Some(&4));
    drop(directory);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_resume_repeatedly_without_duplicating_or_losing_owners() {
    // Arrange
    let controls = controls();
    let mut directory = Directory::new(&controls).expect("directory");
    for number in 0..4 {
        directory.push(value(number, &controls)).expect("push");
    }

    // Act / Assert
    for boundary in 1..=3 {
        let result = directory.push_observed(value(4, &controls), |progress| match progress {
            Progress::Moved(moved) if moved == boundary => Err(CassieError::QueryCancelled),
            _ => Ok(()),
        });
        assert!(matches!(result, Err(CassieError::QueryCancelled)));
        assert_eq!(directory.test_frontier(), Some(boundary));
        for number in 0..4 {
            assert_eq!(*directory.get(number).expect("live").get(), number);
        }
    }
    directory.push(value(4, &controls)).expect("resume");
    assert_eq!(directory.test_frontier(), None);
    assert_eq!(*directory.get(4).expect("new").get(), 4);
    drop(directory);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_reuse_empty_positions_on_both_sides_without_replacing_live_values() {
    // Arrange
    let controls = controls();
    let mut directory = Directory::new(&controls).expect("directory");
    for number in 0..4 {
        directory.push(number).expect("push");
    }
    let result = directory.push_observed(4, |progress| match progress {
        Progress::Moved(2) => Err(CassieError::QueryCancelled),
        _ => Ok(()),
    });
    assert!(matches!(result, Err(CassieError::QueryCancelled)));
    assert_eq!(directory.take(0), Some(0));
    assert_eq!(directory.take(3), Some(3));

    // Act
    directory.put_empty(0, 10).expect("moved slot");
    directory.put_empty(3, 30).expect("unmoved slot");

    // Assert
    assert_eq!(directory.put_empty(0, 99), Err(99));
    assert_eq!(directory.put_empty(99, 100), Err(100));
    directory.push(4).expect("resume");
    assert_eq!(directory.get(0), Some(&10));
    assert_eq!(directory.get(3), Some(&30));
    assert_eq!(directory.get(4), Some(&4));
    drop(directory);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
