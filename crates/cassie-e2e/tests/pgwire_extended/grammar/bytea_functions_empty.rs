//! Known input families are checked even when a scan has no rows.
use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;

#[test]
fn should_validate_empty_case_0_given_fixed_bytea_function_when_no_rows_are_scanned() {
    // Arrange
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(
            "CREATE TABLE bytea_empty (bytes BYTEA,t TEXT,fmt TEXT,n INT,b BOOL,big BIGINT)",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT encode(t,fmt) AS v FROM bytea_empty",
        )]),
    ];
    // Act
    let batches = run_script("bytea-empty-0", steps);
    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(wire::error_code(&batches[1]).as_deref(), Some("42883"));
    let error = &batches[1].iter().find(|frame| frame.0 == b'E').unwrap().1;
    assert_eq!(
        wire::parse_error_fields(error)
            .iter()
            .find(|field| field.0 == 'M')
            .map(|field| field.1.as_str()),
        Some("function encode(text, text) does not exist")
    );
}

#[test]
fn should_validate_empty_case_1_given_fixed_bytea_function_when_no_rows_are_scanned() {
    // Arrange
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(
            "CREATE TABLE bytea_empty (bytes BYTEA,t TEXT,fmt TEXT,n INT,b BOOL,big BIGINT)",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT decode(bytes,fmt) AS v FROM bytea_empty",
        )]),
    ];
    // Act
    let batches = run_script("bytea-empty-1", steps);
    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(wire::error_code(&batches[1]).as_deref(), Some("42883"));
    let error = &batches[1].iter().find(|frame| frame.0 == b'E').unwrap().1;
    assert_eq!(
        wire::parse_error_fields(error)
            .iter()
            .find(|field| field.0 == 'M')
            .map(|field| field.1.as_str()),
        Some("function decode(bytea, text) does not exist")
    );
}

#[test]
fn should_validate_empty_case_2_given_fixed_bytea_function_when_no_rows_are_scanned() {
    // Arrange
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(
            "CREATE TABLE bytea_empty (bytes BYTEA,t TEXT,fmt TEXT,n INT,b BOOL,big BIGINT)",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT get_byte(bytes,big) AS v FROM bytea_empty",
        )]),
    ];
    // Act
    let batches = run_script("bytea-empty-2", steps);
    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(wire::error_code(&batches[1]).as_deref(), Some("42883"));
    let error = &batches[1].iter().find(|frame| frame.0 == b'E').unwrap().1;
    assert_eq!(
        wire::parse_error_fields(error)
            .iter()
            .find(|field| field.0 == 'M')
            .map(|field| field.1.as_str()),
        Some("function get_byte(bytea, bigint) does not exist")
    );
}

#[test]
fn should_validate_empty_case_3_given_fixed_bytea_function_when_no_rows_are_scanned() {
    // Arrange
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(
            "CREATE TABLE bytea_empty (bytes BYTEA,t TEXT,fmt TEXT,n INT,b BOOL,big BIGINT)",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT get_bit(t,big) AS v FROM bytea_empty",
        )]),
    ];
    // Act
    let batches = run_script("bytea-empty-3", steps);
    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(wire::error_code(&batches[1]).as_deref(), Some("42883"));
    let error = &batches[1].iter().find(|frame| frame.0 == b'E').unwrap().1;
    assert_eq!(
        wire::parse_error_fields(error)
            .iter()
            .find(|field| field.0 == 'M')
            .map(|field| field.1.as_str()),
        Some("function get_bit(text, bigint) does not exist")
    );
}

#[test]
fn should_validate_empty_case_4_given_fixed_bytea_function_when_no_rows_are_scanned() {
    // Arrange
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(
            "CREATE TABLE bytea_empty (bytes BYTEA,t TEXT,fmt TEXT,n INT,b BOOL,big BIGINT)",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT sha256(t) AS v FROM bytea_empty",
        )]),
    ];
    // Act
    let batches = run_script("bytea-empty-4", steps);
    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(wire::error_code(&batches[1]).as_deref(), Some("42883"));
    let error = &batches[1].iter().find(|frame| frame.0 == b'E').unwrap().1;
    assert_eq!(
        wire::parse_error_fields(error)
            .iter()
            .find(|field| field.0 == 'M')
            .map(|field| field.1.as_str()),
        Some("function sha256(text) does not exist")
    );
}

#[test]
fn should_validate_empty_case_5_given_fixed_bytea_function_when_no_rows_are_scanned() {
    // Arrange
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(
            "CREATE TABLE bytea_empty (bytes BYTEA,t TEXT,fmt TEXT,n INT,b BOOL,big BIGINT)",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT lower(decode(t,fmt)) AS v FROM bytea_empty",
        )]),
    ];
    // Act
    let batches = run_script("bytea-empty-5", steps);
    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(wire::error_code(&batches[1]).as_deref(), Some("42883"));
    let error = &batches[1].iter().find(|frame| frame.0 == b'E').unwrap().1;
    assert_eq!(
        wire::parse_error_fields(error)
            .iter()
            .find(|field| field.0 == 'M')
            .map(|field| field.1.as_str()),
        Some("function lower(bytea) does not exist")
    );
}

#[test]
fn should_validate_empty_case_6_given_fixed_bytea_function_when_no_rows_are_scanned() {
    // Arrange
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(
            "CREATE TABLE bytea_empty (bytes BYTEA,t TEXT,fmt TEXT,n INT,b BOOL,big BIGINT)",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT encode(bytes,n) AS v FROM bytea_empty",
        )]),
    ];
    // Act
    let batches = run_script("bytea-empty-6", steps);
    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(wire::error_code(&batches[1]).as_deref(), Some("42883"));
    let error = &batches[1].iter().find(|frame| frame.0 == b'E').unwrap().1;
    assert_eq!(
        wire::parse_error_fields(error)
            .iter()
            .find(|field| field.0 == 'M')
            .map(|field| field.1.as_str()),
        Some("function encode(bytea, integer) does not exist")
    );
}

#[test]
fn should_validate_empty_case_7_given_fixed_bytea_function_when_no_rows_are_scanned() {
    // Arrange
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(
            "CREATE TABLE bytea_empty (bytes BYTEA,t TEXT,fmt TEXT,n INT,b BOOL,big BIGINT)",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT get_byte(bytes,n) AS v FROM bytea_empty",
        )]),
    ];
    // Act
    let batches = run_script("bytea-empty-7", steps);
    // Assert
    fixture::assert_success(&batches[0]);
    fixture::assert_success(&batches[1]);
    assert_eq!(
        wire::data_rows(&batches[1]),
        Vec::<Vec<Option<String>>>::new()
    );
    assert!(batches[1].iter().any(|frame| frame.0 == b'T'));
}

#[test]
fn should_validate_empty_case_8_given_fixed_bytea_function_when_no_rows_are_scanned() {
    // Arrange
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(
            "CREATE TABLE bytea_empty (bytes BYTEA,t TEXT,fmt TEXT,n INT,b BOOL,big BIGINT)",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT get_bit(bytes,big) AS v FROM bytea_empty",
        )]),
    ];
    // Act
    let batches = run_script("bytea-empty-8", steps);
    // Assert
    fixture::assert_success(&batches[0]);
    fixture::assert_success(&batches[1]);
    assert_eq!(
        wire::data_rows(&batches[1]),
        Vec::<Vec<Option<String>>>::new()
    );
    assert!(batches[1].iter().any(|frame| frame.0 == b'T'));
}

#[test]
fn should_validate_empty_case_9_given_fixed_bytea_function_when_no_rows_are_scanned() {
    // Arrange
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(
            "CREATE TABLE bytea_empty (bytes BYTEA,t TEXT,fmt TEXT,n INT,b BOOL,big BIGINT)",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT lower(encode(bytes,fmt)) AS v FROM bytea_empty",
        )]),
    ];
    // Act
    let batches = run_script("bytea-empty-9", steps);
    // Assert
    fixture::assert_success(&batches[0]);
    fixture::assert_success(&batches[1]);
    assert_eq!(
        wire::data_rows(&batches[1]),
        Vec::<Vec<Option<String>>>::new()
    );
    assert!(batches[1].iter().any(|frame| frame.0 == b'T'));
}

#[test]
fn should_validate_empty_case_10_given_fixed_bytea_function_when_no_rows_are_scanned() {
    // Arrange
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(
            "CREATE TABLE bytea_empty (bytes BYTEA,t TEXT,fmt TEXT,n INT,b BOOL,big BIGINT)",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT decode(t,fmt) AS v FROM bytea_empty",
        )]),
    ];
    // Act
    let batches = run_script("bytea-empty-10", steps);
    // Assert
    fixture::assert_success(&batches[0]);
    fixture::assert_success(&batches[1]);
    assert_eq!(
        wire::data_rows(&batches[1]),
        Vec::<Vec<Option<String>>>::new()
    );
    assert!(batches[1].iter().any(|frame| frame.0 == b'T'));
}

#[test]
fn should_validate_empty_case_11_given_fixed_bytea_function_when_no_rows_are_scanned() {
    // Arrange
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(
            "CREATE TABLE bytea_empty (bytes BYTEA,t TEXT,fmt TEXT,n INT,b BOOL,big BIGINT)",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT encode(bytes,'bad') AS v FROM bytea_empty",
        )]),
    ];
    // Act
    let batches = run_script("bytea-empty-11", steps);
    // Assert
    fixture::assert_success(&batches[0]);
    fixture::assert_success(&batches[1]);
    assert_eq!(
        wire::data_rows(&batches[1]),
        Vec::<Vec<Option<String>>>::new()
    );
    assert!(batches[1].iter().any(|frame| frame.0 == b'T'));
}

#[test]
fn should_validate_empty_case_12_given_fixed_bytea_function_when_no_rows_are_scanned() {
    // Arrange
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(
            "CREATE TABLE bytea_empty (bytes BYTEA,t TEXT,fmt TEXT,n INT,b BOOL,big BIGINT)",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT get_byte(bytes,-1) AS v FROM bytea_empty",
        )]),
    ];
    // Act
    let batches = run_script("bytea-empty-12", steps);
    // Assert
    fixture::assert_success(&batches[0]);
    fixture::assert_success(&batches[1]);
    assert_eq!(
        wire::data_rows(&batches[1]),
        Vec::<Vec<Option<String>>>::new()
    );
    assert!(batches[1].iter().any(|frame| frame.0 == b'T'));
}
