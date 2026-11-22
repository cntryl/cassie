use super::{parse_expression, parse_statement, SqlErrorKind};

#[test]
fn should_reject_not_chain_before_parser_recursion_exceeds_bound() {
    // Arrange
    let sql = format!("SELECT {}true", "NOT ".repeat(50_000));

    // Act
    let error = parse_statement(&sql).expect_err("deep NOT chain must be rejected");

    // Assert
    assert_eq!(error.kind(), SqlErrorKind::ResourceLimit);
    assert!(error.to_string().contains("NOT nesting exceeds 128"));
}

#[test]
fn should_reject_case_chain_before_parser_recursion_exceeds_bound() {
    // Arrange
    let sql = format!(
        "SELECT {}1{}",
        "CASE WHEN true THEN ".repeat(3_000),
        " END".repeat(3_000)
    );

    // Act
    let error = parse_statement(&sql).expect_err("deep CASE chain must be rejected");

    // Assert
    assert_eq!(error.kind(), SqlErrorKind::ResourceLimit);
    assert!(error.to_string().contains("CASE nesting exceeds 128"));
}

#[test]
#[ignore = "post-freeze focused probe; ARRAY nesting is currently unverified"]
fn should_probe_array_nesting_without_assigning_a_contract() {
    // Arrange
    let sql = format!(
        "SELECT {}1{}",
        "ARRAY[".repeat(256),
        "]".repeat(256)
    );

    // Act
    let result = std::panic::catch_unwind(|| parse_expression(&sql));

    // Assert
    assert!(result.is_ok(), "ARRAY probe must not abort the test process");
}
