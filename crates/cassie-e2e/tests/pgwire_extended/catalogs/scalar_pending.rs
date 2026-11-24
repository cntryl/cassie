//! Explicit staged rejection rather than a fabricated NAME parameter ABI.
use crate::support_bytea_input_errors as errors;
use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;

#[test]
fn should_reject_name_inference_given_an_untyped_encoding_parameter_when_parse_is_attempted() {
    // Arrange
    let sql = "SELECT pg_char_to_encoding($1)";
    // Act
    let batches = errors::parse("catalog-name-pending", sql);
    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("0A000"));
    assert_eq!(
        batches[0].iter().map(|frame| frame.0).collect::<Vec<_>>(),
        vec![b'E']
    );
    fixture::assert_success(&batches[1]);
    fixture::assert_success(&batches[2]);
    assert_eq!(wire::data_rows(&batches[2]), vec![vec![Some("7".into())]]);
}
