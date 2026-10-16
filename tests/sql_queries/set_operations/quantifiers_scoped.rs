//! Native PostgreSQL set quantifiers and multiplicity.
use crate::support_sql_fixture::sql_fixture;
use cassie::types::Value;
#[test]
fn should_preserve_scoped_0_0_given_set_quantifiers_when_rows_repeat() {
    // Arrange
    let fixture = sql_fixture("setspell-scoped_0_0", &[]);
    let sql = "(SELECT ALL n FROM (VALUES (1),(1),(2),(NULL::INT)) AS l(n) ORDER BY n NULLS LAST LIMIT 3) UNION (SELECT ALL n FROM (VALUES (1),(3),(NULL::INT)) AS r(n) ORDER BY n NULLS LAST LIMIT 2) ORDER BY 1 NULLS LAST";
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
fn should_preserve_scoped_0_1_given_set_quantifiers_when_rows_repeat() {
    // Arrange
    let fixture = sql_fixture("setspell-scoped_0_1", &[]);
    let sql = "(SELECT ALL n FROM (VALUES (1),(1),(2),(NULL::INT)) AS l(n) ORDER BY n NULLS LAST LIMIT 3) UNION ALL (SELECT ALL n FROM (VALUES (1),(3),(NULL::INT)) AS r(n) ORDER BY n NULLS LAST LIMIT 2) ORDER BY 1 NULLS LAST";
    // Act
    let result = fixture.execute(sql);
    // Assert
    let expected: Vec<Vec<Value>> = vec![
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
fn should_preserve_scoped_0_2_given_set_quantifiers_when_rows_repeat() {
    // Arrange
    let fixture = sql_fixture("setspell-scoped_0_2", &[]);
    let sql = "(SELECT ALL n FROM (VALUES (1),(1),(2),(NULL::INT)) AS l(n) ORDER BY n NULLS LAST LIMIT 3) UNION DISTINCT (SELECT ALL n FROM (VALUES (1),(3),(NULL::INT)) AS r(n) ORDER BY n NULLS LAST LIMIT 2) ORDER BY 1 NULLS LAST";
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
fn should_preserve_scoped_0_3_given_set_quantifiers_when_rows_repeat() {
    // Arrange
    let fixture = sql_fixture("setspell-scoped_0_3", &[]);
    let sql = "(SELECT ALL n FROM (VALUES (1),(1),(2),(NULL::INT)) AS l(n) ORDER BY n NULLS LAST LIMIT 3) INTERSECT (SELECT ALL n FROM (VALUES (1),(3),(NULL::INT)) AS r(n) ORDER BY n NULLS LAST LIMIT 2) ORDER BY 1 NULLS LAST";
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
fn should_preserve_scoped_0_4_given_set_quantifiers_when_rows_repeat() {
    // Arrange
    let fixture = sql_fixture("setspell-scoped_0_4", &[]);
    let sql = "(SELECT ALL n FROM (VALUES (1),(1),(2),(NULL::INT)) AS l(n) ORDER BY n NULLS LAST LIMIT 3) INTERSECT ALL (SELECT ALL n FROM (VALUES (1),(3),(NULL::INT)) AS r(n) ORDER BY n NULLS LAST LIMIT 2) ORDER BY 1 NULLS LAST";
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
fn should_preserve_scoped_0_5_given_set_quantifiers_when_rows_repeat() {
    // Arrange
    let fixture = sql_fixture("setspell-scoped_0_5", &[]);
    let sql = "(SELECT ALL n FROM (VALUES (1),(1),(2),(NULL::INT)) AS l(n) ORDER BY n NULLS LAST LIMIT 3) INTERSECT DISTINCT (SELECT ALL n FROM (VALUES (1),(3),(NULL::INT)) AS r(n) ORDER BY n NULLS LAST LIMIT 2) ORDER BY 1 NULLS LAST";
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
fn should_preserve_scoped_0_6_given_set_quantifiers_when_rows_repeat() {
    // Arrange
    let fixture = sql_fixture("setspell-scoped_0_6", &[]);
    let sql = "(SELECT ALL n FROM (VALUES (1),(1),(2),(NULL::INT)) AS l(n) ORDER BY n NULLS LAST LIMIT 3) EXCEPT (SELECT ALL n FROM (VALUES (1),(3),(NULL::INT)) AS r(n) ORDER BY n NULLS LAST LIMIT 2) ORDER BY 1 NULLS LAST";
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
fn should_preserve_scoped_0_7_given_set_quantifiers_when_rows_repeat() {
    // Arrange
    let fixture = sql_fixture("setspell-scoped_0_7", &[]);
    let sql = "(SELECT ALL n FROM (VALUES (1),(1),(2),(NULL::INT)) AS l(n) ORDER BY n NULLS LAST LIMIT 3) EXCEPT ALL (SELECT ALL n FROM (VALUES (1),(3),(NULL::INT)) AS r(n) ORDER BY n NULLS LAST LIMIT 2) ORDER BY 1 NULLS LAST";
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
fn should_preserve_scoped_0_8_given_set_quantifiers_when_rows_repeat() {
    // Arrange
    let fixture = sql_fixture("setspell-scoped_0_8", &[]);
    let sql = "(SELECT ALL n FROM (VALUES (1),(1),(2),(NULL::INT)) AS l(n) ORDER BY n NULLS LAST LIMIT 3) EXCEPT DISTINCT (SELECT ALL n FROM (VALUES (1),(3),(NULL::INT)) AS r(n) ORDER BY n NULLS LAST LIMIT 2) ORDER BY 1 NULLS LAST";
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
fn should_preserve_scoped_1_0_given_set_quantifiers_when_rows_repeat() {
    // Arrange
    let fixture = sql_fixture("setspell-scoped_1_0", &[]);
    let sql = "(SELECT ALL n FROM (VALUES (1),(1),(1),(2)) AS l(n) ORDER BY n NULLS LAST LIMIT 3) UNION (SELECT ALL n FROM (VALUES (1),(1),(3)) AS r(n) ORDER BY n NULLS LAST LIMIT 2) ORDER BY 1 NULLS LAST";
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
fn should_preserve_scoped_1_1_given_set_quantifiers_when_rows_repeat() {
    // Arrange
    let fixture = sql_fixture("setspell-scoped_1_1", &[]);
    let sql = "(SELECT ALL n FROM (VALUES (1),(1),(1),(2)) AS l(n) ORDER BY n NULLS LAST LIMIT 3) UNION ALL (SELECT ALL n FROM (VALUES (1),(1),(3)) AS r(n) ORDER BY n NULLS LAST LIMIT 2) ORDER BY 1 NULLS LAST";
    // Act
    let result = fixture.execute(sql);
    // Assert
    let expected: Vec<Vec<Value>> = vec![
        vec![Value::Int64(1)],
        vec![Value::Int64(1)],
        vec![Value::Int64(1)],
        vec![Value::Int64(1)],
        vec![Value::Int64(1)],
    ];
    assert_eq!(
        result.expect("native set query").rows(),
        expected.as_slice()
    );
}
#[test]
fn should_preserve_scoped_1_2_given_set_quantifiers_when_rows_repeat() {
    // Arrange
    let fixture = sql_fixture("setspell-scoped_1_2", &[]);
    let sql = "(SELECT ALL n FROM (VALUES (1),(1),(1),(2)) AS l(n) ORDER BY n NULLS LAST LIMIT 3) UNION DISTINCT (SELECT ALL n FROM (VALUES (1),(1),(3)) AS r(n) ORDER BY n NULLS LAST LIMIT 2) ORDER BY 1 NULLS LAST";
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
fn should_preserve_scoped_1_3_given_set_quantifiers_when_rows_repeat() {
    // Arrange
    let fixture = sql_fixture("setspell-scoped_1_3", &[]);
    let sql = "(SELECT ALL n FROM (VALUES (1),(1),(1),(2)) AS l(n) ORDER BY n NULLS LAST LIMIT 3) INTERSECT (SELECT ALL n FROM (VALUES (1),(1),(3)) AS r(n) ORDER BY n NULLS LAST LIMIT 2) ORDER BY 1 NULLS LAST";
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
fn should_preserve_scoped_1_4_given_set_quantifiers_when_rows_repeat() {
    // Arrange
    let fixture = sql_fixture("setspell-scoped_1_4", &[]);
    let sql = "(SELECT ALL n FROM (VALUES (1),(1),(1),(2)) AS l(n) ORDER BY n NULLS LAST LIMIT 3) INTERSECT ALL (SELECT ALL n FROM (VALUES (1),(1),(3)) AS r(n) ORDER BY n NULLS LAST LIMIT 2) ORDER BY 1 NULLS LAST";
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
fn should_preserve_scoped_1_5_given_set_quantifiers_when_rows_repeat() {
    // Arrange
    let fixture = sql_fixture("setspell-scoped_1_5", &[]);
    let sql = "(SELECT ALL n FROM (VALUES (1),(1),(1),(2)) AS l(n) ORDER BY n NULLS LAST LIMIT 3) INTERSECT DISTINCT (SELECT ALL n FROM (VALUES (1),(1),(3)) AS r(n) ORDER BY n NULLS LAST LIMIT 2) ORDER BY 1 NULLS LAST";
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
fn should_preserve_scoped_1_6_given_set_quantifiers_when_rows_repeat() {
    // Arrange
    let fixture = sql_fixture("setspell-scoped_1_6", &[]);
    let sql = "(SELECT ALL n FROM (VALUES (1),(1),(1),(2)) AS l(n) ORDER BY n NULLS LAST LIMIT 3) EXCEPT (SELECT ALL n FROM (VALUES (1),(1),(3)) AS r(n) ORDER BY n NULLS LAST LIMIT 2) ORDER BY 1 NULLS LAST";
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
fn should_preserve_scoped_1_7_given_set_quantifiers_when_rows_repeat() {
    // Arrange
    let fixture = sql_fixture("setspell-scoped_1_7", &[]);
    let sql = "(SELECT ALL n FROM (VALUES (1),(1),(1),(2)) AS l(n) ORDER BY n NULLS LAST LIMIT 3) EXCEPT ALL (SELECT ALL n FROM (VALUES (1),(1),(3)) AS r(n) ORDER BY n NULLS LAST LIMIT 2) ORDER BY 1 NULLS LAST";
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
fn should_preserve_scoped_1_8_given_set_quantifiers_when_rows_repeat() {
    // Arrange
    let fixture = sql_fixture("setspell-scoped_1_8", &[]);
    let sql = "(SELECT ALL n FROM (VALUES (1),(1),(1),(2)) AS l(n) ORDER BY n NULLS LAST LIMIT 3) EXCEPT DISTINCT (SELECT ALL n FROM (VALUES (1),(1),(3)) AS r(n) ORDER BY n NULLS LAST LIMIT 2) ORDER BY 1 NULLS LAST";
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
fn should_preserve_scoped_2_0_given_set_quantifiers_when_rows_repeat() {
    // Arrange
    let fixture = sql_fixture("setspell-scoped_2_0", &[]);
    let sql = "(SELECT ALL n FROM (VALUES (NULL::INT),(NULL::INT),(2)) AS l(n) ORDER BY n NULLS LAST LIMIT 3) UNION (SELECT ALL n FROM (VALUES (NULL::INT),(2),(2)) AS r(n) ORDER BY n NULLS LAST LIMIT 2) ORDER BY 1 NULLS LAST";
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
fn should_preserve_scoped_2_1_given_set_quantifiers_when_rows_repeat() {
    // Arrange
    let fixture = sql_fixture("setspell-scoped_2_1", &[]);
    let sql = "(SELECT ALL n FROM (VALUES (NULL::INT),(NULL::INT),(2)) AS l(n) ORDER BY n NULLS LAST LIMIT 3) UNION ALL (SELECT ALL n FROM (VALUES (NULL::INT),(2),(2)) AS r(n) ORDER BY n NULLS LAST LIMIT 2) ORDER BY 1 NULLS LAST";
    // Act
    let result = fixture.execute(sql);
    // Assert
    let expected: Vec<Vec<Value>> = vec![
        vec![Value::Int64(2)],
        vec![Value::Int64(2)],
        vec![Value::Int64(2)],
        vec![Value::Null],
        vec![Value::Null],
    ];
    assert_eq!(
        result.expect("native set query").rows(),
        expected.as_slice()
    );
}
#[test]
fn should_preserve_scoped_2_2_given_set_quantifiers_when_rows_repeat() {
    // Arrange
    let fixture = sql_fixture("setspell-scoped_2_2", &[]);
    let sql = "(SELECT ALL n FROM (VALUES (NULL::INT),(NULL::INT),(2)) AS l(n) ORDER BY n NULLS LAST LIMIT 3) UNION DISTINCT (SELECT ALL n FROM (VALUES (NULL::INT),(2),(2)) AS r(n) ORDER BY n NULLS LAST LIMIT 2) ORDER BY 1 NULLS LAST";
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
fn should_preserve_scoped_2_3_given_set_quantifiers_when_rows_repeat() {
    // Arrange
    let fixture = sql_fixture("setspell-scoped_2_3", &[]);
    let sql = "(SELECT ALL n FROM (VALUES (NULL::INT),(NULL::INT),(2)) AS l(n) ORDER BY n NULLS LAST LIMIT 3) INTERSECT (SELECT ALL n FROM (VALUES (NULL::INT),(2),(2)) AS r(n) ORDER BY n NULLS LAST LIMIT 2) ORDER BY 1 NULLS LAST";
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
fn should_preserve_scoped_2_4_given_set_quantifiers_when_rows_repeat() {
    // Arrange
    let fixture = sql_fixture("setspell-scoped_2_4", &[]);
    let sql = "(SELECT ALL n FROM (VALUES (NULL::INT),(NULL::INT),(2)) AS l(n) ORDER BY n NULLS LAST LIMIT 3) INTERSECT ALL (SELECT ALL n FROM (VALUES (NULL::INT),(2),(2)) AS r(n) ORDER BY n NULLS LAST LIMIT 2) ORDER BY 1 NULLS LAST";
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
fn should_preserve_scoped_2_5_given_set_quantifiers_when_rows_repeat() {
    // Arrange
    let fixture = sql_fixture("setspell-scoped_2_5", &[]);
    let sql = "(SELECT ALL n FROM (VALUES (NULL::INT),(NULL::INT),(2)) AS l(n) ORDER BY n NULLS LAST LIMIT 3) INTERSECT DISTINCT (SELECT ALL n FROM (VALUES (NULL::INT),(2),(2)) AS r(n) ORDER BY n NULLS LAST LIMIT 2) ORDER BY 1 NULLS LAST";
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
fn should_preserve_scoped_2_6_given_set_quantifiers_when_rows_repeat() {
    // Arrange
    let fixture = sql_fixture("setspell-scoped_2_6", &[]);
    let sql = "(SELECT ALL n FROM (VALUES (NULL::INT),(NULL::INT),(2)) AS l(n) ORDER BY n NULLS LAST LIMIT 3) EXCEPT (SELECT ALL n FROM (VALUES (NULL::INT),(2),(2)) AS r(n) ORDER BY n NULLS LAST LIMIT 2) ORDER BY 1 NULLS LAST";
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
fn should_preserve_scoped_2_7_given_set_quantifiers_when_rows_repeat() {
    // Arrange
    let fixture = sql_fixture("setspell-scoped_2_7", &[]);
    let sql = "(SELECT ALL n FROM (VALUES (NULL::INT),(NULL::INT),(2)) AS l(n) ORDER BY n NULLS LAST LIMIT 3) EXCEPT ALL (SELECT ALL n FROM (VALUES (NULL::INT),(2),(2)) AS r(n) ORDER BY n NULLS LAST LIMIT 2) ORDER BY 1 NULLS LAST";
    // Act
    let result = fixture.execute(sql);
    // Assert
    let expected: Vec<Vec<Value>> = vec![vec![Value::Null], vec![Value::Null]];
    assert_eq!(
        result.expect("native set query").rows(),
        expected.as_slice()
    );
}
#[test]
fn should_preserve_scoped_2_8_given_set_quantifiers_when_rows_repeat() {
    // Arrange
    let fixture = sql_fixture("setspell-scoped_2_8", &[]);
    let sql = "(SELECT ALL n FROM (VALUES (NULL::INT),(NULL::INT),(2)) AS l(n) ORDER BY n NULLS LAST LIMIT 3) EXCEPT DISTINCT (SELECT ALL n FROM (VALUES (NULL::INT),(2),(2)) AS r(n) ORDER BY n NULLS LAST LIMIT 2) ORDER BY 1 NULLS LAST";
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
fn should_preserve_scoped_3_0_given_set_quantifiers_when_rows_repeat() {
    // Arrange
    let fixture = sql_fixture("setspell-scoped_3_0", &[]);
    let sql = "(SELECT ALL n FROM (VALUES (1),(2)) AS l(n) ORDER BY n NULLS LAST LIMIT 3) UNION (SELECT ALL n FROM (VALUES (3),(4)) AS r(n) ORDER BY n NULLS LAST LIMIT 2) ORDER BY 1 NULLS LAST";
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
fn should_preserve_scoped_3_1_given_set_quantifiers_when_rows_repeat() {
    // Arrange
    let fixture = sql_fixture("setspell-scoped_3_1", &[]);
    let sql = "(SELECT ALL n FROM (VALUES (1),(2)) AS l(n) ORDER BY n NULLS LAST LIMIT 3) UNION ALL (SELECT ALL n FROM (VALUES (3),(4)) AS r(n) ORDER BY n NULLS LAST LIMIT 2) ORDER BY 1 NULLS LAST";
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
fn should_preserve_scoped_3_2_given_set_quantifiers_when_rows_repeat() {
    // Arrange
    let fixture = sql_fixture("setspell-scoped_3_2", &[]);
    let sql = "(SELECT ALL n FROM (VALUES (1),(2)) AS l(n) ORDER BY n NULLS LAST LIMIT 3) UNION DISTINCT (SELECT ALL n FROM (VALUES (3),(4)) AS r(n) ORDER BY n NULLS LAST LIMIT 2) ORDER BY 1 NULLS LAST";
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
fn should_preserve_scoped_3_3_given_set_quantifiers_when_rows_repeat() {
    // Arrange
    let fixture = sql_fixture("setspell-scoped_3_3", &[]);
    let sql = "(SELECT ALL n FROM (VALUES (1),(2)) AS l(n) ORDER BY n NULLS LAST LIMIT 3) INTERSECT (SELECT ALL n FROM (VALUES (3),(4)) AS r(n) ORDER BY n NULLS LAST LIMIT 2) ORDER BY 1 NULLS LAST";
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
fn should_preserve_scoped_3_4_given_set_quantifiers_when_rows_repeat() {
    // Arrange
    let fixture = sql_fixture("setspell-scoped_3_4", &[]);
    let sql = "(SELECT ALL n FROM (VALUES (1),(2)) AS l(n) ORDER BY n NULLS LAST LIMIT 3) INTERSECT ALL (SELECT ALL n FROM (VALUES (3),(4)) AS r(n) ORDER BY n NULLS LAST LIMIT 2) ORDER BY 1 NULLS LAST";
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
fn should_preserve_scoped_3_5_given_set_quantifiers_when_rows_repeat() {
    // Arrange
    let fixture = sql_fixture("setspell-scoped_3_5", &[]);
    let sql = "(SELECT ALL n FROM (VALUES (1),(2)) AS l(n) ORDER BY n NULLS LAST LIMIT 3) INTERSECT DISTINCT (SELECT ALL n FROM (VALUES (3),(4)) AS r(n) ORDER BY n NULLS LAST LIMIT 2) ORDER BY 1 NULLS LAST";
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
fn should_preserve_scoped_3_6_given_set_quantifiers_when_rows_repeat() {
    // Arrange
    let fixture = sql_fixture("setspell-scoped_3_6", &[]);
    let sql = "(SELECT ALL n FROM (VALUES (1),(2)) AS l(n) ORDER BY n NULLS LAST LIMIT 3) EXCEPT (SELECT ALL n FROM (VALUES (3),(4)) AS r(n) ORDER BY n NULLS LAST LIMIT 2) ORDER BY 1 NULLS LAST";
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
fn should_preserve_scoped_3_7_given_set_quantifiers_when_rows_repeat() {
    // Arrange
    let fixture = sql_fixture("setspell-scoped_3_7", &[]);
    let sql = "(SELECT ALL n FROM (VALUES (1),(2)) AS l(n) ORDER BY n NULLS LAST LIMIT 3) EXCEPT ALL (SELECT ALL n FROM (VALUES (3),(4)) AS r(n) ORDER BY n NULLS LAST LIMIT 2) ORDER BY 1 NULLS LAST";
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
fn should_preserve_scoped_3_8_given_set_quantifiers_when_rows_repeat() {
    // Arrange
    let fixture = sql_fixture("setspell-scoped_3_8", &[]);
    let sql = "(SELECT ALL n FROM (VALUES (1),(2)) AS l(n) ORDER BY n NULLS LAST LIMIT 3) EXCEPT DISTINCT (SELECT ALL n FROM (VALUES (3),(4)) AS r(n) ORDER BY n NULLS LAST LIMIT 2) ORDER BY 1 NULLS LAST";
    // Act
    let result = fixture.execute(sql);
    // Assert
    let expected: Vec<Vec<Value>> = vec![vec![Value::Int64(1)], vec![Value::Int64(2)]];
    assert_eq!(
        result.expect("native set query").rows(),
        expected.as_slice()
    );
}
