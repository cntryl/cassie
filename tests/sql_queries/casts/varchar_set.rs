//! Native explicit VARCHAR casts through query contexts.
use crate::support_sql_fixture::sql_fixture;
use cassie::types::Value;

#[test]
fn should_truncate_ascii_given_explicit_varchar_when_set_is_converted_to_text() {
    // Arrange
    let fixture = sql_fixture("varchar-ascii_set-text", &[]);
    // Act
    let result = fixture.execute("SELECT v::TEXT AS output FROM (SELECT ('abcdef')::VARCHAR(3) AS output UNION ALL SELECT NULL::VARCHAR(3) ORDER BY 1 NULLS LAST) AS q(v) ORDER BY 1 NULLS LAST");
    // Assert
    let expected: Vec<Vec<Value>> = vec![vec![Value::String("abc".into())], vec![Value::Null]];
    assert_eq!(
        result.expect("explicit VARCHAR").rows(),
        expected.as_slice()
    );
}

#[test]
fn should_truncate_exact_given_explicit_varchar_when_set_is_converted_to_text() {
    // Arrange
    let fixture = sql_fixture("varchar-exact_set-text", &[]);
    // Act
    let result = fixture.execute("SELECT v::TEXT AS output FROM (SELECT ('abc')::VARCHAR(3) AS output UNION ALL SELECT NULL::VARCHAR(3) ORDER BY 1 NULLS LAST) AS q(v) ORDER BY 1 NULLS LAST");
    // Assert
    let expected: Vec<Vec<Value>> = vec![vec![Value::String("abc".into())], vec![Value::Null]];
    assert_eq!(
        result.expect("explicit VARCHAR").rows(),
        expected.as_slice()
    );
}

#[test]
fn should_truncate_short_given_explicit_varchar_when_set_is_converted_to_text() {
    // Arrange
    let fixture = sql_fixture("varchar-short_set-text", &[]);
    // Act
    let result = fixture.execute("SELECT v::TEXT AS output FROM (SELECT ('a')::VARCHAR(3) AS output UNION ALL SELECT NULL::VARCHAR(3) ORDER BY 1 NULLS LAST) AS q(v) ORDER BY 1 NULLS LAST");
    // Assert
    let expected: Vec<Vec<Value>> = vec![vec![Value::String("a".into())], vec![Value::Null]];
    assert_eq!(
        result.expect("explicit VARCHAR").rows(),
        expected.as_slice()
    );
}

#[test]
fn should_truncate_empty_given_explicit_varchar_when_set_is_converted_to_text() {
    // Arrange
    let fixture = sql_fixture("varchar-empty_set-text", &[]);
    // Act
    let result = fixture.execute("SELECT v::TEXT AS output FROM (SELECT ('')::VARCHAR(3) AS output UNION ALL SELECT NULL::VARCHAR(3) ORDER BY 1 NULLS LAST) AS q(v) ORDER BY 1 NULLS LAST");
    // Assert
    let expected: Vec<Vec<Value>> = vec![vec![Value::String(String::new())], vec![Value::Null]];
    assert_eq!(
        result.expect("explicit VARCHAR").rows(),
        expected.as_slice()
    );
}

#[test]
fn should_truncate_spaces_given_explicit_varchar_when_set_is_converted_to_text() {
    // Arrange
    let fixture = sql_fixture("varchar-spaces_set-text", &[]);
    // Act
    let result = fixture.execute("SELECT v::TEXT AS output FROM (SELECT ('a    ')::VARCHAR(3) AS output UNION ALL SELECT NULL::VARCHAR(3) ORDER BY 1 NULLS LAST) AS q(v) ORDER BY 1 NULLS LAST");
    // Assert
    let expected: Vec<Vec<Value>> = vec![vec![Value::String("a  ".into())], vec![Value::Null]];
    assert_eq!(
        result.expect("explicit VARCHAR").rows(),
        expected.as_slice()
    );
}

#[test]
fn should_truncate_leading_given_explicit_varchar_when_set_is_converted_to_text() {
    // Arrange
    let fixture = sql_fixture("varchar-leading_set-text", &[]);
    // Act
    let result = fixture.execute("SELECT v::TEXT AS output FROM (SELECT ('   abc')::VARCHAR(3) AS output UNION ALL SELECT NULL::VARCHAR(3) ORDER BY 1 NULLS LAST) AS q(v) ORDER BY 1 NULLS LAST");
    // Assert
    let expected: Vec<Vec<Value>> = vec![vec![Value::String("   ".into())], vec![Value::Null]];
    assert_eq!(
        result.expect("explicit VARCHAR").rows(),
        expected.as_slice()
    );
}

#[test]
fn should_truncate_unicode_given_explicit_varchar_when_set_is_converted_to_text() {
    // Arrange
    let fixture = sql_fixture("varchar-unicode_set-text", &[]);
    // Act
    let result = fixture.execute("SELECT v::TEXT AS output FROM (SELECT ('a🙂文z')::VARCHAR(3) AS output UNION ALL SELECT NULL::VARCHAR(3) ORDER BY 1 NULLS LAST) AS q(v) ORDER BY 1 NULLS LAST");
    // Assert
    let expected: Vec<Vec<Value>> = vec![vec![Value::String("a🙂文".into())], vec![Value::Null]];
    assert_eq!(
        result.expect("explicit VARCHAR").rows(),
        expected.as_slice()
    );
}

#[test]
fn should_truncate_emoji_given_explicit_varchar_when_set_is_converted_to_text() {
    // Arrange
    let fixture = sql_fixture("varchar-emoji_set-text", &[]);
    // Act
    let result = fixture.execute("SELECT v::TEXT AS output FROM (SELECT ('🙂🙃🙂🙃')::VARCHAR(2) AS output UNION ALL SELECT NULL::VARCHAR(2) ORDER BY 1 NULLS LAST) AS q(v) ORDER BY 1 NULLS LAST");
    // Assert
    let expected: Vec<Vec<Value>> = vec![vec![Value::String("🙂🙃".into())], vec![Value::Null]];
    assert_eq!(
        result.expect("explicit VARCHAR").rows(),
        expected.as_slice()
    );
}

#[test]
fn should_truncate_combining_given_explicit_varchar_when_set_is_converted_to_text() {
    // Arrange
    let fixture = sql_fixture("varchar-combining_set-text", &[]);
    // Act
    let result = fixture.execute("SELECT v::TEXT AS output FROM (SELECT ('e\u{301}xy')::VARCHAR(2) AS output UNION ALL SELECT NULL::VARCHAR(2) ORDER BY 1 NULLS LAST) AS q(v) ORDER BY 1 NULLS LAST");
    // Assert
    let expected: Vec<Vec<Value>> = vec![vec![Value::String("e\u{301}".into())], vec![Value::Null]];
    assert_eq!(
        result.expect("explicit VARCHAR").rows(),
        expected.as_slice()
    );
}

#[test]
fn should_truncate_null_given_explicit_varchar_when_set_is_converted_to_text() {
    // Arrange
    let fixture = sql_fixture("varchar-null_set-text", &[]);
    // Act
    let result = fixture.execute("SELECT v::TEXT AS output FROM (SELECT (NULL)::VARCHAR(3) AS output UNION ALL SELECT NULL::VARCHAR(3) ORDER BY 1 NULLS LAST) AS q(v) ORDER BY 1 NULLS LAST");
    // Assert
    let expected: Vec<Vec<Value>> = vec![vec![Value::Null], vec![Value::Null]];
    assert_eq!(
        result.expect("explicit VARCHAR").rows(),
        expected.as_slice()
    );
}

#[test]
fn should_truncate_char_given_explicit_varchar_when_set_is_converted_to_text() {
    // Arrange
    let fixture = sql_fixture("varchar-char_set-text", &[]);
    // Act
    let result = fixture.execute("SELECT v::TEXT AS output FROM (SELECT ('a'::CHAR(6))::VARCHAR(3) AS output UNION ALL SELECT NULL::VARCHAR(3) ORDER BY 1 NULLS LAST) AS q(v) ORDER BY 1 NULLS LAST");
    // Assert
    let expected: Vec<Vec<Value>> = vec![vec![Value::String("a".into())], vec![Value::Null]];
    assert_eq!(
        result.expect("explicit VARCHAR").rows(),
        expected.as_slice()
    );
}

#[test]
fn should_truncate_char_long_given_explicit_varchar_when_set_is_converted_to_text() {
    // Arrange
    let fixture = sql_fixture("varchar-char_long_set-text", &[]);
    // Act
    let result = fixture.execute("SELECT v::TEXT AS output FROM (SELECT ('abcdef'::CHAR(8))::VARCHAR(3) AS output UNION ALL SELECT NULL::VARCHAR(3) ORDER BY 1 NULLS LAST) AS q(v) ORDER BY 1 NULLS LAST");
    // Assert
    let expected: Vec<Vec<Value>> = vec![vec![Value::String("abc".into())], vec![Value::Null]];
    assert_eq!(
        result.expect("explicit VARCHAR").rows(),
        expected.as_slice()
    );
}

#[test]
fn should_truncate_integer_given_explicit_varchar_when_set_is_converted_to_text() {
    // Arrange
    let fixture = sql_fixture("varchar-integer_set-text", &[]);
    // Act
    let result = fixture.execute("SELECT v::TEXT AS output FROM (SELECT (123456::BIGINT)::VARCHAR(3) AS output UNION ALL SELECT NULL::VARCHAR(3) ORDER BY 1 NULLS LAST) AS q(v) ORDER BY 1 NULLS LAST");
    // Assert
    let expected: Vec<Vec<Value>> = vec![vec![Value::String("123".into())], vec![Value::Null]];
    assert_eq!(
        result.expect("explicit VARCHAR").rows(),
        expected.as_slice()
    );
}

#[test]
fn should_truncate_negative_given_explicit_varchar_when_set_is_converted_to_text() {
    // Arrange
    let fixture = sql_fixture("varchar-negative_set-text", &[]);
    // Act
    let result = fixture.execute("SELECT v::TEXT AS output FROM (SELECT ((-123456)::BIGINT)::VARCHAR(4) AS output UNION ALL SELECT NULL::VARCHAR(4) ORDER BY 1 NULLS LAST) AS q(v) ORDER BY 1 NULLS LAST");
    // Assert
    let expected: Vec<Vec<Value>> = vec![vec![Value::String("-123".into())], vec![Value::Null]];
    assert_eq!(
        result.expect("explicit VARCHAR").rows(),
        expected.as_slice()
    );
}

#[test]
fn should_truncate_boolean_given_explicit_varchar_when_set_is_converted_to_text() {
    // Arrange
    let fixture = sql_fixture("varchar-boolean_set-text", &[]);
    // Act
    let result = fixture.execute("SELECT v::TEXT AS output FROM (SELECT (true)::VARCHAR(2) AS output UNION ALL SELECT NULL::VARCHAR(2) ORDER BY 1 NULLS LAST) AS q(v) ORDER BY 1 NULLS LAST");
    // Assert
    let expected: Vec<Vec<Value>> = vec![vec![Value::String("tr".into())], vec![Value::Null]];
    assert_eq!(
        result.expect("explicit VARCHAR").rows(),
        expected.as_slice()
    );
}

#[test]
fn should_truncate_float_given_explicit_varchar_when_set_is_converted_to_text() {
    // Arrange
    let fixture = sql_fixture("varchar-float_set-text", &[]);
    // Act
    let result = fixture.execute("SELECT v::TEXT AS output FROM (SELECT (1.2345::FLOAT8)::VARCHAR(4) AS output UNION ALL SELECT NULL::VARCHAR(4) ORDER BY 1 NULLS LAST) AS q(v) ORDER BY 1 NULLS LAST");
    // Assert
    let expected: Vec<Vec<Value>> = vec![vec![Value::String("1.23".into())], vec![Value::Null]];
    assert_eq!(
        result.expect("explicit VARCHAR").rows(),
        expected.as_slice()
    );
}

#[test]
fn should_truncate_time_given_explicit_varchar_when_set_is_converted_to_text() {
    // Arrange
    let fixture = sql_fixture("varchar-time_set-text", &[]);
    // Act
    let result = fixture.execute("SELECT v::TEXT AS output FROM (SELECT ('12:34:56.120000'::TIME)::VARCHAR(7) AS output UNION ALL SELECT NULL::VARCHAR(7) ORDER BY 1 NULLS LAST) AS q(v) ORDER BY 1 NULLS LAST");
    // Assert
    let expected: Vec<Vec<Value>> = vec![vec![Value::String("12:34:5".into())], vec![Value::Null]];
    assert_eq!(
        result.expect("explicit VARCHAR").rows(),
        expected.as_slice()
    );
}

#[test]
fn should_truncate_timestamp_given_explicit_varchar_when_set_is_converted_to_text() {
    // Arrange
    let fixture = sql_fixture("varchar-timestamp_set-text", &[]);
    // Act
    let result = fixture.execute("SELECT v::TEXT AS output FROM (SELECT ('2024-02-29 12:34:56.120000'::TIMESTAMP)::VARCHAR(12) AS output UNION ALL SELECT NULL::VARCHAR(12) ORDER BY 1 NULLS LAST) AS q(v) ORDER BY 1 NULLS LAST");
    // Assert
    let expected: Vec<Vec<Value>> = vec![
        vec![Value::String("2024-02-29 1".into())],
        vec![Value::Null],
    ];
    assert_eq!(
        result.expect("explicit VARCHAR").rows(),
        expected.as_slice()
    );
}
