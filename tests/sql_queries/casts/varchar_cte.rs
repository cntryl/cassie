//! Native explicit VARCHAR casts through query contexts.
use crate::support_sql_fixture::sql_fixture;
use cassie::types::Value;

#[test]
fn should_truncate_ascii_given_explicit_varchar_when_cte_is_converted_to_text() {
    // Arrange
    let fixture = sql_fixture("varchar-ascii_cte-text", &[]);
    // Act
    let result = fixture.execute("SELECT v::TEXT AS output FROM (WITH q(v) AS (VALUES (('abcdef')::VARCHAR(3))) SELECT v AS output FROM q) AS q(v) ORDER BY 1 NULLS LAST");
    // Assert
    let expected: Vec<Vec<Value>> = vec![vec![Value::String("abc".into())]];
    assert_eq!(
        result.expect("explicit VARCHAR").rows(),
        expected.as_slice()
    );
}

#[test]
fn should_truncate_exact_given_explicit_varchar_when_cte_is_converted_to_text() {
    // Arrange
    let fixture = sql_fixture("varchar-exact_cte-text", &[]);
    // Act
    let result = fixture.execute("SELECT v::TEXT AS output FROM (WITH q(v) AS (VALUES (('abc')::VARCHAR(3))) SELECT v AS output FROM q) AS q(v) ORDER BY 1 NULLS LAST");
    // Assert
    let expected: Vec<Vec<Value>> = vec![vec![Value::String("abc".into())]];
    assert_eq!(
        result.expect("explicit VARCHAR").rows(),
        expected.as_slice()
    );
}

#[test]
fn should_truncate_short_given_explicit_varchar_when_cte_is_converted_to_text() {
    // Arrange
    let fixture = sql_fixture("varchar-short_cte-text", &[]);
    // Act
    let result = fixture.execute("SELECT v::TEXT AS output FROM (WITH q(v) AS (VALUES (('a')::VARCHAR(3))) SELECT v AS output FROM q) AS q(v) ORDER BY 1 NULLS LAST");
    // Assert
    let expected: Vec<Vec<Value>> = vec![vec![Value::String("a".into())]];
    assert_eq!(
        result.expect("explicit VARCHAR").rows(),
        expected.as_slice()
    );
}

#[test]
fn should_truncate_empty_given_explicit_varchar_when_cte_is_converted_to_text() {
    // Arrange
    let fixture = sql_fixture("varchar-empty_cte-text", &[]);
    // Act
    let result = fixture.execute("SELECT v::TEXT AS output FROM (WITH q(v) AS (VALUES (('')::VARCHAR(3))) SELECT v AS output FROM q) AS q(v) ORDER BY 1 NULLS LAST");
    // Assert
    let expected: Vec<Vec<Value>> = vec![vec![Value::String(String::new())]];
    assert_eq!(
        result.expect("explicit VARCHAR").rows(),
        expected.as_slice()
    );
}

#[test]
fn should_truncate_spaces_given_explicit_varchar_when_cte_is_converted_to_text() {
    // Arrange
    let fixture = sql_fixture("varchar-spaces_cte-text", &[]);
    // Act
    let result = fixture.execute("SELECT v::TEXT AS output FROM (WITH q(v) AS (VALUES (('a    ')::VARCHAR(3))) SELECT v AS output FROM q) AS q(v) ORDER BY 1 NULLS LAST");
    // Assert
    let expected: Vec<Vec<Value>> = vec![vec![Value::String("a  ".into())]];
    assert_eq!(
        result.expect("explicit VARCHAR").rows(),
        expected.as_slice()
    );
}

#[test]
fn should_truncate_leading_given_explicit_varchar_when_cte_is_converted_to_text() {
    // Arrange
    let fixture = sql_fixture("varchar-leading_cte-text", &[]);
    // Act
    let result = fixture.execute("SELECT v::TEXT AS output FROM (WITH q(v) AS (VALUES (('   abc')::VARCHAR(3))) SELECT v AS output FROM q) AS q(v) ORDER BY 1 NULLS LAST");
    // Assert
    let expected: Vec<Vec<Value>> = vec![vec![Value::String("   ".into())]];
    assert_eq!(
        result.expect("explicit VARCHAR").rows(),
        expected.as_slice()
    );
}

#[test]
fn should_truncate_unicode_given_explicit_varchar_when_cte_is_converted_to_text() {
    // Arrange
    let fixture = sql_fixture("varchar-unicode_cte-text", &[]);
    // Act
    let result = fixture.execute("SELECT v::TEXT AS output FROM (WITH q(v) AS (VALUES (('a🙂文z')::VARCHAR(3))) SELECT v AS output FROM q) AS q(v) ORDER BY 1 NULLS LAST");
    // Assert
    let expected: Vec<Vec<Value>> = vec![vec![Value::String("a🙂文".into())]];
    assert_eq!(
        result.expect("explicit VARCHAR").rows(),
        expected.as_slice()
    );
}

#[test]
fn should_truncate_emoji_given_explicit_varchar_when_cte_is_converted_to_text() {
    // Arrange
    let fixture = sql_fixture("varchar-emoji_cte-text", &[]);
    // Act
    let result = fixture.execute("SELECT v::TEXT AS output FROM (WITH q(v) AS (VALUES (('🙂🙃🙂🙃')::VARCHAR(2))) SELECT v AS output FROM q) AS q(v) ORDER BY 1 NULLS LAST");
    // Assert
    let expected: Vec<Vec<Value>> = vec![vec![Value::String("🙂🙃".into())]];
    assert_eq!(
        result.expect("explicit VARCHAR").rows(),
        expected.as_slice()
    );
}

#[test]
fn should_truncate_combining_given_explicit_varchar_when_cte_is_converted_to_text() {
    // Arrange
    let fixture = sql_fixture("varchar-combining_cte-text", &[]);
    // Act
    let result = fixture.execute("SELECT v::TEXT AS output FROM (WITH q(v) AS (VALUES (('e\u{301}xy')::VARCHAR(2))) SELECT v AS output FROM q) AS q(v) ORDER BY 1 NULLS LAST");
    // Assert
    let expected: Vec<Vec<Value>> = vec![vec![Value::String("e\u{301}".into())]];
    assert_eq!(
        result.expect("explicit VARCHAR").rows(),
        expected.as_slice()
    );
}

#[test]
fn should_truncate_null_given_explicit_varchar_when_cte_is_converted_to_text() {
    // Arrange
    let fixture = sql_fixture("varchar-null_cte-text", &[]);
    // Act
    let result = fixture.execute("SELECT v::TEXT AS output FROM (WITH q(v) AS (VALUES ((NULL)::VARCHAR(3))) SELECT v AS output FROM q) AS q(v) ORDER BY 1 NULLS LAST");
    // Assert
    let expected: Vec<Vec<Value>> = vec![vec![Value::Null]];
    assert_eq!(
        result.expect("explicit VARCHAR").rows(),
        expected.as_slice()
    );
}

#[test]
fn should_truncate_char_given_explicit_varchar_when_cte_is_converted_to_text() {
    // Arrange
    let fixture = sql_fixture("varchar-char_cte-text", &[]);
    // Act
    let result = fixture.execute("SELECT v::TEXT AS output FROM (WITH q(v) AS (VALUES (('a'::CHAR(6))::VARCHAR(3))) SELECT v AS output FROM q) AS q(v) ORDER BY 1 NULLS LAST");
    // Assert
    let expected: Vec<Vec<Value>> = vec![vec![Value::String("a".into())]];
    assert_eq!(
        result.expect("explicit VARCHAR").rows(),
        expected.as_slice()
    );
}

#[test]
fn should_truncate_char_long_given_explicit_varchar_when_cte_is_converted_to_text() {
    // Arrange
    let fixture = sql_fixture("varchar-char_long_cte-text", &[]);
    // Act
    let result = fixture.execute("SELECT v::TEXT AS output FROM (WITH q(v) AS (VALUES (('abcdef'::CHAR(8))::VARCHAR(3))) SELECT v AS output FROM q) AS q(v) ORDER BY 1 NULLS LAST");
    // Assert
    let expected: Vec<Vec<Value>> = vec![vec![Value::String("abc".into())]];
    assert_eq!(
        result.expect("explicit VARCHAR").rows(),
        expected.as_slice()
    );
}

#[test]
fn should_truncate_integer_given_explicit_varchar_when_cte_is_converted_to_text() {
    // Arrange
    let fixture = sql_fixture("varchar-integer_cte-text", &[]);
    // Act
    let result = fixture.execute("SELECT v::TEXT AS output FROM (WITH q(v) AS (VALUES ((123456::BIGINT)::VARCHAR(3))) SELECT v AS output FROM q) AS q(v) ORDER BY 1 NULLS LAST");
    // Assert
    let expected: Vec<Vec<Value>> = vec![vec![Value::String("123".into())]];
    assert_eq!(
        result.expect("explicit VARCHAR").rows(),
        expected.as_slice()
    );
}

#[test]
fn should_truncate_negative_given_explicit_varchar_when_cte_is_converted_to_text() {
    // Arrange
    let fixture = sql_fixture("varchar-negative_cte-text", &[]);
    // Act
    let result = fixture.execute("SELECT v::TEXT AS output FROM (WITH q(v) AS (VALUES (((-123456)::BIGINT)::VARCHAR(4))) SELECT v AS output FROM q) AS q(v) ORDER BY 1 NULLS LAST");
    // Assert
    let expected: Vec<Vec<Value>> = vec![vec![Value::String("-123".into())]];
    assert_eq!(
        result.expect("explicit VARCHAR").rows(),
        expected.as_slice()
    );
}

#[test]
fn should_truncate_boolean_given_explicit_varchar_when_cte_is_converted_to_text() {
    // Arrange
    let fixture = sql_fixture("varchar-boolean_cte-text", &[]);
    // Act
    let result = fixture.execute("SELECT v::TEXT AS output FROM (WITH q(v) AS (VALUES ((true)::VARCHAR(2))) SELECT v AS output FROM q) AS q(v) ORDER BY 1 NULLS LAST");
    // Assert
    let expected: Vec<Vec<Value>> = vec![vec![Value::String("tr".into())]];
    assert_eq!(
        result.expect("explicit VARCHAR").rows(),
        expected.as_slice()
    );
}

#[test]
fn should_truncate_float_given_explicit_varchar_when_cte_is_converted_to_text() {
    // Arrange
    let fixture = sql_fixture("varchar-float_cte-text", &[]);
    // Act
    let result = fixture.execute("SELECT v::TEXT AS output FROM (WITH q(v) AS (VALUES ((1.2345::FLOAT8)::VARCHAR(4))) SELECT v AS output FROM q) AS q(v) ORDER BY 1 NULLS LAST");
    // Assert
    let expected: Vec<Vec<Value>> = vec![vec![Value::String("1.23".into())]];
    assert_eq!(
        result.expect("explicit VARCHAR").rows(),
        expected.as_slice()
    );
}

#[test]
fn should_truncate_time_given_explicit_varchar_when_cte_is_converted_to_text() {
    // Arrange
    let fixture = sql_fixture("varchar-time_cte-text", &[]);
    // Act
    let result = fixture.execute("SELECT v::TEXT AS output FROM (WITH q(v) AS (VALUES (('12:34:56.120000'::TIME)::VARCHAR(7))) SELECT v AS output FROM q) AS q(v) ORDER BY 1 NULLS LAST");
    // Assert
    let expected: Vec<Vec<Value>> = vec![vec![Value::String("12:34:5".into())]];
    assert_eq!(
        result.expect("explicit VARCHAR").rows(),
        expected.as_slice()
    );
}

#[test]
fn should_truncate_timestamp_given_explicit_varchar_when_cte_is_converted_to_text() {
    // Arrange
    let fixture = sql_fixture("varchar-timestamp_cte-text", &[]);
    // Act
    let result = fixture.execute("SELECT v::TEXT AS output FROM (WITH q(v) AS (VALUES (('2024-02-29 12:34:56.120000'::TIMESTAMP)::VARCHAR(12))) SELECT v AS output FROM q) AS q(v) ORDER BY 1 NULLS LAST");
    // Assert
    let expected: Vec<Vec<Value>> = vec![vec![Value::String("2024-02-29 1".into())]];
    assert_eq!(
        result.expect("explicit VARCHAR").rows(),
        expected.as_slice()
    );
}
