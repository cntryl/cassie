use super::*;

#[test]
fn should_map_aggregate_integer_overflow_to_numeric_value_out_of_range() {
    // Arrange
    let error = CassieError::Execution("aggregate integer overflow".to_string());

    // Act
    let descriptor = error.descriptor();

    // Assert
    assert_eq!(descriptor.sql_state, "22003");
}

#[test]
fn should_map_float_overflow_to_numeric_value_out_of_range() {
    // Arrange
    let error = CassieError::Execution(crate::types::numeric::FLOAT_OVERFLOW.to_string());

    // Act
    let descriptor = error.descriptor();

    // Assert
    assert_eq!(descriptor.sql_state, "22003");
}

#[test]
fn should_map_non_numeric_aggregate_input_to_undefined_function() {
    // Arrange
    let error = CassieError::Execution(crate::types::numeric::non_numeric_aggregate_input(
        "SUM",
        &crate::types::Value::String("a".to_string()),
    ));

    // Act
    let descriptor = error.descriptor();

    // Assert
    assert_eq!(descriptor.sql_state, "42883");
    assert_eq!(descriptor.message, "function sum(text) does not exist");
}

#[test]
fn should_map_invalid_group_by_position_to_invalid_column_reference() {
    // Arrange
    let error = CassieError::Planner("GROUP BY position 3 is not in select list".to_string());

    // Act
    let descriptor = error.descriptor();

    // Assert
    assert_eq!(descriptor.sql_state, "42P10");
}

#[test]
fn should_map_builtin_function_arity_mismatch_to_undefined_function() {
    // Arrange
    let error =
        CassieError::Planner("function 'regexp_like' expects 2 to 3 args, got 1".to_string());

    // Act
    let descriptor = error.descriptor();

    // Assert
    assert_eq!(descriptor.sql_state, "42883");
}

#[test]
fn should_classify_retryable_storage_errors_by_kind() {
    // Arrange
    let errors = [
        cntryl_midge::MidgeError::WriteConflict("overlap".to_string()),
        cntryl_midge::MidgeError::WriteStall("memtable full".to_string()),
        cntryl_midge::MidgeError::Fenced("stale epoch".to_string()),
    ];

    // Act
    let kinds = errors
        .into_iter()
        .map(|error| CassieError::from(error).storage_retry_kind())
        .collect::<Vec<_>>();

    // Assert
    assert_eq!(
        kinds,
        vec![
            Some(StorageRetryKind::WriteConflict),
            Some(StorageRetryKind::WriteStall),
            Some(StorageRetryKind::Fenced),
        ]
    );
    assert_eq!(
        CassieError::Storage("disk".to_string()).storage_retry_kind(),
        None
    );
    assert_eq!(
        CassieError::StorageRetryable("midge write conflict: spoofed".into()).storage_retry_kind(),
        Some(StorageRetryKind::Unavailable)
    );
}

#[test]
fn should_map_transaction_error_kinds_independently_of_message_text() {
    // Arrange
    let kinds = [
        (TransactionErrorKind::InvalidState, "25000", 400),
        (TransactionErrorKind::ActiveTransaction, "25001", 400),
        (TransactionErrorKind::NoActiveTransaction, "25P01", 400),
        (TransactionErrorKind::FailedTransaction, "25P02", 400),
        (TransactionErrorKind::ReadOnlyTransaction, "25006", 400),
        (TransactionErrorKind::InvalidSavepoint, "3B001", 400),
        (TransactionErrorKind::SerializationFailure, "40001", 409),
        (TransactionErrorKind::DeadlockDetected, "40P01", 409),
        (TransactionErrorKind::LockNotAvailable, "55P03", 409),
    ];

    // Act
    let descriptors = kinds.map(|(kind, _, _)| {
        CassieError::Transaction {
            kind,
            message: "division by zero".to_string(),
        }
        .descriptor()
    });

    // Assert
    for ((_, sql_state, http_status), descriptor) in kinds.iter().zip(descriptors) {
        assert_eq!(descriptor.sql_state, *sql_state);
        assert_eq!(descriptor.http_status, *http_status);
        assert_eq!(descriptor.message, "division by zero");
    }
}

#[test]
fn should_map_scalar_integer_overflow_to_numeric_value_out_of_range() {
    // Arrange
    let error = CassieError::Execution("bigint out of range".to_string());

    // Act
    let descriptor = error.descriptor();

    // Assert
    assert_eq!(descriptor.sql_state, "22003");
}
