//! Remaining PostgreSQL 18 query contracts tracked by #803.

use cassie::types::Value;

use crate::support_sql_fixture::sql_fixture;

#[test]
fn should_parse_ident_in_non_strict_mode_before_trailing_text() {
    // Arrange
    let fixture = sql_fixture("issue-803-parse-ident-non-strict", &[]);

    // Act
    let result = fixture.execute("SELECT parse_ident('alpha.beta trailing', false)");

    // Assert
    let result = result.expect("parse_ident non-strict mode accepts trailing text");
    assert_eq!(result.columns()[0].type_oid, 1009);
    assert_eq!(
        result.rows(),
        &[vec![Value::String("{alpha,beta}".to_owned())]]
    );
}
