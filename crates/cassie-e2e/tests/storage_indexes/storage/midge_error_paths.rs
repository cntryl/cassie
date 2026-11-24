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
fn should_map_write_conflict_to_retryable_storage_error() {
    // Arrange
    let error = MidgeError::WriteConflict("conflict on overlapping writes".to_string());

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

#[test]
fn should_keep_unknown_commit_terminal_given_conflict_text_when_storage_errors_are_mapped() {
    // Arrange
    let error =
        MidgeError::OutcomeUnknown("write conflict while confirming publication".to_owned());
    // Act
    let mapped = CassieError::from(error);
    // Assert
    assert!(matches!(&mapped, CassieError::Storage(_)));
    assert!(mapped.to_string().contains("reconcile"));
}

#[test]
fn should_preserve_unknown_commit_detail_given_lost_acknowledgment_when_storage_errors_are_mapped()
{
    // Arrange
    let error = MidgeError::OutcomeUnknown("sync acknowledgment lost".to_owned());
    // Act
    let mapped = CassieError::from(error);
    // Assert
    assert!(matches!(&mapped, CassieError::Storage(_)));
    assert!(mapped.to_string().contains("sync acknowledgment lost"));
    assert!(mapped.to_string().contains("reconcile"));
}

#[test]
fn should_keep_corruption_terminal_given_conflict_text_when_storage_errors_are_mapped() {
    // Arrange
    let error = MidgeError::Corruption("write conflict in corrupt journal".to_owned());
    // Act
    let mapped = CassieError::from(error);
    // Assert
    assert!(matches!(mapped, CassieError::Storage(_)));
}
