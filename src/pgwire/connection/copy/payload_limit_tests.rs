use super::{copy_payload_exceeds_limit, MAX_FRONTEND_MESSAGE_BYTES};

#[test]
fn should_accept_copy_payload_just_below_sixteen_mib_limit() {
    // Arrange
    let limit = MAX_FRONTEND_MESSAGE_BYTES;

    // Act
    let exceeded = copy_payload_exceeds_limit(limit - 2, 1, limit);

    // Assert
    assert!(!exceeded);
}

#[test]
fn should_accept_copy_payload_at_sixteen_mib_limit() {
    // Arrange
    let limit = MAX_FRONTEND_MESSAGE_BYTES;

    // Act
    let exceeded = copy_payload_exceeds_limit(limit - 1, 1, limit);

    // Assert
    assert!(!exceeded);
    assert_eq!(limit, 16 * 1024 * 1024);
}

#[test]
fn should_reject_copy_payload_one_byte_over_sixteen_mib_limit() {
    // Arrange
    let limit = MAX_FRONTEND_MESSAGE_BYTES;

    // Act
    let exceeded = copy_payload_exceeds_limit(limit - 1, 2, limit);

    // Assert
    assert!(exceeded);
}

#[test]
fn should_reject_copy_payload_length_overflow() {
    // Arrange
    let limit = MAX_FRONTEND_MESSAGE_BYTES;

    // Act
    let exceeded = copy_payload_exceeds_limit(usize::MAX, 1, limit);

    // Assert
    assert!(exceeded);
}
