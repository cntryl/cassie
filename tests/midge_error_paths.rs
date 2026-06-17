use cassie::app::CassieError;
use cntryl_midge::MidgeError;

#[test]
fn should_map_write_stall_to_retryable_storage_error() {
    // Arrange
    let error = MidgeError::WriteStall("temporary write stall".to_string());

    // Act
    let mapped = CassieError::from(error);

    // Assert
    assert!(matches!(mapped, CassieError::StorageRetryable(_)));
}

#[test]
fn should_map_fenced_write_to_retryable_storage_error() {
    // Arrange
    let error = MidgeError::Fenced("writer fenced".to_string());

    // Act
    let mapped = CassieError::from(error);

    // Assert
    assert!(matches!(mapped, CassieError::StorageRetryable(_)));
}

#[test]
fn should_map_invalid_argument_family_error_to_missing_family() {
    // Arrange
    let error = MidgeError::InvalidArgument("column family 999 does not exist".to_string());

    // Act
    let mapped = CassieError::from(error);

    // Assert
    assert!(matches!(mapped, CassieError::StorageMissingFamily(_)));
}
