//! Native PostgreSQL set quantifiers and multiplicity.
use crate::support_sql_fixture::sql_fixture;
use cassie::types::Value;
#[test]
fn should_preserve_precedence_0_given_set_quantifiers_when_rows_repeat() {
    // Arrange
    let fixture = sql_fixture("setspell-precedence_0", &[]);
    let sql = "VALUES (1),(1),(2) UNION VALUES (1),(3) INTERSECT ALL VALUES (1),(1) ORDER BY 1";
    // Act
    let result = fixture.execute(sql);
    // Assert
    let expected: Vec<Vec<Value>> = vec![vec![Value::Int64(1)], vec![Value::Int64(2)]];
    assert_eq!(
        result.expect("native set query").rows(),
        expected.as_slice()
    );
}
#[test]
fn should_preserve_precedence_1_given_set_quantifiers_when_rows_repeat() {
    // Arrange
    let fixture = sql_fixture("setspell-precedence_1", &[]);
    let sql = "VALUES (1),(1),(2) UNION ALL VALUES (1),(3) INTERSECT ALL VALUES (1),(1) ORDER BY 1";
    // Act
    let result = fixture.execute(sql);
    // Assert
    let expected: Vec<Vec<Value>> = vec![
        vec![Value::Int64(1)],
        vec![Value::Int64(1)],
        vec![Value::Int64(1)],
        vec![Value::Int64(2)],
    ];
    assert_eq!(
        result.expect("native set query").rows(),
        expected.as_slice()
    );
}
#[test]
fn should_preserve_precedence_2_given_set_quantifiers_when_rows_repeat() {
    // Arrange
    let fixture = sql_fixture("setspell-precedence_2", &[]);
    let sql =
        "VALUES (1),(1),(2) UNION DISTINCT VALUES (1),(3) INTERSECT ALL VALUES (1),(1) ORDER BY 1";
    // Act
    let result = fixture.execute(sql);
    // Assert
    let expected: Vec<Vec<Value>> = vec![vec![Value::Int64(1)], vec![Value::Int64(2)]];
    assert_eq!(
        result.expect("native set query").rows(),
        expected.as_slice()
    );
}
#[test]
fn should_preserve_precedence_3_given_set_quantifiers_when_rows_repeat() {
    // Arrange
    let fixture = sql_fixture("setspell-precedence_3", &[]);
    let sql = "VALUES (1),(1),(2) INTERSECT VALUES (1),(3) INTERSECT ALL VALUES (1),(1) ORDER BY 1";
    // Act
    let result = fixture.execute(sql);
    // Assert
    let expected: Vec<Vec<Value>> = vec![vec![Value::Int64(1)]];
    assert_eq!(
        result.expect("native set query").rows(),
        expected.as_slice()
    );
}
#[test]
fn should_preserve_precedence_4_given_set_quantifiers_when_rows_repeat() {
    // Arrange
    let fixture = sql_fixture("setspell-precedence_4", &[]);
    let sql =
        "VALUES (1),(1),(2) INTERSECT ALL VALUES (1),(3) INTERSECT ALL VALUES (1),(1) ORDER BY 1";
    // Act
    let result = fixture.execute(sql);
    // Assert
    let expected: Vec<Vec<Value>> = vec![vec![Value::Int64(1)]];
    assert_eq!(
        result.expect("native set query").rows(),
        expected.as_slice()
    );
}
#[test]
fn should_preserve_precedence_5_given_set_quantifiers_when_rows_repeat() {
    // Arrange
    let fixture = sql_fixture("setspell-precedence_5", &[]);
    let sql = "VALUES (1),(1),(2) INTERSECT DISTINCT VALUES (1),(3) INTERSECT ALL VALUES (1),(1) ORDER BY 1";
    // Act
    let result = fixture.execute(sql);
    // Assert
    let expected: Vec<Vec<Value>> = vec![vec![Value::Int64(1)]];
    assert_eq!(
        result.expect("native set query").rows(),
        expected.as_slice()
    );
}
#[test]
fn should_preserve_precedence_6_given_set_quantifiers_when_rows_repeat() {
    // Arrange
    let fixture = sql_fixture("setspell-precedence_6", &[]);
    let sql = "VALUES (1),(1),(2) EXCEPT VALUES (1),(3) INTERSECT ALL VALUES (1),(1) ORDER BY 1";
    // Act
    let result = fixture.execute(sql);
    // Assert
    let expected: Vec<Vec<Value>> = vec![vec![Value::Int64(2)]];
    assert_eq!(
        result.expect("native set query").rows(),
        expected.as_slice()
    );
}
#[test]
fn should_preserve_precedence_7_given_set_quantifiers_when_rows_repeat() {
    // Arrange
    let fixture = sql_fixture("setspell-precedence_7", &[]);
    let sql =
        "VALUES (1),(1),(2) EXCEPT ALL VALUES (1),(3) INTERSECT ALL VALUES (1),(1) ORDER BY 1";
    // Act
    let result = fixture.execute(sql);
    // Assert
    let expected: Vec<Vec<Value>> = vec![vec![Value::Int64(1)], vec![Value::Int64(2)]];
    assert_eq!(
        result.expect("native set query").rows(),
        expected.as_slice()
    );
}
#[test]
fn should_preserve_precedence_8_given_set_quantifiers_when_rows_repeat() {
    // Arrange
    let fixture = sql_fixture("setspell-precedence_8", &[]);
    let sql =
        "VALUES (1),(1),(2) EXCEPT DISTINCT VALUES (1),(3) INTERSECT ALL VALUES (1),(1) ORDER BY 1";
    // Act
    let result = fixture.execute(sql);
    // Assert
    let expected: Vec<Vec<Value>> = vec![vec![Value::Int64(2)]];
    assert_eq!(
        result.expect("native set query").rows(),
        expected.as_slice()
    );
}
