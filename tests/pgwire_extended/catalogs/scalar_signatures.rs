//! Native Parse-time signature rejection, followed by Sync recovery.
use crate::support_bytea_input_errors as errors;
use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;

#[test]
fn should_reject_pretty_unknown_given_native_signature_resolution_when_parse_is_attempted() {
    // Arrange
    let sql = "SELECT pg_size_pretty('1024')";
    // Act
    let batches = errors::parse("catalog-parse-pretty_unknown", sql);
    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("42725"));
    assert_eq!(
        batches[0].iter().map(|frame| frame.0).collect::<Vec<_>>(),
        vec![b'E']
    );
    fixture::assert_success(&batches[1]);
    fixture::assert_success(&batches[2]);
    assert_eq!(wire::data_rows(&batches[2]), vec![vec![Some("7".into())]]);
}

#[test]
fn should_reject_pretty_untyped_null_given_native_signature_resolution_when_parse_is_attempted() {
    // Arrange
    let sql = "SELECT pg_size_pretty(NULL)";
    // Act
    let batches = errors::parse("catalog-parse-pretty_untyped_null", sql);
    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("42725"));
    assert_eq!(
        batches[0].iter().map(|frame| frame.0).collect::<Vec<_>>(),
        vec![b'E']
    );
    fixture::assert_success(&batches[1]);
    fixture::assert_success(&batches[2]);
    assert_eq!(wire::data_rows(&batches[2]), vec![vec![Some("7".into())]]);
}

#[test]
fn should_reject_pretty_untyped_parameter_given_native_signature_resolution_when_parse_is_attempted(
) {
    // Arrange
    let sql = "SELECT pg_size_pretty($1)";
    // Act
    let batches = errors::parse("catalog-parse-pretty_untyped_parameter", sql);
    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("42725"));
    assert_eq!(
        batches[0].iter().map(|frame| frame.0).collect::<Vec<_>>(),
        vec![b'E']
    );
    fixture::assert_success(&batches[1]);
    fixture::assert_success(&batches[2]);
    assert_eq!(wire::data_rows(&batches[2]), vec![vec![Some("7".into())]]);
}

#[test]
fn should_reject_bytes_integer_given_native_signature_resolution_when_parse_is_attempted() {
    // Arrange
    let sql = "SELECT pg_size_bytes(1)";
    // Act
    let batches = errors::parse("catalog-parse-bytes_integer", sql);
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
fn should_reject_encoding_integer_given_native_signature_resolution_when_parse_is_attempted() {
    // Arrange
    let sql = "SELECT pg_char_to_encoding(1)";
    // Act
    let batches = errors::parse("catalog-parse-encoding_integer", sql);
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
fn should_reject_pretty_text_given_native_signature_resolution_when_parse_is_attempted() {
    // Arrange
    let sql = "SELECT pg_size_pretty('1'::text)";
    // Act
    let batches = errors::parse("catalog-parse-pretty_text", sql);
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
fn should_reject_pretty_float_given_native_signature_resolution_when_parse_is_attempted() {
    // Arrange
    let sql = "SELECT pg_size_pretty(1::float8)";
    // Act
    let batches = errors::parse("catalog-parse-pretty_float", sql);
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
fn should_reject_bytes_no_args_given_native_signature_resolution_when_parse_is_attempted() {
    // Arrange
    let sql = "SELECT pg_size_bytes()";
    // Act
    let batches = errors::parse("catalog-parse-bytes_no_args", sql);
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
fn should_reject_encoding_two_args_given_native_signature_resolution_when_parse_is_attempted() {
    // Arrange
    let sql = "SELECT pg_char_to_encoding('UTF8','UTF8')";
    // Act
    let batches = errors::parse("catalog-parse-encoding_two_args", sql);
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
