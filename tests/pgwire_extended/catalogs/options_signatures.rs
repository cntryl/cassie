//! Signature errors and pending native array/RECORD identities.
use crate::support_bytea_input_errors as errors;
use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;

#[test]
fn should_reject_zero_given_option_signatures_when_parse_is_attempted() {
    // Arrange
    let sql = "SELECT * FROM pg_options_to_table()";
    // Act
    let batches = errors::parse("options-signature-zero", sql);
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
fn should_reject_extra_given_option_signatures_when_parse_is_attempted() {
    // Arrange
    let sql = "SELECT * FROM pg_options_to_table(NULL,NULL)";
    // Act
    let batches = errors::parse("options-signature-extra", sql);
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
fn should_reject_integer_given_option_signatures_when_parse_is_attempted() {
    // Arrange
    let sql = "SELECT * FROM pg_options_to_table(1)";
    // Act
    let batches = errors::parse("options-signature-integer", sql);
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
fn should_reject_text_given_option_signatures_when_parse_is_attempted() {
    // Arrange
    let sql = "SELECT * FROM pg_options_to_table('x'::TEXT)";
    // Act
    let batches = errors::parse("options-signature-text", sql);
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
fn should_reject_record_given_option_signatures_when_parse_is_attempted() {
    // Arrange
    let sql = "SELECT pg_options_to_table(NULL::TEXT[])";
    // Act
    let batches = errors::parse("options-signature-record", sql);
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

#[test]
fn should_reject_parameter_given_option_signatures_when_parse_is_attempted() {
    // Arrange
    let sql = "SELECT * FROM pg_options_to_table($1)";
    // Act
    let batches = errors::parse("options-signature-parameter", sql);
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

#[test]
fn should_reject_cast_parameter_given_option_signatures_when_parse_is_attempted() {
    // Arrange
    let sql = "SELECT * FROM pg_options_to_table($1::TEXT[])";
    // Act
    let batches = errors::parse("options-signature-cast_parameter", sql);
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

#[test]
fn should_reject_distinct_given_option_table_calls_when_aggregate_modifiers_are_supplied() {
    // Arrange
    let sql = "SELECT * FROM pg_options_to_table(DISTINCT ARRAY['a=b']::TEXT[])";
    // Act
    let batches = errors::parse("options-modifier-distinct", sql);
    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("42809"));
    fixture::assert_success(&batches[1]);
    fixture::assert_success(&batches[2]);
}

#[test]
fn should_reject_order_given_option_table_calls_when_aggregate_modifiers_are_supplied() {
    // Arrange
    let sql = "SELECT * FROM pg_options_to_table(ARRAY['a=b']::TEXT[] ORDER BY 1)";
    // Act
    let batches = errors::parse("options-modifier-order", sql);
    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("42809"));
    fixture::assert_success(&batches[1]);
    fixture::assert_success(&batches[2]);
}
