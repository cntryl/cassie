//! Ordinary GROUP BY output aliases and ordinals for #804.

use cassie::types::Value;

use super::grouping_fixture;
use crate::support_sql_fixture::sql_fixture;

#[test]
fn should_group_by_output_alias_outside_grouping_sets() {
    // Arrange
    let fixture = grouping_fixture("issue-804-group-by-output-alias");

    // Act
    let result = fixture.execute(
        "SELECT region AS area, SUM(amount) FROM issue_804_group_values \
         GROUP BY area ORDER BY area",
    );

    // Assert
    assert_eq!(
        result.expect("GROUP BY resolves an output alias").rows(),
        &[
            vec![Value::String("east".to_owned()), Value::Int64(5)],
            vec![Value::String("west".to_owned()), Value::Int64(5)],
        ]
    );
}

#[test]
fn should_group_by_output_ordinal_outside_grouping_sets() {
    // Arrange
    let fixture = grouping_fixture("issue-804-group-by-output-ordinal");

    // Act
    let result = fixture.execute(
        "SELECT region AS area, SUM(amount) FROM issue_804_group_values \
         GROUP BY 1 ORDER BY 1",
    );

    // Assert
    assert_eq!(
        result
            .expect("GROUP BY resolves the first output position")
            .rows(),
        &[
            vec![Value::String("east".to_owned()), Value::Int64(5)],
            vec![Value::String("west".to_owned()), Value::Int64(5)],
        ]
    );
}

#[test]
fn should_group_by_an_output_expression_alias() {
    // Arrange
    let fixture = grouping_fixture("issue-804-group-by-expression-alias");

    // Act
    let result = fixture.execute(
        "SELECT UPPER(region) AS area, SUM(amount) FROM issue_804_group_values \
         GROUP BY area ORDER BY area",
    );

    // Assert
    assert_eq!(
        result
            .expect("GROUP BY resolves an output expression alias")
            .rows(),
        &[
            vec![Value::String("EAST".to_owned()), Value::Int64(5)],
            vec![Value::String("WEST".to_owned()), Value::Int64(5)],
        ]
    );
}

#[test]
fn should_prefer_input_column_given_group_by_name_matching_output_alias() {
    // Arrange
    let fixture = sql_fixture(
        "issue-804-group-by-input-name-precedence",
        &[
            "CREATE TABLE issue_804_alias_collision (region TEXT, amount INT)",
            "INSERT INTO issue_804_alias_collision VALUES ('east', 2), ('EAST', 3), ('west', 5)",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT UPPER(region) AS region, SUM(amount) FROM issue_804_alias_collision \
         GROUP BY region ORDER BY 2",
    );

    // Assert
    assert_eq!(
        result
            .expect("GROUP BY prefers the matching input column")
            .rows(),
        &[
            vec![Value::String("EAST".to_owned()), Value::Int64(2)],
            vec![Value::String("EAST".to_owned()), Value::Int64(3)],
            vec![Value::String("WEST".to_owned()), Value::Int64(5)],
        ]
    );
}

#[test]
fn should_keep_integer_expression_as_group_expression_instead_of_ordinal() {
    // Arrange
    let fixture = grouping_fixture("issue-804-group-by-integer-expression");

    // Act
    let result = fixture.execute(
        "SELECT 1 + 0 AS constant, SUM(amount) FROM issue_804_group_values \
         GROUP BY 1 + 0",
    );

    // Assert
    assert_eq!(
        result
            .expect("arithmetic remains a group expression")
            .rows(),
        &[vec![Value::Int64(1), Value::Int64(10)]]
    );
}

#[test]
fn should_keep_unary_plus_integer_as_group_expression_instead_of_ordinal() {
    // Arrange
    let fixture = grouping_fixture("issue-804-group-by-unary-plus-integer");

    // Act
    let direct =
        fixture.execute("SELECT region, SUM(amount) FROM issue_804_group_values GROUP BY +1");
    let parenthesized =
        fixture.execute("SELECT region, SUM(amount) FROM issue_804_group_values GROUP BY (+1)");

    // Assert
    for result in [direct, parenthesized] {
        let error = result.expect_err("unary plus remains a group expression");
        assert!(
            error
                .to_string()
                .contains("must appear in the GROUP BY clause"),
            "the input column is not grouped by an ordinal: {error}"
        );
    }
}

#[test]
fn should_reject_out_of_range_group_by_ordinal_given_select_list() {
    // Arrange
    let fixture = grouping_fixture("issue-804-group-by-ordinal-out-of-range");

    // Act
    let result = fixture.execute("SELECT region, COUNT(*) FROM issue_804_group_values GROUP BY 3");

    // Assert
    let error = result.expect_err("an ordinal outside the select list must fail");
    assert!(
        error
            .to_string()
            .contains("GROUP BY position 3 is not in select list"),
        "the positional error identifies the invalid output position: {error}"
    );
}
