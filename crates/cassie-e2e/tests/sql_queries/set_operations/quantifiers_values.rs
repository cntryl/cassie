//! Native PostgreSQL set quantifiers and multiplicity.
use crate::support_sql_fixture::sql_fixture;
use cassie::types::Value;
#[test]
fn should_preserve_values_0_0_given_set_quantifiers_when_rows_repeat() {
    // Arrange
    let fixture = sql_fixture("setspell-values_0_0", &[]);
    let sql =
        "VALUES (1),(1),(2),(NULL::INT) UNION VALUES (1),(3),(NULL::INT) ORDER BY 1 NULLS LAST";
    // Act
    let result = fixture.execute(sql);
    // Assert
    let expected: Vec<Vec<Value>> = vec![
        vec![Value::Int64(1)],
        vec![Value::Int64(2)],
        vec![Value::Int64(3)],
        vec![Value::Null],
    ];
    assert_eq!(
        result.expect("native set query").rows(),
        expected.as_slice()
    );
}
#[test]
fn should_preserve_values_0_1_given_set_quantifiers_when_rows_repeat() {
    // Arrange
    let fixture = sql_fixture("setspell-values_0_1", &[]);
    let sql =
        "VALUES (1),(1),(2),(NULL::INT) UNION ALL VALUES (1),(3),(NULL::INT) ORDER BY 1 NULLS LAST";
    // Act
    let result = fixture.execute(sql);
    // Assert
    let expected: Vec<Vec<Value>> = vec![
        vec![Value::Int64(1)],
        vec![Value::Int64(1)],
        vec![Value::Int64(1)],
        vec![Value::Int64(2)],
        vec![Value::Int64(3)],
        vec![Value::Null],
        vec![Value::Null],
    ];
    assert_eq!(
        result.expect("native set query").rows(),
        expected.as_slice()
    );
}
#[test]
fn should_preserve_values_0_2_given_set_quantifiers_when_rows_repeat() {
    // Arrange
    let fixture = sql_fixture("setspell-values_0_2", &[]);
    let sql = "VALUES (1),(1),(2),(NULL::INT) UNION DISTINCT VALUES (1),(3),(NULL::INT) ORDER BY 1 NULLS LAST";
    // Act
    let result = fixture.execute(sql);
    // Assert
    let expected: Vec<Vec<Value>> = vec![
        vec![Value::Int64(1)],
        vec![Value::Int64(2)],
        vec![Value::Int64(3)],
        vec![Value::Null],
    ];
    assert_eq!(
        result.expect("native set query").rows(),
        expected.as_slice()
    );
}
#[test]
fn should_preserve_values_0_3_given_set_quantifiers_when_rows_repeat() {
    // Arrange
    let fixture = sql_fixture("setspell-values_0_3", &[]);
    let sql =
        "VALUES (1),(1),(2),(NULL::INT) INTERSECT VALUES (1),(3),(NULL::INT) ORDER BY 1 NULLS LAST";
    // Act
    let result = fixture.execute(sql);
    // Assert
    let expected: Vec<Vec<Value>> = vec![vec![Value::Int64(1)], vec![Value::Null]];
    assert_eq!(
        result.expect("native set query").rows(),
        expected.as_slice()
    );
}
#[test]
fn should_preserve_values_0_4_given_set_quantifiers_when_rows_repeat() {
    // Arrange
    let fixture = sql_fixture("setspell-values_0_4", &[]);
    let sql = "VALUES (1),(1),(2),(NULL::INT) INTERSECT ALL VALUES (1),(3),(NULL::INT) ORDER BY 1 NULLS LAST";
    // Act
    let result = fixture.execute(sql);
    // Assert
    let expected: Vec<Vec<Value>> = vec![vec![Value::Int64(1)], vec![Value::Null]];
    assert_eq!(
        result.expect("native set query").rows(),
        expected.as_slice()
    );
}
#[test]
fn should_preserve_values_0_5_given_set_quantifiers_when_rows_repeat() {
    // Arrange
    let fixture = sql_fixture("setspell-values_0_5", &[]);
    let sql = "VALUES (1),(1),(2),(NULL::INT) INTERSECT DISTINCT VALUES (1),(3),(NULL::INT) ORDER BY 1 NULLS LAST";
    // Act
    let result = fixture.execute(sql);
    // Assert
    let expected: Vec<Vec<Value>> = vec![vec![Value::Int64(1)], vec![Value::Null]];
    assert_eq!(
        result.expect("native set query").rows(),
        expected.as_slice()
    );
}
#[test]
fn should_preserve_values_0_6_given_set_quantifiers_when_rows_repeat() {
    // Arrange
    let fixture = sql_fixture("setspell-values_0_6", &[]);
    let sql =
        "VALUES (1),(1),(2),(NULL::INT) EXCEPT VALUES (1),(3),(NULL::INT) ORDER BY 1 NULLS LAST";
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
fn should_preserve_values_0_7_given_set_quantifiers_when_rows_repeat() {
    // Arrange
    let fixture = sql_fixture("setspell-values_0_7", &[]);
    let sql = "VALUES (1),(1),(2),(NULL::INT) EXCEPT ALL VALUES (1),(3),(NULL::INT) ORDER BY 1 NULLS LAST";
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
fn should_preserve_values_0_8_given_set_quantifiers_when_rows_repeat() {
    // Arrange
    let fixture = sql_fixture("setspell-values_0_8", &[]);
    let sql = "VALUES (1),(1),(2),(NULL::INT) EXCEPT DISTINCT VALUES (1),(3),(NULL::INT) ORDER BY 1 NULLS LAST";
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
fn should_preserve_values_1_0_given_set_quantifiers_when_rows_repeat() {
    // Arrange
    let fixture = sql_fixture("setspell-values_1_0", &[]);
    let sql = "VALUES (1),(1),(1),(2) UNION VALUES (1),(1),(3) ORDER BY 1 NULLS LAST";
    // Act
    let result = fixture.execute(sql);
    // Assert
    let expected: Vec<Vec<Value>> = vec![
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
fn should_preserve_values_1_1_given_set_quantifiers_when_rows_repeat() {
    // Arrange
    let fixture = sql_fixture("setspell-values_1_1", &[]);
    let sql = "VALUES (1),(1),(1),(2) UNION ALL VALUES (1),(1),(3) ORDER BY 1 NULLS LAST";
    // Act
    let result = fixture.execute(sql);
    // Assert
    let expected: Vec<Vec<Value>> = vec![
        vec![Value::Int64(1)],
        vec![Value::Int64(1)],
        vec![Value::Int64(1)],
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
fn should_preserve_values_1_2_given_set_quantifiers_when_rows_repeat() {
    // Arrange
    let fixture = sql_fixture("setspell-values_1_2", &[]);
    let sql = "VALUES (1),(1),(1),(2) UNION DISTINCT VALUES (1),(1),(3) ORDER BY 1 NULLS LAST";
    // Act
    let result = fixture.execute(sql);
    // Assert
    let expected: Vec<Vec<Value>> = vec![
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
fn should_preserve_values_1_3_given_set_quantifiers_when_rows_repeat() {
    // Arrange
    let fixture = sql_fixture("setspell-values_1_3", &[]);
    let sql = "VALUES (1),(1),(1),(2) INTERSECT VALUES (1),(1),(3) ORDER BY 1 NULLS LAST";
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
fn should_preserve_values_1_4_given_set_quantifiers_when_rows_repeat() {
    // Arrange
    let fixture = sql_fixture("setspell-values_1_4", &[]);
    let sql = "VALUES (1),(1),(1),(2) INTERSECT ALL VALUES (1),(1),(3) ORDER BY 1 NULLS LAST";
    // Act
    let result = fixture.execute(sql);
    // Assert
    let expected: Vec<Vec<Value>> = vec![vec![Value::Int64(1)], vec![Value::Int64(1)]];
    assert_eq!(
        result.expect("native set query").rows(),
        expected.as_slice()
    );
}
#[test]
fn should_preserve_values_1_5_given_set_quantifiers_when_rows_repeat() {
    // Arrange
    let fixture = sql_fixture("setspell-values_1_5", &[]);
    let sql = "VALUES (1),(1),(1),(2) INTERSECT DISTINCT VALUES (1),(1),(3) ORDER BY 1 NULLS LAST";
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
fn should_preserve_values_1_6_given_set_quantifiers_when_rows_repeat() {
    // Arrange
    let fixture = sql_fixture("setspell-values_1_6", &[]);
    let sql = "VALUES (1),(1),(1),(2) EXCEPT VALUES (1),(1),(3) ORDER BY 1 NULLS LAST";
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
fn should_preserve_values_1_7_given_set_quantifiers_when_rows_repeat() {
    // Arrange
    let fixture = sql_fixture("setspell-values_1_7", &[]);
    let sql = "VALUES (1),(1),(1),(2) EXCEPT ALL VALUES (1),(1),(3) ORDER BY 1 NULLS LAST";
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
fn should_preserve_values_1_8_given_set_quantifiers_when_rows_repeat() {
    // Arrange
    let fixture = sql_fixture("setspell-values_1_8", &[]);
    let sql = "VALUES (1),(1),(1),(2) EXCEPT DISTINCT VALUES (1),(1),(3) ORDER BY 1 NULLS LAST";
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
fn should_preserve_values_2_0_given_set_quantifiers_when_rows_repeat() {
    // Arrange
    let fixture = sql_fixture("setspell-values_2_0", &[]);
    let sql =
        "VALUES (NULL::INT),(NULL::INT),(2) UNION VALUES (NULL::INT),(2),(2) ORDER BY 1 NULLS LAST";
    // Act
    let result = fixture.execute(sql);
    // Assert
    let expected: Vec<Vec<Value>> = vec![vec![Value::Int64(2)], vec![Value::Null]];
    assert_eq!(
        result.expect("native set query").rows(),
        expected.as_slice()
    );
}
#[test]
fn should_preserve_values_2_1_given_set_quantifiers_when_rows_repeat() {
    // Arrange
    let fixture = sql_fixture("setspell-values_2_1", &[]);
    let sql = "VALUES (NULL::INT),(NULL::INT),(2) UNION ALL VALUES (NULL::INT),(2),(2) ORDER BY 1 NULLS LAST";
    // Act
    let result = fixture.execute(sql);
    // Assert
    let expected: Vec<Vec<Value>> = vec![
        vec![Value::Int64(2)],
        vec![Value::Int64(2)],
        vec![Value::Int64(2)],
        vec![Value::Null],
        vec![Value::Null],
        vec![Value::Null],
    ];
    assert_eq!(
        result.expect("native set query").rows(),
        expected.as_slice()
    );
}
#[test]
fn should_preserve_values_2_2_given_set_quantifiers_when_rows_repeat() {
    // Arrange
    let fixture = sql_fixture("setspell-values_2_2", &[]);
    let sql = "VALUES (NULL::INT),(NULL::INT),(2) UNION DISTINCT VALUES (NULL::INT),(2),(2) ORDER BY 1 NULLS LAST";
    // Act
    let result = fixture.execute(sql);
    // Assert
    let expected: Vec<Vec<Value>> = vec![vec![Value::Int64(2)], vec![Value::Null]];
    assert_eq!(
        result.expect("native set query").rows(),
        expected.as_slice()
    );
}
#[test]
fn should_preserve_values_2_3_given_set_quantifiers_when_rows_repeat() {
    // Arrange
    let fixture = sql_fixture("setspell-values_2_3", &[]);
    let sql = "VALUES (NULL::INT),(NULL::INT),(2) INTERSECT VALUES (NULL::INT),(2),(2) ORDER BY 1 NULLS LAST";
    // Act
    let result = fixture.execute(sql);
    // Assert
    let expected: Vec<Vec<Value>> = vec![vec![Value::Int64(2)], vec![Value::Null]];
    assert_eq!(
        result.expect("native set query").rows(),
        expected.as_slice()
    );
}
#[test]
fn should_preserve_values_2_4_given_set_quantifiers_when_rows_repeat() {
    // Arrange
    let fixture = sql_fixture("setspell-values_2_4", &[]);
    let sql = "VALUES (NULL::INT),(NULL::INT),(2) INTERSECT ALL VALUES (NULL::INT),(2),(2) ORDER BY 1 NULLS LAST";
    // Act
    let result = fixture.execute(sql);
    // Assert
    let expected: Vec<Vec<Value>> = vec![vec![Value::Int64(2)], vec![Value::Null]];
    assert_eq!(
        result.expect("native set query").rows(),
        expected.as_slice()
    );
}
#[test]
fn should_preserve_values_2_5_given_set_quantifiers_when_rows_repeat() {
    // Arrange
    let fixture = sql_fixture("setspell-values_2_5", &[]);
    let sql = "VALUES (NULL::INT),(NULL::INT),(2) INTERSECT DISTINCT VALUES (NULL::INT),(2),(2) ORDER BY 1 NULLS LAST";
    // Act
    let result = fixture.execute(sql);
    // Assert
    let expected: Vec<Vec<Value>> = vec![vec![Value::Int64(2)], vec![Value::Null]];
    assert_eq!(
        result.expect("native set query").rows(),
        expected.as_slice()
    );
}
#[test]
fn should_preserve_values_2_6_given_set_quantifiers_when_rows_repeat() {
    // Arrange
    let fixture = sql_fixture("setspell-values_2_6", &[]);
    let sql = "VALUES (NULL::INT),(NULL::INT),(2) EXCEPT VALUES (NULL::INT),(2),(2) ORDER BY 1 NULLS LAST";
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
fn should_preserve_values_2_7_given_set_quantifiers_when_rows_repeat() {
    // Arrange
    let fixture = sql_fixture("setspell-values_2_7", &[]);
    let sql = "VALUES (NULL::INT),(NULL::INT),(2) EXCEPT ALL VALUES (NULL::INT),(2),(2) ORDER BY 1 NULLS LAST";
    // Act
    let result = fixture.execute(sql);
    // Assert
    let expected: Vec<Vec<Value>> = vec![vec![Value::Null]];
    assert_eq!(
        result.expect("native set query").rows(),
        expected.as_slice()
    );
}
#[test]
fn should_preserve_values_2_8_given_set_quantifiers_when_rows_repeat() {
    // Arrange
    let fixture = sql_fixture("setspell-values_2_8", &[]);
    let sql = "VALUES (NULL::INT),(NULL::INT),(2) EXCEPT DISTINCT VALUES (NULL::INT),(2),(2) ORDER BY 1 NULLS LAST";
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
fn should_preserve_values_3_0_given_set_quantifiers_when_rows_repeat() {
    // Arrange
    let fixture = sql_fixture("setspell-values_3_0", &[]);
    let sql = "VALUES (1),(2) UNION VALUES (3),(4) ORDER BY 1 NULLS LAST";
    // Act
    let result = fixture.execute(sql);
    // Assert
    let expected: Vec<Vec<Value>> = vec![
        vec![Value::Int64(1)],
        vec![Value::Int64(2)],
        vec![Value::Int64(3)],
        vec![Value::Int64(4)],
    ];
    assert_eq!(
        result.expect("native set query").rows(),
        expected.as_slice()
    );
}
#[test]
fn should_preserve_values_3_1_given_set_quantifiers_when_rows_repeat() {
    // Arrange
    let fixture = sql_fixture("setspell-values_3_1", &[]);
    let sql = "VALUES (1),(2) UNION ALL VALUES (3),(4) ORDER BY 1 NULLS LAST";
    // Act
    let result = fixture.execute(sql);
    // Assert
    let expected: Vec<Vec<Value>> = vec![
        vec![Value::Int64(1)],
        vec![Value::Int64(2)],
        vec![Value::Int64(3)],
        vec![Value::Int64(4)],
    ];
    assert_eq!(
        result.expect("native set query").rows(),
        expected.as_slice()
    );
}
#[test]
fn should_preserve_values_3_2_given_set_quantifiers_when_rows_repeat() {
    // Arrange
    let fixture = sql_fixture("setspell-values_3_2", &[]);
    let sql = "VALUES (1),(2) UNION DISTINCT VALUES (3),(4) ORDER BY 1 NULLS LAST";
    // Act
    let result = fixture.execute(sql);
    // Assert
    let expected: Vec<Vec<Value>> = vec![
        vec![Value::Int64(1)],
        vec![Value::Int64(2)],
        vec![Value::Int64(3)],
        vec![Value::Int64(4)],
    ];
    assert_eq!(
        result.expect("native set query").rows(),
        expected.as_slice()
    );
}
#[test]
fn should_preserve_values_3_3_given_set_quantifiers_when_rows_repeat() {
    // Arrange
    let fixture = sql_fixture("setspell-values_3_3", &[]);
    let sql = "VALUES (1),(2) INTERSECT VALUES (3),(4) ORDER BY 1 NULLS LAST";
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
fn should_preserve_values_3_4_given_set_quantifiers_when_rows_repeat() {
    // Arrange
    let fixture = sql_fixture("setspell-values_3_4", &[]);
    let sql = "VALUES (1),(2) INTERSECT ALL VALUES (3),(4) ORDER BY 1 NULLS LAST";
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
fn should_preserve_values_3_5_given_set_quantifiers_when_rows_repeat() {
    // Arrange
    let fixture = sql_fixture("setspell-values_3_5", &[]);
    let sql = "VALUES (1),(2) INTERSECT DISTINCT VALUES (3),(4) ORDER BY 1 NULLS LAST";
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
fn should_preserve_values_3_6_given_set_quantifiers_when_rows_repeat() {
    // Arrange
    let fixture = sql_fixture("setspell-values_3_6", &[]);
    let sql = "VALUES (1),(2) EXCEPT VALUES (3),(4) ORDER BY 1 NULLS LAST";
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
fn should_preserve_values_3_7_given_set_quantifiers_when_rows_repeat() {
    // Arrange
    let fixture = sql_fixture("setspell-values_3_7", &[]);
    let sql = "VALUES (1),(2) EXCEPT ALL VALUES (3),(4) ORDER BY 1 NULLS LAST";
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
fn should_preserve_values_3_8_given_set_quantifiers_when_rows_repeat() {
    // Arrange
    let fixture = sql_fixture("setspell-values_3_8", &[]);
    let sql = "VALUES (1),(2) EXCEPT DISTINCT VALUES (3),(4) ORDER BY 1 NULLS LAST";
    // Act
    let result = fixture.execute(sql);
    // Assert
    let expected: Vec<Vec<Value>> = vec![vec![Value::Int64(1)], vec![Value::Int64(2)]];
    assert_eq!(
        result.expect("native set query").rows(),
        expected.as_slice()
    );
}
