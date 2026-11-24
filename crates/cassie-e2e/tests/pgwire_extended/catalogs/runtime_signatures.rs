//! Native signature errors and explicit pending record-valued projection.
use crate::support_bytea_input_errors as errors;
use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;

#[test]
fn should_reject_recovery_arg_given_invalid_runtime_signature_when_parse_is_attempted() {
    // Arrange
    let sql = "SELECT pg_is_in_recovery(1)";
    // Act
    let batches = errors::parse("runtime-recovery_arg", sql);
    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("42883"));
    assert_eq!(
        batches[0].iter().map(|frame| frame.0).collect::<Vec<_>>(),
        vec![b'E']
    );
    fixture::assert_success(&batches[1]);
    fixture::assert_success(&batches[2]);
    assert_eq!(wire::data_rows(&batches[2]), vec![vec![Some("7".into())]]);
}

#[test]
fn should_reject_jit_arg_given_invalid_runtime_signature_when_parse_is_attempted() {
    // Arrange
    let sql = "SELECT pg_jit_available(1)";
    // Act
    let batches = errors::parse("runtime-jit_arg", sql);
    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("42883"));
    assert_eq!(
        batches[0].iter().map(|frame| frame.0).collect::<Vec<_>>(),
        vec![b'E']
    );
    fixture::assert_success(&batches[1]);
    fixture::assert_success(&batches[2]);
    assert_eq!(wire::data_rows(&batches[2]), vec![vec![Some("7".into())]]);
}

#[test]
fn should_reject_numa_arg_given_invalid_runtime_signature_when_parse_is_attempted() {
    // Arrange
    let sql = "SELECT pg_numa_available(NULL)";
    // Act
    let batches = errors::parse("runtime-numa_arg", sql);
    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("42883"));
    assert_eq!(
        batches[0].iter().map(|frame| frame.0).collect::<Vec<_>>(),
        vec![b'E']
    );
    fixture::assert_success(&batches[1]);
    fixture::assert_success(&batches[2]);
    assert_eq!(wire::data_rows(&batches[2]), vec![vec![Some("7".into())]]);
}

#[test]
fn should_reject_unicode_arg_given_invalid_runtime_signature_when_parse_is_attempted() {
    // Arrange
    let sql = "SELECT unicode_version(1)";
    // Act
    let batches = errors::parse("runtime-unicode_arg", sql);
    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("42883"));
    assert_eq!(
        batches[0].iter().map(|frame| frame.0).collect::<Vec<_>>(),
        vec![b'E']
    );
    fixture::assert_success(&batches[1]);
    fixture::assert_success(&batches[2]);
    assert_eq!(wire::data_rows(&batches[2]), vec![vec![Some("7".into())]]);
}

#[test]
fn should_reject_compression_empty_given_invalid_runtime_signature_when_parse_is_attempted() {
    // Arrange
    let sql = "SELECT pg_column_compression()";
    // Act
    let batches = errors::parse("runtime-compression_empty", sql);
    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("42883"));
    assert_eq!(
        batches[0].iter().map(|frame| frame.0).collect::<Vec<_>>(),
        vec![b'E']
    );
    fixture::assert_success(&batches[1]);
    fixture::assert_success(&batches[2]);
    assert_eq!(wire::data_rows(&batches[2]), vec![vec![Some("7".into())]]);
}

#[test]
fn should_reject_compression_extra_given_invalid_runtime_signature_when_parse_is_attempted() {
    // Arrange
    let sql = "SELECT pg_column_compression(1,2)";
    // Act
    let batches = errors::parse("runtime-compression_extra", sql);
    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("42883"));
    assert_eq!(
        batches[0].iter().map(|frame| frame.0).collect::<Vec<_>>(),
        vec![b'E']
    );
    fixture::assert_success(&batches[1]);
    fixture::assert_success(&batches[2]);
    assert_eq!(wire::data_rows(&batches[2]), vec![vec![Some("7".into())]]);
}

#[test]
fn should_reject_compression_unknown_given_invalid_runtime_signature_when_parse_is_attempted() {
    // Arrange
    let sql = "SELECT pg_column_compression($1)";
    // Act
    let batches = errors::parse("runtime-compression_unknown", sql);
    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("42P18"));
    assert_eq!(
        batches[0].iter().map(|frame| frame.0).collect::<Vec<_>>(),
        vec![b'E']
    );
    fixture::assert_success(&batches[1]);
    fixture::assert_success(&batches[2]);
    assert_eq!(wire::data_rows(&batches[2]), vec![vec![Some("7".into())]]);
}

#[test]
fn should_reject_modules_arg_given_invalid_runtime_signature_when_parse_is_attempted() {
    // Arrange
    let sql = "SELECT * FROM pg_get_loaded_modules(1)";
    // Act
    let batches = errors::parse("runtime-modules_arg", sql);
    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("42883"));
    assert_eq!(
        batches[0].iter().map(|frame| frame.0).collect::<Vec<_>>(),
        vec![b'E']
    );
    fixture::assert_success(&batches[1]);
    fixture::assert_success(&batches[2]);
    assert_eq!(wire::data_rows(&batches[2]), vec![vec![Some("7".into())]]);
}

#[test]
fn should_reject_record_projection_given_pending_record_abi_when_loaded_modules_is_selected_as_a_scalar(
) {
    // Arrange
    let sql = "SELECT pg_get_loaded_modules()";
    // Act
    let batches = errors::parse("runtime-record-pending", sql);
    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("0A000"));
    assert_eq!(
        batches[0].iter().map(|frame| frame.0).collect::<Vec<_>>(),
        vec![b'E']
    );
    fixture::assert_success(&batches[1]);
    fixture::assert_success(&batches[2]);
}
