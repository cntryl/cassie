//! Native explicit VARCHAR casts through query contexts.
use crate::support_sql_fixture::sql_fixture;
use cassie::types::Value;

#[test]
fn should_truncate_ascii_given_explicit_varchar_when_derived_is_converted_to_text() {
    // Arrange
    let fixture = sql_fixture("varchar-ascii_derived-text", &[]);
    // Act
    let result = fixture.execute("SELECT v::TEXT AS output FROM (SELECT v AS output FROM (VALUES (('abcdef')::VARCHAR(3))) AS q(v)) AS q(v) ORDER BY 1 NULLS LAST");
    // Assert
    let expected: Vec<Vec<Value>> = vec![vec![Value::String("abc".into())]];
    assert_eq!(
        result.expect("explicit VARCHAR").rows(),
        expected.as_slice()
    );
}

#[test]
fn should_truncate_exact_given_explicit_varchar_when_derived_is_converted_to_text() {
    // Arrange
    let fixture = sql_fixture("varchar-exact_derived-text", &[]);
    // Act
    let result = fixture.execute("SELECT v::TEXT AS output FROM (SELECT v AS output FROM (VALUES (('abc')::VARCHAR(3))) AS q(v)) AS q(v) ORDER BY 1 NULLS LAST");
    // Assert
    let expected: Vec<Vec<Value>> = vec![vec![Value::String("abc".into())]];
    assert_eq!(
        result.expect("explicit VARCHAR").rows(),
        expected.as_slice()
    );
}

#[test]
fn should_truncate_short_given_explicit_varchar_when_derived_is_converted_to_text() {
    // Arrange
    let fixture = sql_fixture("varchar-short_derived-text", &[]);
    // Act
    let result = fixture.execute("SELECT v::TEXT AS output FROM (SELECT v AS output FROM (VALUES (('a')::VARCHAR(3))) AS q(v)) AS q(v) ORDER BY 1 NULLS LAST");
    // Assert
    let expected: Vec<Vec<Value>> = vec![vec![Value::String("a".into())]];
    assert_eq!(
        result.expect("explicit VARCHAR").rows(),
        expected.as_slice()
    );
}

#[test]
fn should_truncate_empty_given_explicit_varchar_when_derived_is_converted_to_text() {
    // Arrange
    let fixture = sql_fixture("varchar-empty_derived-text", &[]);
    // Act
    let result = fixture.execute("SELECT v::TEXT AS output FROM (SELECT v AS output FROM (VALUES (('')::VARCHAR(3))) AS q(v)) AS q(v) ORDER BY 1 NULLS LAST");
    // Assert
    let expected: Vec<Vec<Value>> = vec![vec![Value::String(String::new())]];
    assert_eq!(
        result.expect("explicit VARCHAR").rows(),
        expected.as_slice()
    );
}

#[test]
fn should_truncate_spaces_given_explicit_varchar_when_derived_is_converted_to_text() {
    // Arrange
    let fixture = sql_fixture("varchar-spaces_derived-text", &[]);
    // Act
    let result = fixture.execute("SELECT v::TEXT AS output FROM (SELECT v AS output FROM (VALUES (('a    ')::VARCHAR(3))) AS q(v)) AS q(v) ORDER BY 1 NULLS LAST");
    // Assert
    let expected: Vec<Vec<Value>> = vec![vec![Value::String("a  ".into())]];
    assert_eq!(
        result.expect("explicit VARCHAR").rows(),
        expected.as_slice()
    );
}

#[test]
fn should_truncate_leading_given_explicit_varchar_when_derived_is_converted_to_text() {
    // Arrange
    let fixture = sql_fixture("varchar-leading_derived-text", &[]);
    // Act
    let result = fixture.execute("SELECT v::TEXT AS output FROM (SELECT v AS output FROM (VALUES (('   abc')::VARCHAR(3))) AS q(v)) AS q(v) ORDER BY 1 NULLS LAST");
    // Assert
    let expected: Vec<Vec<Value>> = vec![vec![Value::String("   ".into())]];
    assert_eq!(
        result.expect("explicit VARCHAR").rows(),
        expected.as_slice()
    );
}

#[test]
fn should_truncate_unicode_given_explicit_varchar_when_derived_is_converted_to_text() {
    // Arrange
    let fixture = sql_fixture("varchar-unicode_derived-text", &[]);
    // Act
    let result = fixture.execute("SELECT v::TEXT AS output FROM (SELECT v AS output FROM (VALUES (('a🙂文z')::VARCHAR(3))) AS q(v)) AS q(v) ORDER BY 1 NULLS LAST");
    // Assert
    let expected: Vec<Vec<Value>> = vec![vec![Value::String("a🙂文".into())]];
    assert_eq!(
        result.expect("explicit VARCHAR").rows(),
        expected.as_slice()
    );
}

#[test]
fn should_truncate_emoji_given_explicit_varchar_when_derived_is_converted_to_text() {
    // Arrange
    let fixture = sql_fixture("varchar-emoji_derived-text", &[]);
    // Act
    let result = fixture.execute("SELECT v::TEXT AS output FROM (SELECT v AS output FROM (VALUES (('🙂🙃🙂🙃')::VARCHAR(2))) AS q(v)) AS q(v) ORDER BY 1 NULLS LAST");
    // Assert
    let expected: Vec<Vec<Value>> = vec![vec![Value::String("🙂🙃".into())]];
    assert_eq!(
        result.expect("explicit VARCHAR").rows(),
        expected.as_slice()
    );
}

#[test]
fn should_truncate_combining_given_explicit_varchar_when_derived_is_converted_to_text() {
    // Arrange
    let fixture = sql_fixture("varchar-combining_derived-text", &[]);
    // Act
    let result = fixture.execute("SELECT v::TEXT AS output FROM (SELECT v AS output FROM (VALUES (('e\u{301}xy')::VARCHAR(2))) AS q(v)) AS q(v) ORDER BY 1 NULLS LAST");
    // Assert
    let expected: Vec<Vec<Value>> = vec![vec![Value::String("e\u{301}".into())]];
    assert_eq!(
        result.expect("explicit VARCHAR").rows(),
        expected.as_slice()
    );
}

#[test]
fn should_truncate_null_given_explicit_varchar_when_derived_is_converted_to_text() {
    // Arrange
    let fixture = sql_fixture("varchar-null_derived-text", &[]);
    // Act
    let result = fixture.execute("SELECT v::TEXT AS output FROM (SELECT v AS output FROM (VALUES ((NULL)::VARCHAR(3))) AS q(v)) AS q(v) ORDER BY 1 NULLS LAST");
    // Assert
    let expected: Vec<Vec<Value>> = vec![vec![Value::Null]];
    assert_eq!(
        result.expect("explicit VARCHAR").rows(),
        expected.as_slice()
    );
}

#[test]
fn should_truncate_char_given_explicit_varchar_when_derived_is_converted_to_text() {
    // Arrange
    let fixture = sql_fixture("varchar-char_derived-text", &[]);
    // Act
    let result = fixture.execute("SELECT v::TEXT AS output FROM (SELECT v AS output FROM (VALUES (('a'::CHAR(6))::VARCHAR(3))) AS q(v)) AS q(v) ORDER BY 1 NULLS LAST");
    // Assert
    let expected: Vec<Vec<Value>> = vec![vec![Value::String("a".into())]];
    assert_eq!(
        result.expect("explicit VARCHAR").rows(),
        expected.as_slice()
    );
}

#[test]
fn should_truncate_char_long_given_explicit_varchar_when_derived_is_converted_to_text() {
    // Arrange
    let fixture = sql_fixture("varchar-char_long_derived-text", &[]);
    // Act
    let result = fixture.execute("SELECT v::TEXT AS output FROM (SELECT v AS output FROM (VALUES (('abcdef'::CHAR(8))::VARCHAR(3))) AS q(v)) AS q(v) ORDER BY 1 NULLS LAST");
    // Assert
    let expected: Vec<Vec<Value>> = vec![vec![Value::String("abc".into())]];
    assert_eq!(
        result.expect("explicit VARCHAR").rows(),
        expected.as_slice()
    );
}

#[test]
fn should_truncate_integer_given_explicit_varchar_when_derived_is_converted_to_text() {
    // Arrange
    let fixture = sql_fixture("varchar-integer_derived-text", &[]);
    // Act
    let result = fixture.execute("SELECT v::TEXT AS output FROM (SELECT v AS output FROM (VALUES ((123456::BIGINT)::VARCHAR(3))) AS q(v)) AS q(v) ORDER BY 1 NULLS LAST");
    // Assert
    let expected: Vec<Vec<Value>> = vec![vec![Value::String("123".into())]];
    assert_eq!(
        result.expect("explicit VARCHAR").rows(),
        expected.as_slice()
    );
}

#[test]
fn should_truncate_negative_given_explicit_varchar_when_derived_is_converted_to_text() {
    // Arrange
    let fixture = sql_fixture("varchar-negative_derived-text", &[]);
    // Act
    let result = fixture.execute("SELECT v::TEXT AS output FROM (SELECT v AS output FROM (VALUES (((-123456)::BIGINT)::VARCHAR(4))) AS q(v)) AS q(v) ORDER BY 1 NULLS LAST");
    // Assert
    let expected: Vec<Vec<Value>> = vec![vec![Value::String("-123".into())]];
    assert_eq!(
        result.expect("explicit VARCHAR").rows(),
        expected.as_slice()
    );
}

#[test]
fn should_truncate_boolean_given_explicit_varchar_when_derived_is_converted_to_text() {
    // Arrange
    let fixture = sql_fixture("varchar-boolean_derived-text", &[]);
    // Act
    let result = fixture.execute("SELECT v::TEXT AS output FROM (SELECT v AS output FROM (VALUES ((true)::VARCHAR(2))) AS q(v)) AS q(v) ORDER BY 1 NULLS LAST");
    // Assert
    let expected: Vec<Vec<Value>> = vec![vec![Value::String("tr".into())]];
    assert_eq!(
        result.expect("explicit VARCHAR").rows(),
        expected.as_slice()
    );
}

#[test]
fn should_truncate_float_given_explicit_varchar_when_derived_is_converted_to_text() {
    // Arrange
    let fixture = sql_fixture("varchar-float_derived-text", &[]);
    // Act
    let result = fixture.execute("SELECT v::TEXT AS output FROM (SELECT v AS output FROM (VALUES ((1.2345::FLOAT8)::VARCHAR(4))) AS q(v)) AS q(v) ORDER BY 1 NULLS LAST");
    // Assert
    let expected: Vec<Vec<Value>> = vec![vec![Value::String("1.23".into())]];
    assert_eq!(
        result.expect("explicit VARCHAR").rows(),
        expected.as_slice()
    );
}

#[test]
fn should_truncate_time_given_explicit_varchar_when_derived_is_converted_to_text() {
    // Arrange
    let fixture = sql_fixture("varchar-time_derived-text", &[]);
    // Act
    let result = fixture.execute("SELECT v::TEXT AS output FROM (SELECT v AS output FROM (VALUES (('12:34:56.120000'::TIME)::VARCHAR(7))) AS q(v)) AS q(v) ORDER BY 1 NULLS LAST");
    // Assert
    let expected: Vec<Vec<Value>> = vec![vec![Value::String("12:34:5".into())]];
    assert_eq!(
        result.expect("explicit VARCHAR").rows(),
        expected.as_slice()
    );
}

#[test]
fn should_truncate_timestamp_given_explicit_varchar_when_derived_is_converted_to_text() {
    // Arrange
    let fixture = sql_fixture("varchar-timestamp_derived-text", &[]);
    // Act
    let result = fixture.execute("SELECT v::TEXT AS output FROM (SELECT v AS output FROM (VALUES (('2024-02-29 12:34:56.120000'::TIMESTAMP)::VARCHAR(12))) AS q(v)) AS q(v) ORDER BY 1 NULLS LAST");
    // Assert
    let expected: Vec<Vec<Value>> = vec![vec![Value::String("2024-02-29 1".into())]];
    assert_eq!(
        result.expect("explicit VARCHAR").rows(),
        expected.as_slice()
    );
}
