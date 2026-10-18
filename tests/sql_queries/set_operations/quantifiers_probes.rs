use crate::support_sql_fixture::sql_fixture;
use cassie::types::Value;
#[test]
fn should_preserve_text_intersect_all_given_set_inputs_when_multiplicity_is_resolved() {
    // Arrange
    let fixture = sql_fixture("setspell-text_intersect_all", &[]);
    let sql = "SELECT a FROM (VALUES ('a'::TEXT),('a'),('b'),(NULL::TEXT)) AS l(a) INTERSECT ALL SELECT a FROM (VALUES ('a'::TEXT),(NULL::TEXT)) AS r(a) ORDER BY 1 NULLS LAST";
    // Act
    let result = fixture.execute(sql);
    // Assert
    let expected: Vec<Vec<Value>> = vec![vec![Value::String("a".to_owned())], vec![Value::Null]];
    assert_eq!(result.expect("native probe").rows(), expected.as_slice());
}
#[test]
fn should_preserve_tuple_intersect_all_given_set_inputs_when_multiplicity_is_resolved() {
    // Arrange
    let fixture = sql_fixture("setspell-tuple_intersect_all", &[]);
    let sql = "SELECT a,b FROM (VALUES ('a'::TEXT,1),('a',1),('a',2),(NULL::TEXT,1)) AS l(a,b) INTERSECT ALL SELECT a,b FROM (VALUES ('a'::TEXT,1),(NULL::TEXT,1)) AS r(a,b) ORDER BY 1 NULLS LAST,2";
    // Act
    let result = fixture.execute(sql);
    // Assert
    let expected: Vec<Vec<Value>> = vec![
        vec![Value::String("a".to_owned()), Value::Int64(1)],
        vec![Value::Null, Value::Int64(1)],
    ];
    assert_eq!(result.expect("native probe").rows(), expected.as_slice());
}
#[test]
fn should_preserve_empty_left_intersect_all_given_set_inputs_when_multiplicity_is_resolved() {
    // Arrange
    let fixture = sql_fixture("setspell-empty_left_intersect_all", &[]);
    let sql = "SELECT a,b FROM (VALUES ('a'::TEXT,1),('a',1),('a',2),(NULL::TEXT,1)) AS l(a,b) WHERE FALSE INTERSECT ALL SELECT a,b FROM (VALUES ('a'::TEXT,1),(NULL::TEXT,1)) AS r(a,b) ORDER BY 1 NULLS LAST,2";
    // Act
    let result = fixture.execute(sql);
    // Assert
    let expected: Vec<Vec<Value>> = vec![];
    assert_eq!(result.expect("native probe").rows(), expected.as_slice());
}
#[test]
fn should_preserve_empty_right_intersect_all_given_set_inputs_when_multiplicity_is_resolved() {
    // Arrange
    let fixture = sql_fixture("setspell-empty_right_intersect_all", &[]);
    let sql = "SELECT a,b FROM (VALUES ('a'::TEXT,1),('a',1),('a',2),(NULL::TEXT,1)) AS l(a,b) INTERSECT ALL SELECT a,b FROM (VALUES ('a'::TEXT,1),(NULL::TEXT,1)) AS r(a,b) WHERE FALSE ORDER BY 1 NULLS LAST,2";
    // Act
    let result = fixture.execute(sql);
    // Assert
    let expected: Vec<Vec<Value>> = vec![];
    assert_eq!(result.expect("native probe").rows(), expected.as_slice());
}
#[test]
fn should_preserve_comments_intersect_all_given_set_inputs_when_multiplicity_is_resolved() {
    // Arrange
    let fixture = sql_fixture("setspell-comments_intersect_all", &[]);
    let sql = "SELECT a,b FROM (VALUES ('a'::TEXT,1),('a',1),('a',2),(NULL::TEXT,1)) AS l(a,b) INTERSECT /* set quantifier */\nALL SELECT a,b FROM (VALUES ('a'::TEXT,1),(NULL::TEXT,1)) AS r(a,b) ORDER BY 1 NULLS LAST,2";
    // Act
    let result = fixture.execute(sql);
    // Assert
    let expected: Vec<Vec<Value>> = vec![
        vec![Value::String("a".to_owned()), Value::Int64(1)],
        vec![Value::Null, Value::Int64(1)],
    ];
    assert_eq!(result.expect("native probe").rows(), expected.as_slice());
}
#[test]
fn should_preserve_text_except_all_given_set_inputs_when_multiplicity_is_resolved() {
    // Arrange
    let fixture = sql_fixture("setspell-text_except_all", &[]);
    let sql = "SELECT a FROM (VALUES ('a'::TEXT),('a'),('b'),(NULL::TEXT)) AS l(a) EXCEPT ALL SELECT a FROM (VALUES ('a'::TEXT),(NULL::TEXT)) AS r(a) ORDER BY 1 NULLS LAST";
    // Act
    let result = fixture.execute(sql);
    // Assert
    let expected: Vec<Vec<Value>> = vec![
        vec![Value::String("a".to_owned())],
        vec![Value::String("b".to_owned())],
    ];
    assert_eq!(result.expect("native probe").rows(), expected.as_slice());
}
#[test]
fn should_preserve_tuple_except_all_given_set_inputs_when_multiplicity_is_resolved() {
    // Arrange
    let fixture = sql_fixture("setspell-tuple_except_all", &[]);
    let sql = "SELECT a,b FROM (VALUES ('a'::TEXT,1),('a',1),('a',2),(NULL::TEXT,1)) AS l(a,b) EXCEPT ALL SELECT a,b FROM (VALUES ('a'::TEXT,1),(NULL::TEXT,1)) AS r(a,b) ORDER BY 1 NULLS LAST,2";
    // Act
    let result = fixture.execute(sql);
    // Assert
    let expected: Vec<Vec<Value>> = vec![
        vec![Value::String("a".to_owned()), Value::Int64(1)],
        vec![Value::String("a".to_owned()), Value::Int64(2)],
    ];
    assert_eq!(result.expect("native probe").rows(), expected.as_slice());
}
#[test]
fn should_preserve_empty_left_except_all_given_set_inputs_when_multiplicity_is_resolved() {
    // Arrange
    let fixture = sql_fixture("setspell-empty_left_except_all", &[]);
    let sql = "SELECT a,b FROM (VALUES ('a'::TEXT,1),('a',1),('a',2),(NULL::TEXT,1)) AS l(a,b) WHERE FALSE EXCEPT ALL SELECT a,b FROM (VALUES ('a'::TEXT,1),(NULL::TEXT,1)) AS r(a,b) ORDER BY 1 NULLS LAST,2";
    // Act
    let result = fixture.execute(sql);
    // Assert
    let expected: Vec<Vec<Value>> = vec![];
    assert_eq!(result.expect("native probe").rows(), expected.as_slice());
}
#[test]
fn should_preserve_empty_right_except_all_given_set_inputs_when_multiplicity_is_resolved() {
    // Arrange
    let fixture = sql_fixture("setspell-empty_right_except_all", &[]);
    let sql = "SELECT a,b FROM (VALUES ('a'::TEXT,1),('a',1),('a',2),(NULL::TEXT,1)) AS l(a,b) EXCEPT ALL SELECT a,b FROM (VALUES ('a'::TEXT,1),(NULL::TEXT,1)) AS r(a,b) WHERE FALSE ORDER BY 1 NULLS LAST,2";
    // Act
    let result = fixture.execute(sql);
    // Assert
    let expected: Vec<Vec<Value>> = vec![
        vec![Value::String("a".to_owned()), Value::Int64(1)],
        vec![Value::String("a".to_owned()), Value::Int64(1)],
        vec![Value::String("a".to_owned()), Value::Int64(2)],
        vec![Value::Null, Value::Int64(1)],
    ];
    assert_eq!(result.expect("native probe").rows(), expected.as_slice());
}
#[test]
fn should_preserve_comments_except_all_given_set_inputs_when_multiplicity_is_resolved() {
    // Arrange
    let fixture = sql_fixture("setspell-comments_except_all", &[]);
    let sql = "SELECT a,b FROM (VALUES ('a'::TEXT,1),('a',1),('a',2),(NULL::TEXT,1)) AS l(a,b) EXCEPT /* set quantifier */\nALL SELECT a,b FROM (VALUES ('a'::TEXT,1),(NULL::TEXT,1)) AS r(a,b) ORDER BY 1 NULLS LAST,2";
    // Act
    let result = fixture.execute(sql);
    // Assert
    let expected: Vec<Vec<Value>> = vec![
        vec![Value::String("a".to_owned()), Value::Int64(1)],
        vec![Value::String("a".to_owned()), Value::Int64(2)],
    ];
    assert_eq!(result.expect("native probe").rows(), expected.as_slice());
}
#[test]
fn should_preserve_text_union_distinct_given_set_inputs_when_multiplicity_is_resolved() {
    // Arrange
    let fixture = sql_fixture("setspell-text_union_distinct", &[]);
    let sql = "SELECT a FROM (VALUES ('a'::TEXT),('a'),('b'),(NULL::TEXT)) AS l(a) UNION DISTINCT SELECT a FROM (VALUES ('a'::TEXT),(NULL::TEXT)) AS r(a) ORDER BY 1 NULLS LAST";
    // Act
    let result = fixture.execute(sql);
    // Assert
    let expected: Vec<Vec<Value>> = vec![
        vec![Value::String("a".to_owned())],
        vec![Value::String("b".to_owned())],
        vec![Value::Null],
    ];
    assert_eq!(result.expect("native probe").rows(), expected.as_slice());
}
#[test]
fn should_preserve_tuple_union_distinct_given_set_inputs_when_multiplicity_is_resolved() {
    // Arrange
    let fixture = sql_fixture("setspell-tuple_union_distinct", &[]);
    let sql = "SELECT a,b FROM (VALUES ('a'::TEXT,1),('a',1),('a',2),(NULL::TEXT,1)) AS l(a,b) UNION DISTINCT SELECT a,b FROM (VALUES ('a'::TEXT,1),(NULL::TEXT,1)) AS r(a,b) ORDER BY 1 NULLS LAST,2";
    // Act
    let result = fixture.execute(sql);
    // Assert
    let expected: Vec<Vec<Value>> = vec![
        vec![Value::String("a".to_owned()), Value::Int64(1)],
        vec![Value::String("a".to_owned()), Value::Int64(2)],
        vec![Value::Null, Value::Int64(1)],
    ];
    assert_eq!(result.expect("native probe").rows(), expected.as_slice());
}
#[test]
fn should_preserve_empty_left_union_distinct_given_set_inputs_when_multiplicity_is_resolved() {
    // Arrange
    let fixture = sql_fixture("setspell-empty_left_union_distinct", &[]);
    let sql = "SELECT a,b FROM (VALUES ('a'::TEXT,1),('a',1),('a',2),(NULL::TEXT,1)) AS l(a,b) WHERE FALSE UNION DISTINCT SELECT a,b FROM (VALUES ('a'::TEXT,1),(NULL::TEXT,1)) AS r(a,b) ORDER BY 1 NULLS LAST,2";
    // Act
    let result = fixture.execute(sql);
    // Assert
    let expected: Vec<Vec<Value>> = vec![
        vec![Value::String("a".to_owned()), Value::Int64(1)],
        vec![Value::Null, Value::Int64(1)],
    ];
    assert_eq!(result.expect("native probe").rows(), expected.as_slice());
}
#[test]
fn should_preserve_empty_right_union_distinct_given_set_inputs_when_multiplicity_is_resolved() {
    // Arrange
    let fixture = sql_fixture("setspell-empty_right_union_distinct", &[]);
    let sql = "SELECT a,b FROM (VALUES ('a'::TEXT,1),('a',1),('a',2),(NULL::TEXT,1)) AS l(a,b) UNION DISTINCT SELECT a,b FROM (VALUES ('a'::TEXT,1),(NULL::TEXT,1)) AS r(a,b) WHERE FALSE ORDER BY 1 NULLS LAST,2";
    // Act
    let result = fixture.execute(sql);
    // Assert
    let expected: Vec<Vec<Value>> = vec![
        vec![Value::String("a".to_owned()), Value::Int64(1)],
        vec![Value::String("a".to_owned()), Value::Int64(2)],
        vec![Value::Null, Value::Int64(1)],
    ];
    assert_eq!(result.expect("native probe").rows(), expected.as_slice());
}
#[test]
fn should_preserve_comments_union_distinct_given_set_inputs_when_multiplicity_is_resolved() {
    // Arrange
    let fixture = sql_fixture("setspell-comments_union_distinct", &[]);
    let sql = "SELECT a,b FROM (VALUES ('a'::TEXT,1),('a',1),('a',2),(NULL::TEXT,1)) AS l(a,b) UNION /* set quantifier */\nDISTINCT SELECT a,b FROM (VALUES ('a'::TEXT,1),(NULL::TEXT,1)) AS r(a,b) ORDER BY 1 NULLS LAST,2";
    // Act
    let result = fixture.execute(sql);
    // Assert
    let expected: Vec<Vec<Value>> = vec![
        vec![Value::String("a".to_owned()), Value::Int64(1)],
        vec![Value::String("a".to_owned()), Value::Int64(2)],
        vec![Value::Null, Value::Int64(1)],
    ];
    assert_eq!(result.expect("native probe").rows(), expected.as_slice());
}
#[test]
fn should_preserve_cte_tuple_1_given_multiset_keys_when_materialization_is_required() {
    // Arrange
    let fixture = sql_fixture("setspell-cte-tuple-1", &[]);
    let sql = "WITH combined AS (SELECT a,b FROM (VALUES ('a'::TEXT,1),('a',1),('a',2),(NULL::TEXT,1)) AS l(a,b) INTERSECT ALL SELECT a,b FROM (VALUES ('a'::TEXT,1),(NULL::TEXT,1)) AS r(a,b) ORDER BY 1 NULLS LAST,2) SELECT a,b FROM combined ORDER BY a NULLS LAST,b";
    // Act
    let result = fixture.execute(sql);
    // Assert
    let expected: Vec<Vec<Value>> = vec![
        vec![Value::String("a".to_owned()), Value::Int64(1)],
        vec![Value::Null, Value::Int64(1)],
    ];
    assert_eq!(
        result.expect("materialized tuple multiset").rows(),
        expected.as_slice()
    );
}
#[test]
fn should_preserve_cte_tuple_6_given_multiset_keys_when_materialization_is_required() {
    // Arrange
    let fixture = sql_fixture("setspell-cte-tuple-6", &[]);
    let sql = "WITH combined AS (SELECT a,b FROM (VALUES ('a'::TEXT,1),('a',1),('a',2),(NULL::TEXT,1)) AS l(a,b) EXCEPT ALL SELECT a,b FROM (VALUES ('a'::TEXT,1),(NULL::TEXT,1)) AS r(a,b) ORDER BY 1 NULLS LAST,2) SELECT a,b FROM combined ORDER BY a NULLS LAST,b";
    // Act
    let result = fixture.execute(sql);
    // Assert
    let expected: Vec<Vec<Value>> = vec![
        vec![Value::String("a".to_owned()), Value::Int64(1)],
        vec![Value::String("a".to_owned()), Value::Int64(2)],
    ];
    assert_eq!(
        result.expect("materialized tuple multiset").rows(),
        expected.as_slice()
    );
}
#[test]
fn should_preserve_cte_tuple_11_given_multiset_keys_when_materialization_is_required() {
    // Arrange
    let fixture = sql_fixture("setspell-cte-tuple-11", &[]);
    let sql = "WITH combined AS (SELECT a,b FROM (VALUES ('a'::TEXT,1),('a',1),('a',2),(NULL::TEXT,1)) AS l(a,b) UNION DISTINCT SELECT a,b FROM (VALUES ('a'::TEXT,1),(NULL::TEXT,1)) AS r(a,b) ORDER BY 1 NULLS LAST,2) SELECT a,b FROM combined ORDER BY a NULLS LAST,b";
    // Act
    let result = fixture.execute(sql);
    // Assert
    let expected: Vec<Vec<Value>> = vec![
        vec![Value::String("a".to_owned()), Value::Int64(1)],
        vec![Value::String("a".to_owned()), Value::Int64(2)],
        vec![Value::Null, Value::Int64(1)],
    ];
    assert_eq!(
        result.expect("materialized tuple multiset").rows(),
        expected.as_slice()
    );
}
