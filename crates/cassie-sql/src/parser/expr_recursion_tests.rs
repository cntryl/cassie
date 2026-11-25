use crate::parser::{parse_statement, SqlErrorKind};

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
fn should_accept_exact_not_depth_boundary() {
    // Arrange
    let sql = format!("SELECT {}true", "NOT ".repeat(128));

    // Act
    let result = parse_statement(&sql);

    // Assert
    assert!(result.is_ok(), "128 NOT tokens are within the contract");
}

#[test]
fn should_reject_not_chain_at_one_beyond_depth_boundary() {
    // Arrange
    let sql = format!("SELECT {}true", "NOT ".repeat(129));

    // Act
    let error = parse_statement(&sql).expect_err("129 NOT tokens exceed the contract");

    // Assert
    assert_eq!(error.kind(), SqlErrorKind::ResourceLimit);
    assert!(error.to_string().contains("NOT nesting exceeds 128"));
}

#[test]
fn should_accept_boolean_chains_at_operand_boundary() {
    // Arrange
    let or_chain = vec!["true"; 128].join(" OR ");
    let and_chain = vec!["true"; 128].join(" AND ");

    // Act
    let or_result = parse_statement(&format!("SELECT {or_chain}"));
    let and_result = parse_statement(&format!("SELECT {and_chain}"));

    // Assert
    assert!(or_result.is_ok(), "128 OR operands are within the contract");
    assert!(
        and_result.is_ok(),
        "128 AND operands are within the contract"
    );
}

#[test]
fn should_reject_boolean_chains_one_operand_over_boundary() {
    // Arrange
    let or_chain = vec!["true"; 129].join(" OR ");
    let and_chain = vec!["true"; 129].join(" AND ");

    // Act
    let or_error = parse_statement(&format!("SELECT {or_chain}"))
        .expect_err("129 OR operands exceed the contract");
    let and_error = parse_statement(&format!("SELECT {and_chain}"))
        .expect_err("129 AND operands exceed the contract");

    // Assert
    assert_eq!(or_error.kind(), SqlErrorKind::ResourceLimit);
    assert_eq!(and_error.kind(), SqlErrorKind::ResourceLimit);
}

#[test]
fn should_count_mixed_logical_operators_through_grouped_expressions() {
    // Arrange
    let grouped_path = |outer_operands: usize, inner_operands: usize| {
        format!(
            "SELECT {} OR ({})",
            vec!["true"; outer_operands - 1].join(" OR "),
            vec!["true"; inner_operands].join(" AND ")
        )
    };
    let at_limit = grouped_path(65, 64);
    let over_limit = grouped_path(66, 64);
    let left_sibling = vec!["true"; 127].join(" AND ");
    let right_sibling = vec!["true"; 127].join(" AND ");
    let independent_siblings = format!("SELECT ({left_sibling}) OR ({right_sibling})");

    // Act
    let accepted = parse_statement(&at_limit);
    let rejected = parse_statement(&over_limit);
    let siblings = parse_statement(&independent_siblings);

    // Assert
    assert!(
        accepted.is_ok(),
        "127 combined logical nodes are within the limit"
    );
    assert_eq!(
        rejected
            .expect_err("128 combined logical nodes exceed the limit")
            .kind(),
        SqlErrorKind::ResourceLimit
    );
    assert!(
        siblings.is_ok(),
        "each sibling subtree gets an independent depth budget"
    );
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
fn should_accept_exact_case_depth_boundary() {
    // Arrange
    let sql = format!(
        "SELECT {}1{}",
        "CASE WHEN true THEN ".repeat(128),
        " END".repeat(128)
    );

    // Act
    let result = parse_statement(&sql);

    // Assert
    assert!(result.is_ok(), "128 CASE levels are within the contract");
}

#[test]
fn should_reject_case_chain_at_one_beyond_depth_boundary() {
    // Arrange
    let sql = format!(
        "SELECT {}1{}",
        "CASE WHEN true THEN ".repeat(129),
        " END".repeat(129)
    );

    // Act
    let error = parse_statement(&sql).expect_err("129 CASE levels exceed the contract");

    // Assert
    assert_eq!(error.kind(), SqlErrorKind::ResourceLimit);
    assert!(error.to_string().contains("CASE nesting exceeds 128"));
}

#[test]
fn should_parse_array_at_exact_nesting_boundary() {
    // Arrange
    let sql = format!("SELECT {}1{}", "ARRAY[".repeat(128), "]".repeat(128));

    // Act
    let result = parse_statement(&sql);

    // Assert
    assert!(result.is_ok(), "128 ARRAY levels are within the contract");
}
