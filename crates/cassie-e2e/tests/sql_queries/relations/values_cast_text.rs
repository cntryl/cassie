//! VALUES source-aware text conversion from native PostgreSQL witnesses.
use crate::support_sql_fixture::sql_fixture;
use cassie::types::Value;

#[test]
fn should_preserve_date_repeat_given_values_sources_when_output_is_cast_to_text() {
    // Arrange
    let fixture = sql_fixture("valuescasts-text-date_repeat", &[]);
    // Act
    let result = fixture.execute("SELECT v::TEXT AS output FROM (VALUES ('2024-02-29'::DATE),('2024-02-29'::DATE),('2000-01-01'::DATE)) AS q(v) ORDER BY v NULLS LAST");
    // Assert
    let expected: Vec<Vec<Value>> = vec![
        vec![Value::String("2000-01-01".into())],
        vec![Value::String("2024-02-29".into())],
        vec![Value::String("2024-02-29".into())],
    ];
    assert_eq!(
        result.expect("VALUES source identity").rows(),
        expected.as_slice()
    );
}

#[test]
fn should_preserve_date_null_tail_given_values_sources_when_output_is_cast_to_text() {
    // Arrange
    let fixture = sql_fixture("valuescasts-text-date_null_tail", &[]);
    // Act
    let result = fixture.execute("SELECT v::TEXT AS output FROM (VALUES ('2024-02-29'::DATE),(NULL),('2000-01-01'::DATE)) AS q(v) ORDER BY v NULLS LAST");
    // Assert
    let expected: Vec<Vec<Value>> = vec![
        vec![Value::String("2000-01-01".into())],
        vec![Value::String("2024-02-29".into())],
        vec![Value::Null],
    ];
    assert_eq!(
        result.expect("VALUES source identity").rows(),
        expected.as_slice()
    );
}

#[test]
fn should_preserve_date_unknown_head_given_values_sources_when_output_is_cast_to_text() {
    // Arrange
    let fixture = sql_fixture("valuescasts-text-date_unknown_head", &[]);
    // Act
    let result = fixture.execute("SELECT v::TEXT AS output FROM (VALUES ('2000-01-01'),('2024-02-29'::DATE)) AS q(v) ORDER BY v NULLS LAST");
    // Assert
    let expected: Vec<Vec<Value>> = vec![
        vec![Value::String("2000-01-01".into())],
        vec![Value::String("2024-02-29".into())],
    ];
    assert_eq!(
        result.expect("VALUES source identity").rows(),
        expected.as_slice()
    );
}

#[test]
fn should_preserve_date_unknown_tail_given_values_sources_when_output_is_cast_to_text() {
    // Arrange
    let fixture = sql_fixture("valuescasts-text-date_unknown_tail", &[]);
    // Act
    let result = fixture.execute("SELECT v::TEXT AS output FROM (VALUES ('2024-02-29'::DATE),('2000-01-01')) AS q(v) ORDER BY v NULLS LAST");
    // Assert
    let expected: Vec<Vec<Value>> = vec![
        vec![Value::String("2000-01-01".into())],
        vec![Value::String("2024-02-29".into())],
    ];
    assert_eq!(
        result.expect("VALUES source identity").rows(),
        expected.as_slice()
    );
}

#[test]
fn should_preserve_date_typed_null_given_values_sources_when_output_is_cast_to_text() {
    // Arrange
    let fixture = sql_fixture("valuescasts-text-date_typed_null", &[]);
    // Act
    let result = fixture.execute("SELECT v::TEXT AS output FROM (VALUES (NULL::DATE),('2024-02-29'::DATE)) AS q(v) ORDER BY v NULLS LAST");
    // Assert
    let expected: Vec<Vec<Value>> =
        vec![vec![Value::String("2024-02-29".into())], vec![Value::Null]];
    assert_eq!(
        result.expect("VALUES source identity").rows(),
        expected.as_slice()
    );
}

#[test]
fn should_preserve_date_mixed_width_given_values_sources_when_output_is_cast_to_text() {
    // Arrange
    let fixture = sql_fixture("valuescasts-text-date_mixed_width", &[]);
    // Act
    let result = fixture.execute("SELECT v::TEXT AS output FROM (VALUES ('2024-02-29'::DATE),('2000-01-01'::DATE)) AS q(v) ORDER BY v NULLS LAST");
    // Assert
    let expected: Vec<Vec<Value>> = vec![
        vec![Value::String("2000-01-01".into())],
        vec![Value::String("2024-02-29".into())],
    ];
    assert_eq!(
        result.expect("VALUES source identity").rows(),
        expected.as_slice()
    );
}

#[test]
fn should_preserve_time_repeat_given_values_sources_when_output_is_cast_to_text() {
    // Arrange
    let fixture = sql_fixture("valuescasts-text-time_repeat", &[]);
    // Act
    let result = fixture.execute("SELECT v::TEXT AS output FROM (VALUES ('12:34:56.120000'::TIME),('12:34:56.120000'::TIME),('00:00:00'::TIME)) AS q(v) ORDER BY v NULLS LAST");
    // Assert
    let expected: Vec<Vec<Value>> = vec![
        vec![Value::String("00:00:00".into())],
        vec![Value::String("12:34:56.12".into())],
        vec![Value::String("12:34:56.12".into())],
    ];
    assert_eq!(
        result.expect("VALUES source identity").rows(),
        expected.as_slice()
    );
}

#[test]
fn should_preserve_time_null_tail_given_values_sources_when_output_is_cast_to_text() {
    // Arrange
    let fixture = sql_fixture("valuescasts-text-time_null_tail", &[]);
    // Act
    let result = fixture.execute("SELECT v::TEXT AS output FROM (VALUES ('12:34:56.120000'::TIME),(NULL),('00:00:00'::TIME)) AS q(v) ORDER BY v NULLS LAST");
    // Assert
    let expected: Vec<Vec<Value>> = vec![
        vec![Value::String("00:00:00".into())],
        vec![Value::String("12:34:56.12".into())],
        vec![Value::Null],
    ];
    assert_eq!(
        result.expect("VALUES source identity").rows(),
        expected.as_slice()
    );
}

#[test]
fn should_preserve_time_unknown_head_given_values_sources_when_output_is_cast_to_text() {
    // Arrange
    let fixture = sql_fixture("valuescasts-text-time_unknown_head", &[]);
    // Act
    let result = fixture.execute("SELECT v::TEXT AS output FROM (VALUES ('00:00:00'),('12:34:56.120000'::TIME)) AS q(v) ORDER BY v NULLS LAST");
    // Assert
    let expected: Vec<Vec<Value>> = vec![
        vec![Value::String("00:00:00".into())],
        vec![Value::String("12:34:56.12".into())],
    ];
    assert_eq!(
        result.expect("VALUES source identity").rows(),
        expected.as_slice()
    );
}

#[test]
fn should_preserve_time_unknown_tail_given_values_sources_when_output_is_cast_to_text() {
    // Arrange
    let fixture = sql_fixture("valuescasts-text-time_unknown_tail", &[]);
    // Act
    let result = fixture.execute("SELECT v::TEXT AS output FROM (VALUES ('12:34:56.120000'::TIME),('00:00:00')) AS q(v) ORDER BY v NULLS LAST");
    // Assert
    let expected: Vec<Vec<Value>> = vec![
        vec![Value::String("00:00:00".into())],
        vec![Value::String("12:34:56.12".into())],
    ];
    assert_eq!(
        result.expect("VALUES source identity").rows(),
        expected.as_slice()
    );
}

#[test]
fn should_preserve_time_typed_null_given_values_sources_when_output_is_cast_to_text() {
    // Arrange
    let fixture = sql_fixture("valuescasts-text-time_typed_null", &[]);
    // Act
    let result = fixture.execute("SELECT v::TEXT AS output FROM (VALUES (NULL::TIME),('12:34:56.120000'::TIME)) AS q(v) ORDER BY v NULLS LAST");
    // Assert
    let expected: Vec<Vec<Value>> =
        vec![vec![Value::String("12:34:56.12".into())], vec![Value::Null]];
    assert_eq!(
        result.expect("VALUES source identity").rows(),
        expected.as_slice()
    );
}

#[test]
fn should_preserve_time_mixed_width_given_values_sources_when_output_is_cast_to_text() {
    // Arrange
    let fixture = sql_fixture("valuescasts-text-time_mixed_width", &[]);
    // Act
    let result = fixture.execute("SELECT v::TEXT AS output FROM (VALUES ('12:34:56.120000'::TIME),('00:00:00'::TIME)) AS q(v) ORDER BY v NULLS LAST");
    // Assert
    let expected: Vec<Vec<Value>> = vec![
        vec![Value::String("00:00:00".into())],
        vec![Value::String("12:34:56.12".into())],
    ];
    assert_eq!(
        result.expect("VALUES source identity").rows(),
        expected.as_slice()
    );
}

#[test]
fn should_preserve_timestamp_repeat_given_values_sources_when_output_is_cast_to_text() {
    // Arrange
    let fixture = sql_fixture("valuescasts-text-timestamp_repeat", &[]);
    // Act
    let result = fixture.execute("SELECT v::TEXT AS output FROM (VALUES ('2024-02-29 12:34:56.120000'::TIMESTAMP),('2024-02-29 12:34:56.120000'::TIMESTAMP),('2000-01-01 00:00:00'::TIMESTAMP)) AS q(v) ORDER BY v NULLS LAST");
    // Assert
    let expected: Vec<Vec<Value>> = vec![
        vec![Value::String("2000-01-01 00:00:00".into())],
        vec![Value::String("2024-02-29 12:34:56.12".into())],
        vec![Value::String("2024-02-29 12:34:56.12".into())],
    ];
    assert_eq!(
        result.expect("VALUES source identity").rows(),
        expected.as_slice()
    );
}

#[test]
fn should_preserve_timestamp_null_tail_given_values_sources_when_output_is_cast_to_text() {
    // Arrange
    let fixture = sql_fixture("valuescasts-text-timestamp_null_tail", &[]);
    // Act
    let result = fixture.execute("SELECT v::TEXT AS output FROM (VALUES ('2024-02-29 12:34:56.120000'::TIMESTAMP),(NULL),('2000-01-01 00:00:00'::TIMESTAMP)) AS q(v) ORDER BY v NULLS LAST");
    // Assert
    let expected: Vec<Vec<Value>> = vec![
        vec![Value::String("2000-01-01 00:00:00".into())],
        vec![Value::String("2024-02-29 12:34:56.12".into())],
        vec![Value::Null],
    ];
    assert_eq!(
        result.expect("VALUES source identity").rows(),
        expected.as_slice()
    );
}

#[test]
fn should_preserve_timestamp_unknown_head_given_values_sources_when_output_is_cast_to_text() {
    // Arrange
    let fixture = sql_fixture("valuescasts-text-timestamp_unknown_head", &[]);
    // Act
    let result = fixture.execute("SELECT v::TEXT AS output FROM (VALUES ('2000-01-01 00:00:00'),('2024-02-29 12:34:56.120000'::TIMESTAMP)) AS q(v) ORDER BY v NULLS LAST");
    // Assert
    let expected: Vec<Vec<Value>> = vec![
        vec![Value::String("2000-01-01 00:00:00".into())],
        vec![Value::String("2024-02-29 12:34:56.12".into())],
    ];
    assert_eq!(
        result.expect("VALUES source identity").rows(),
        expected.as_slice()
    );
}

#[test]
fn should_preserve_timestamp_unknown_tail_given_values_sources_when_output_is_cast_to_text() {
    // Arrange
    let fixture = sql_fixture("valuescasts-text-timestamp_unknown_tail", &[]);
    // Act
    let result = fixture.execute("SELECT v::TEXT AS output FROM (VALUES ('2024-02-29 12:34:56.120000'::TIMESTAMP),('2000-01-01 00:00:00')) AS q(v) ORDER BY v NULLS LAST");
    // Assert
    let expected: Vec<Vec<Value>> = vec![
        vec![Value::String("2000-01-01 00:00:00".into())],
        vec![Value::String("2024-02-29 12:34:56.12".into())],
    ];
    assert_eq!(
        result.expect("VALUES source identity").rows(),
        expected.as_slice()
    );
}

#[test]
fn should_preserve_timestamp_typed_null_given_values_sources_when_output_is_cast_to_text() {
    // Arrange
    let fixture = sql_fixture("valuescasts-text-timestamp_typed_null", &[]);
    // Act
    let result = fixture.execute("SELECT v::TEXT AS output FROM (VALUES (NULL::TIMESTAMP),('2024-02-29 12:34:56.120000'::TIMESTAMP)) AS q(v) ORDER BY v NULLS LAST");
    // Assert
    let expected: Vec<Vec<Value>> = vec![
        vec![Value::String("2024-02-29 12:34:56.12".into())],
        vec![Value::Null],
    ];
    assert_eq!(
        result.expect("VALUES source identity").rows(),
        expected.as_slice()
    );
}

#[test]
fn should_preserve_timestamp_mixed_width_given_values_sources_when_output_is_cast_to_text() {
    // Arrange
    let fixture = sql_fixture("valuescasts-text-timestamp_mixed_width", &[]);
    // Act
    let result = fixture.execute("SELECT v::TEXT AS output FROM (VALUES ('2024-02-29 12:34:56.120000'::TIMESTAMP),('2000-01-01 00:00:00'::TIMESTAMP)) AS q(v) ORDER BY v NULLS LAST");
    // Assert
    let expected: Vec<Vec<Value>> = vec![
        vec![Value::String("2000-01-01 00:00:00".into())],
        vec![Value::String("2024-02-29 12:34:56.12".into())],
    ];
    assert_eq!(
        result.expect("VALUES source identity").rows(),
        expected.as_slice()
    );
}

#[test]
fn should_preserve_uuid_repeat_given_values_sources_when_output_is_cast_to_text() {
    // Arrange
    let fixture = sql_fixture("valuescasts-text-uuid_repeat", &[]);
    // Act
    let result = fixture.execute("SELECT v::TEXT AS output FROM (VALUES ('A0EEBC99-9C0B-4EF8-BB6D-6BB9BD380A11'::UUID),('A0EEBC99-9C0B-4EF8-BB6D-6BB9BD380A11'::UUID),('00000000-0000-0000-0000-000000000001'::UUID)) AS q(v) ORDER BY v NULLS LAST");
    // Assert
    let expected: Vec<Vec<Value>> = vec![
        vec![Value::String("00000000-0000-0000-0000-000000000001".into())],
        vec![Value::String("a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11".into())],
        vec![Value::String("a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11".into())],
    ];
    assert_eq!(
        result.expect("VALUES source identity").rows(),
        expected.as_slice()
    );
}

#[test]
fn should_preserve_uuid_null_tail_given_values_sources_when_output_is_cast_to_text() {
    // Arrange
    let fixture = sql_fixture("valuescasts-text-uuid_null_tail", &[]);
    // Act
    let result = fixture.execute("SELECT v::TEXT AS output FROM (VALUES ('A0EEBC99-9C0B-4EF8-BB6D-6BB9BD380A11'::UUID),(NULL),('00000000-0000-0000-0000-000000000001'::UUID)) AS q(v) ORDER BY v NULLS LAST");
    // Assert
    let expected: Vec<Vec<Value>> = vec![
        vec![Value::String("00000000-0000-0000-0000-000000000001".into())],
        vec![Value::String("a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11".into())],
        vec![Value::Null],
    ];
    assert_eq!(
        result.expect("VALUES source identity").rows(),
        expected.as_slice()
    );
}

#[test]
fn should_preserve_uuid_unknown_head_given_values_sources_when_output_is_cast_to_text() {
    // Arrange
    let fixture = sql_fixture("valuescasts-text-uuid_unknown_head", &[]);
    // Act
    let result = fixture.execute("SELECT v::TEXT AS output FROM (VALUES ('00000000-0000-0000-0000-000000000001'),('A0EEBC99-9C0B-4EF8-BB6D-6BB9BD380A11'::UUID)) AS q(v) ORDER BY v NULLS LAST");
    // Assert
    let expected: Vec<Vec<Value>> = vec![
        vec![Value::String("00000000-0000-0000-0000-000000000001".into())],
        vec![Value::String("a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11".into())],
    ];
    assert_eq!(
        result.expect("VALUES source identity").rows(),
        expected.as_slice()
    );
}

#[test]
fn should_preserve_uuid_unknown_tail_given_values_sources_when_output_is_cast_to_text() {
    // Arrange
    let fixture = sql_fixture("valuescasts-text-uuid_unknown_tail", &[]);
    // Act
    let result = fixture.execute("SELECT v::TEXT AS output FROM (VALUES ('A0EEBC99-9C0B-4EF8-BB6D-6BB9BD380A11'::UUID),('00000000-0000-0000-0000-000000000001')) AS q(v) ORDER BY v NULLS LAST");
    // Assert
    let expected: Vec<Vec<Value>> = vec![
        vec![Value::String("00000000-0000-0000-0000-000000000001".into())],
        vec![Value::String("a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11".into())],
    ];
    assert_eq!(
        result.expect("VALUES source identity").rows(),
        expected.as_slice()
    );
}

#[test]
fn should_preserve_uuid_typed_null_given_values_sources_when_output_is_cast_to_text() {
    // Arrange
    let fixture = sql_fixture("valuescasts-text-uuid_typed_null", &[]);
    // Act
    let result = fixture.execute("SELECT v::TEXT AS output FROM (VALUES (NULL::UUID),('A0EEBC99-9C0B-4EF8-BB6D-6BB9BD380A11'::UUID)) AS q(v) ORDER BY v NULLS LAST");
    // Assert
    let expected: Vec<Vec<Value>> = vec![
        vec![Value::String("a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11".into())],
        vec![Value::Null],
    ];
    assert_eq!(
        result.expect("VALUES source identity").rows(),
        expected.as_slice()
    );
}

#[test]
fn should_preserve_uuid_mixed_width_given_values_sources_when_output_is_cast_to_text() {
    // Arrange
    let fixture = sql_fixture("valuescasts-text-uuid_mixed_width", &[]);
    // Act
    let result = fixture.execute("SELECT v::TEXT AS output FROM (VALUES ('A0EEBC99-9C0B-4EF8-BB6D-6BB9BD380A11'::UUID),('00000000-0000-0000-0000-000000000001'::UUID)) AS q(v) ORDER BY v NULLS LAST");
    // Assert
    let expected: Vec<Vec<Value>> = vec![
        vec![Value::String("00000000-0000-0000-0000-000000000001".into())],
        vec![Value::String("a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11".into())],
    ];
    assert_eq!(
        result.expect("VALUES source identity").rows(),
        expected.as_slice()
    );
}

#[test]
fn should_preserve_bytea_repeat_given_values_sources_when_output_is_cast_to_text() {
    // Arrange
    let fixture = sql_fixture("valuescasts-text-bytea_repeat", &[]);
    // Act
    let result = fixture.execute("SELECT v::TEXT AS output FROM (VALUES ('\\x00Abff'::BYTEA),('\\x00Abff'::BYTEA),('\\x0102'::BYTEA)) AS q(v) ORDER BY v NULLS LAST");
    // Assert
    let expected: Vec<Vec<Value>> = vec![
        vec![Value::String("\\x00abff".into())],
        vec![Value::String("\\x00abff".into())],
        vec![Value::String("\\x0102".into())],
    ];
    assert_eq!(
        result.expect("VALUES source identity").rows(),
        expected.as_slice()
    );
}

#[test]
fn should_preserve_bytea_null_tail_given_values_sources_when_output_is_cast_to_text() {
    // Arrange
    let fixture = sql_fixture("valuescasts-text-bytea_null_tail", &[]);
    // Act
    let result = fixture.execute("SELECT v::TEXT AS output FROM (VALUES ('\\x00Abff'::BYTEA),(NULL),('\\x0102'::BYTEA)) AS q(v) ORDER BY v NULLS LAST");
    // Assert
    let expected: Vec<Vec<Value>> = vec![
        vec![Value::String("\\x00abff".into())],
        vec![Value::String("\\x0102".into())],
        vec![Value::Null],
    ];
    assert_eq!(
        result.expect("VALUES source identity").rows(),
        expected.as_slice()
    );
}

#[test]
fn should_preserve_bytea_unknown_head_given_values_sources_when_output_is_cast_to_text() {
    // Arrange
    let fixture = sql_fixture("valuescasts-text-bytea_unknown_head", &[]);
    // Act
    let result = fixture.execute("SELECT v::TEXT AS output FROM (VALUES ('\\x0102'),('\\x00Abff'::BYTEA)) AS q(v) ORDER BY v NULLS LAST");
    // Assert
    let expected: Vec<Vec<Value>> = vec![
        vec![Value::String("\\x00abff".into())],
        vec![Value::String("\\x0102".into())],
    ];
    assert_eq!(
        result.expect("VALUES source identity").rows(),
        expected.as_slice()
    );
}

#[test]
fn should_preserve_bytea_unknown_tail_given_values_sources_when_output_is_cast_to_text() {
    // Arrange
    let fixture = sql_fixture("valuescasts-text-bytea_unknown_tail", &[]);
    // Act
    let result = fixture.execute("SELECT v::TEXT AS output FROM (VALUES ('\\x00Abff'::BYTEA),('\\x0102')) AS q(v) ORDER BY v NULLS LAST");
    // Assert
    let expected: Vec<Vec<Value>> = vec![
        vec![Value::String("\\x00abff".into())],
        vec![Value::String("\\x0102".into())],
    ];
    assert_eq!(
        result.expect("VALUES source identity").rows(),
        expected.as_slice()
    );
}

#[test]
fn should_preserve_bytea_typed_null_given_values_sources_when_output_is_cast_to_text() {
    // Arrange
    let fixture = sql_fixture("valuescasts-text-bytea_typed_null", &[]);
    // Act
    let result = fixture.execute("SELECT v::TEXT AS output FROM (VALUES (NULL::BYTEA),('\\x00Abff'::BYTEA)) AS q(v) ORDER BY v NULLS LAST");
    // Assert
    let expected: Vec<Vec<Value>> =
        vec![vec![Value::String("\\x00abff".into())], vec![Value::Null]];
    assert_eq!(
        result.expect("VALUES source identity").rows(),
        expected.as_slice()
    );
}

#[test]
fn should_preserve_bytea_mixed_width_given_values_sources_when_output_is_cast_to_text() {
    // Arrange
    let fixture = sql_fixture("valuescasts-text-bytea_mixed_width", &[]);
    // Act
    let result = fixture.execute("SELECT v::TEXT AS output FROM (VALUES ('\\x00Abff'::BYTEA),('\\x0102'::BYTEA)) AS q(v) ORDER BY v NULLS LAST");
    // Assert
    let expected: Vec<Vec<Value>> = vec![
        vec![Value::String("\\x00abff".into())],
        vec![Value::String("\\x0102".into())],
    ];
    assert_eq!(
        result.expect("VALUES source identity").rows(),
        expected.as_slice()
    );
}

#[test]
fn should_preserve_char_repeat_given_values_sources_when_output_is_cast_to_text() {
    // Arrange
    let fixture = sql_fixture("valuescasts-text-char_repeat", &[]);
    // Act
    let result = fixture.execute("SELECT v::TEXT AS output FROM (VALUES ('a'::CHAR(3)),('a'::CHAR(3)),('b'::CHAR(3))) AS q(v) ORDER BY v NULLS LAST");
    // Assert
    let expected: Vec<Vec<Value>> = vec![
        vec![Value::String("a".into())],
        vec![Value::String("a".into())],
        vec![Value::String("b".into())],
    ];
    assert_eq!(
        result.expect("VALUES source identity").rows(),
        expected.as_slice()
    );
}

#[test]
fn should_preserve_char_null_tail_given_values_sources_when_output_is_cast_to_text() {
    // Arrange
    let fixture = sql_fixture("valuescasts-text-char_null_tail", &[]);
    // Act
    let result = fixture.execute("SELECT v::TEXT AS output FROM (VALUES ('a'::CHAR(3)),(NULL),('b'::CHAR(3))) AS q(v) ORDER BY v NULLS LAST");
    // Assert
    let expected: Vec<Vec<Value>> = vec![
        vec![Value::String("a".into())],
        vec![Value::String("b".into())],
        vec![Value::Null],
    ];
    assert_eq!(
        result.expect("VALUES source identity").rows(),
        expected.as_slice()
    );
}

#[test]
fn should_preserve_char_unknown_head_given_values_sources_when_output_is_cast_to_text() {
    // Arrange
    let fixture = sql_fixture("valuescasts-text-char_unknown_head", &[]);
    // Act
    let result = fixture.execute(
        "SELECT v::TEXT AS output FROM (VALUES ('b'),('a'::CHAR(3))) AS q(v) ORDER BY v NULLS LAST",
    );
    // Assert
    let expected: Vec<Vec<Value>> = vec![
        vec![Value::String("a".into())],
        vec![Value::String("b".into())],
    ];
    assert_eq!(
        result.expect("VALUES source identity").rows(),
        expected.as_slice()
    );
}

#[test]
fn should_preserve_char_unknown_tail_given_values_sources_when_output_is_cast_to_text() {
    // Arrange
    let fixture = sql_fixture("valuescasts-text-char_unknown_tail", &[]);
    // Act
    let result = fixture.execute(
        "SELECT v::TEXT AS output FROM (VALUES ('a'::CHAR(3)),('b')) AS q(v) ORDER BY v NULLS LAST",
    );
    // Assert
    let expected: Vec<Vec<Value>> = vec![
        vec![Value::String("a".into())],
        vec![Value::String("b".into())],
    ];
    assert_eq!(
        result.expect("VALUES source identity").rows(),
        expected.as_slice()
    );
}

#[test]
fn should_preserve_char_typed_null_given_values_sources_when_output_is_cast_to_text() {
    // Arrange
    let fixture = sql_fixture("valuescasts-text-char_typed_null", &[]);
    // Act
    let result = fixture.execute("SELECT v::TEXT AS output FROM (VALUES (NULL::CHAR(3)),('a'::CHAR(3))) AS q(v) ORDER BY v NULLS LAST");
    // Assert
    let expected: Vec<Vec<Value>> = vec![vec![Value::String("a".into())], vec![Value::Null]];
    assert_eq!(
        result.expect("VALUES source identity").rows(),
        expected.as_slice()
    );
}

#[test]
fn should_preserve_char_mixed_width_given_values_sources_when_output_is_cast_to_text() {
    // Arrange
    let fixture = sql_fixture("valuescasts-text-char_mixed_width", &[]);
    // Act
    let result = fixture.execute("SELECT v::TEXT AS output FROM (VALUES ('a'::CHAR(3)),('b'::CHAR(5))) AS q(v) ORDER BY v NULLS LAST");
    // Assert
    let expected: Vec<Vec<Value>> = vec![
        vec![Value::String("a".into())],
        vec![Value::String("b".into())],
    ];
    assert_eq!(
        result.expect("VALUES source identity").rows(),
        expected.as_slice()
    );
}

#[test]
fn should_preserve_varchar_repeat_given_values_sources_when_output_is_cast_to_text() {
    // Arrange
    let fixture = sql_fixture("valuescasts-text-varchar_repeat", &[]);
    // Act
    let result = fixture.execute("SELECT v::TEXT AS output FROM (VALUES ('ab🙂'::VARCHAR(4)),('ab🙂'::VARCHAR(4)),('b'::VARCHAR(4))) AS q(v) ORDER BY v NULLS LAST");
    // Assert
    let expected: Vec<Vec<Value>> = vec![
        vec![Value::String("ab🙂".into())],
        vec![Value::String("ab🙂".into())],
        vec![Value::String("b".into())],
    ];
    assert_eq!(
        result.expect("VALUES source identity").rows(),
        expected.as_slice()
    );
}

#[test]
fn should_preserve_varchar_null_tail_given_values_sources_when_output_is_cast_to_text() {
    // Arrange
    let fixture = sql_fixture("valuescasts-text-varchar_null_tail", &[]);
    // Act
    let result = fixture.execute("SELECT v::TEXT AS output FROM (VALUES ('ab🙂'::VARCHAR(4)),(NULL),('b'::VARCHAR(4))) AS q(v) ORDER BY v NULLS LAST");
    // Assert
    let expected: Vec<Vec<Value>> = vec![
        vec![Value::String("ab🙂".into())],
        vec![Value::String("b".into())],
        vec![Value::Null],
    ];
    assert_eq!(
        result.expect("VALUES source identity").rows(),
        expected.as_slice()
    );
}

#[test]
fn should_preserve_varchar_unknown_head_given_values_sources_when_output_is_cast_to_text() {
    // Arrange
    let fixture = sql_fixture("valuescasts-text-varchar_unknown_head", &[]);
    // Act
    let result = fixture.execute("SELECT v::TEXT AS output FROM (VALUES ('b'),('ab🙂'::VARCHAR(4))) AS q(v) ORDER BY v NULLS LAST");
    // Assert
    let expected: Vec<Vec<Value>> = vec![
        vec![Value::String("ab🙂".into())],
        vec![Value::String("b".into())],
    ];
    assert_eq!(
        result.expect("VALUES source identity").rows(),
        expected.as_slice()
    );
}

#[test]
fn should_preserve_varchar_unknown_tail_given_values_sources_when_output_is_cast_to_text() {
    // Arrange
    let fixture = sql_fixture("valuescasts-text-varchar_unknown_tail", &[]);
    // Act
    let result = fixture.execute("SELECT v::TEXT AS output FROM (VALUES ('ab🙂'::VARCHAR(4)),('b')) AS q(v) ORDER BY v NULLS LAST");
    // Assert
    let expected: Vec<Vec<Value>> = vec![
        vec![Value::String("ab🙂".into())],
        vec![Value::String("b".into())],
    ];
    assert_eq!(
        result.expect("VALUES source identity").rows(),
        expected.as_slice()
    );
}

#[test]
fn should_preserve_varchar_typed_null_given_values_sources_when_output_is_cast_to_text() {
    // Arrange
    let fixture = sql_fixture("valuescasts-text-varchar_typed_null", &[]);
    // Act
    let result = fixture.execute("SELECT v::TEXT AS output FROM (VALUES (NULL::VARCHAR(4)),('ab🙂'::VARCHAR(4))) AS q(v) ORDER BY v NULLS LAST");
    // Assert
    let expected: Vec<Vec<Value>> = vec![vec![Value::String("ab🙂".into())], vec![Value::Null]];
    assert_eq!(
        result.expect("VALUES source identity").rows(),
        expected.as_slice()
    );
}

#[test]
fn should_preserve_varchar_mixed_width_given_values_sources_when_output_is_cast_to_text() {
    // Arrange
    let fixture = sql_fixture("valuescasts-text-varchar_mixed_width", &[]);
    // Act
    let result = fixture.execute("SELECT v::TEXT AS output FROM (VALUES ('ab🙂'::VARCHAR(4)),('b'::VARCHAR(7))) AS q(v) ORDER BY v NULLS LAST");
    // Assert
    let expected: Vec<Vec<Value>> = vec![
        vec![Value::String("ab🙂".into())],
        vec![Value::String("b".into())],
    ];
    assert_eq!(
        result.expect("VALUES source identity").rows(),
        expected.as_slice()
    );
}
