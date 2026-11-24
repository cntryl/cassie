//! Native PostgreSQL set quantifiers and multiplicity.
use crate::support_sql_fixture::sql_fixture;
use cassie::types::Value;
#[test]
fn should_preserve_left_association_0_given_set_quantifiers_when_rows_repeat() {
    // Arrange
    let fixture = sql_fixture("setspell-left_association_0", &[]);
    let sql = "VALUES (1),(1),(2) UNION VALUES (1),(3) EXCEPT ALL VALUES (1) ORDER BY 1";
    // Act
    let result = fixture.execute(sql);
    // Assert
    let expected: Vec<Vec<Value>> = vec![vec![Value::Int64(2)], vec![Value::Int64(3)]];
    assert_eq!(
        result.expect("native set query").rows(),
        expected.as_slice()
    );
}
#[test]
fn should_preserve_left_association_1_given_set_quantifiers_when_rows_repeat() {
    // Arrange
    let fixture = sql_fixture("setspell-left_association_1", &[]);
    let sql = "VALUES (1),(1),(2) UNION ALL VALUES (1),(3) EXCEPT ALL VALUES (1) ORDER BY 1";
    // Act
    let result = fixture.execute(sql);
    // Assert
    let expected: Vec<Vec<Value>> = vec![
        vec![Value::Int64(1)],
        vec![Value::Int64(1)],
        vec![Value::Int64(2)],
        vec![Value::Int64(3)],
    ];
    assert_eq!(
        result.expect("native set query").rows(),
        expected.as_slice()
    );
}
#[test]
fn should_preserve_left_association_2_given_set_quantifiers_when_rows_repeat() {
    // Arrange
    let fixture = sql_fixture("setspell-left_association_2", &[]);
    let sql = "VALUES (1),(1),(2) UNION DISTINCT VALUES (1),(3) EXCEPT ALL VALUES (1) ORDER BY 1";
    // Act
    let result = fixture.execute(sql);
    // Assert
    let expected: Vec<Vec<Value>> = vec![vec![Value::Int64(2)], vec![Value::Int64(3)]];
    assert_eq!(
        result.expect("native set query").rows(),
        expected.as_slice()
    );
}
#[test]
fn should_preserve_left_association_3_given_set_quantifiers_when_rows_repeat() {
    // Arrange
    let fixture = sql_fixture("setspell-left_association_3", &[]);
    let sql = "VALUES (1),(1),(2) INTERSECT VALUES (1),(3) EXCEPT ALL VALUES (1) ORDER BY 1";
    // Act
    let result = fixture.execute(sql);
    // Assert
    let expected: Vec<Vec<Value>> = vec![];
    assert_eq!(
        result.expect("native set query").rows(),
        expected.as_slice()
    );
}
#[test]
fn should_preserve_left_association_4_given_set_quantifiers_when_rows_repeat() {
    // Arrange
    let fixture = sql_fixture("setspell-left_association_4", &[]);
    let sql = "VALUES (1),(1),(2) INTERSECT ALL VALUES (1),(3) EXCEPT ALL VALUES (1) ORDER BY 1";
    // Act
    let result = fixture.execute(sql);
    // Assert
    let expected: Vec<Vec<Value>> = vec![];
    assert_eq!(
        result.expect("native set query").rows(),
        expected.as_slice()
    );
}
#[test]
fn should_preserve_left_association_5_given_set_quantifiers_when_rows_repeat() {
    // Arrange
    let fixture = sql_fixture("setspell-left_association_5", &[]);
    let sql =
        "VALUES (1),(1),(2) INTERSECT DISTINCT VALUES (1),(3) EXCEPT ALL VALUES (1) ORDER BY 1";
    // Act
    let result = fixture.execute(sql);
    // Assert
    let expected: Vec<Vec<Value>> = vec![];
    assert_eq!(
        result.expect("native set query").rows(),
        expected.as_slice()
    );
}
#[test]
fn should_preserve_left_association_6_given_set_quantifiers_when_rows_repeat() {
    // Arrange
    let fixture = sql_fixture("setspell-left_association_6", &[]);
    let sql = "VALUES (1),(1),(2) EXCEPT VALUES (1),(3) EXCEPT ALL VALUES (1) ORDER BY 1";
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
fn should_preserve_left_association_7_given_set_quantifiers_when_rows_repeat() {
    // Arrange
    let fixture = sql_fixture("setspell-left_association_7", &[]);
    let sql = "VALUES (1),(1),(2) EXCEPT ALL VALUES (1),(3) EXCEPT ALL VALUES (1) ORDER BY 1";
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
fn should_preserve_left_association_8_given_set_quantifiers_when_rows_repeat() {
    // Arrange
    let fixture = sql_fixture("setspell-left_association_8", &[]);
    let sql = "VALUES (1),(1),(2) EXCEPT DISTINCT VALUES (1),(3) EXCEPT ALL VALUES (1) ORDER BY 1";
    // Act
    let result = fixture.execute(sql);
    // Assert
    let expected: Vec<Vec<Value>> = vec![vec![Value::Int64(2)]];
    assert_eq!(
        result.expect("native set query").rows(),
        expected.as_slice()
    );
}
