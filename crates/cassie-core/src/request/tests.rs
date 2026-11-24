use super::{
    Interruption, InterruptionError, MemoryBudgetError, RequestCancellation, RequestMemory,
};
use std::time::Instant;

#[test]
fn should_observe_cancellation_through_cloned_request_handle() {
    // Arrange
    let cancellation = RequestCancellation::new();
    let clone = cancellation.clone();
    let interruption = Interruption::new(None, &clone);

    // Act
    cancellation.cancel();

    // Assert
    assert_eq!(interruption.check(), Err(InterruptionError::Canceled));
}

#[test]
fn should_wake_late_subscriber_after_cancellation() {
    // Arrange
    let cancellation = RequestCancellation::new();
    cancellation.cancel();
    // Act
    let mut subscriber = cancellation.subscribe();

    // Assert
    assert!(cancellation.is_canceled());
    assert!(*subscriber.borrow_and_update());
}

#[test]
fn should_transfer_and_release_memory_lease_without_changing_tracker_identity() {
    // Arrange
    let request = RequestMemory::new(10);
    let first = request.reserve(4).expect("first lease");
    let mut destination = request.reserve(2).expect("destination lease");
    let same_tracker = destination.same_tracker(&first);

    // Act
    destination.absorb(first);
    let transferred = request.current_bytes();
    drop(destination);

    // Assert
    assert_eq!(transferred, 6);
    assert!(same_tracker);
    assert_eq!(request.current_bytes(), 0);
}

#[test]
fn should_share_budget_atomically_between_sibling_reservations_and_track_peak() {
    // Arrange
    let request = RequestMemory::new(7);
    let first = request.reserve(5).expect("five bytes");

    // Act
    let sibling = first.try_sibling(2).expect("remaining budget");
    let overflow = first
        .try_sibling(1)
        .expect_err("budget should reject one extra byte");
    let peak = request.peak_bytes();
    drop(sibling);

    // Assert
    assert_eq!(
        overflow,
        MemoryBudgetError {
            budget: 7,
            requested: 8
        }
    );
    assert_eq!(request.current_bytes(), 5);
    assert_eq!(peak, 7);
}

#[test]
fn should_prioritize_cancellation_over_an_expired_deadline() {
    // Arrange
    let cancellation = RequestCancellation::new();
    cancellation.cancel();
    let interruption = Interruption::new(Some(Instant::now()), &cancellation);

    // Act / Assert
    assert_eq!(interruption.check(), Err(InterruptionError::Canceled));
}

#[test]
fn should_report_an_expired_absolute_deadline() {
    // Arrange
    let cancellation = RequestCancellation::new();
    let interruption = Interruption::new(Some(Instant::now()), &cancellation);

    // Act / Assert
    assert_eq!(
        interruption.check(),
        Err(InterruptionError::DeadlineExceeded)
    );
}
