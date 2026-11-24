use crate::sql::parser::{parse_statement, SqlErrorKind};

#[test]
fn should_bound_truth_depth_given_chained_predicates_when_parser_resource_limit_is_reached() {
    // Arrange
    let limit = super::super::depth::MAX_POSTFIX_DEPTH;
    let inputs = [
        format!("SELECT TRUE{}", " IS TRUE".repeat(limit + 1)),
        format!("SELECT TRUE{} IS TRUE", "::BOOLEAN".repeat(limit)),
    ];
    // Act
    let results = inputs.map(|sql| parse_statement(&sql));
    // Assert
    for result in results {
        assert_eq!(
            result
                .expect_err("truth test must share the expression depth bound")
                .kind(),
            SqlErrorKind::ResourceLimit
        );
    }
}
