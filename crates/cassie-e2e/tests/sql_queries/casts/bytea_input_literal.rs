//! Native BYTEA text conversion values.
use crate::support_sql_fixture::sql_fixture;
use cassie::types::Value;

#[test]
fn should_decode_empty_given_literal_input_when_sql_returns_bytea() {
    // Arrange
    let fixture = sql_fixture("byteainput-sql-literal-empty", &[]);
    // Act
    let result = fixture.execute("SELECT ''::BYTEA AS output");
    // Assert
    assert_eq!(
        result.expect("native BYTEA input").rows(),
        &[vec![Value::String("\\x".into())]]
    );
}

#[test]
fn should_decode_ascii_given_literal_input_when_sql_returns_bytea() {
    // Arrange
    let fixture = sql_fixture("byteainput-sql-literal-ascii", &[]);
    // Act
    let result = fixture.execute("SELECT 'abc'::BYTEA AS output");
    // Assert
    assert_eq!(
        result.expect("native BYTEA input").rows(),
        &[vec![Value::String("\\x616263".into())]]
    );
}

#[test]
fn should_decode_utf8_given_literal_input_when_sql_returns_bytea() {
    // Arrange
    let fixture = sql_fixture("byteainput-sql-literal-utf8", &[]);
    // Act
    let result = fixture.execute("SELECT 'é🙂'::BYTEA AS output");
    // Assert
    assert_eq!(
        result.expect("native BYTEA input").rows(),
        &[vec![Value::String("\\xc3a9f09f9982".into())]]
    );
}

#[test]
fn should_decode_nul_given_literal_input_when_sql_returns_bytea() {
    // Arrange
    let fixture = sql_fixture("byteainput-sql-literal-nul", &[]);
    // Act
    let result = fixture.execute("SELECT '\\000'::BYTEA AS output");
    // Assert
    assert_eq!(
        result.expect("native BYTEA input").rows(),
        &[vec![Value::String("\\x00".into())]]
    );
}

#[test]
fn should_decode_high_given_literal_input_when_sql_returns_bytea() {
    // Arrange
    let fixture = sql_fixture("byteainput-sql-literal-high", &[]);
    // Act
    let result = fixture.execute("SELECT '\\377'::BYTEA AS output");
    // Assert
    assert_eq!(
        result.expect("native BYTEA input").rows(),
        &[vec![Value::String("\\xff".into())]]
    );
}

#[test]
fn should_decode_slash_given_literal_input_when_sql_returns_bytea() {
    // Arrange
    let fixture = sql_fixture("byteainput-sql-literal-slash", &[]);
    // Act
    let result = fixture.execute("SELECT '\\\\'::BYTEA AS output");
    // Assert
    assert_eq!(
        result.expect("native BYTEA input").rows(),
        &[vec![Value::String("\\x5c".into())]]
    );
}

#[test]
fn should_decode_mixed_given_literal_input_when_sql_returns_bytea() {
    // Arrange
    let fixture = sql_fixture("byteainput-sql-literal-mixed", &[]);
    // Act
    let result = fixture.execute("SELECT 'a\\000b\\\\c\\377'::BYTEA AS output");
    // Assert
    assert_eq!(
        result.expect("native BYTEA input").rows(),
        &[vec![Value::String("\\x6100625c63ff".into())]]
    );
}

#[test]
fn should_decode_octal_ascii_given_literal_input_when_sql_returns_bytea() {
    // Arrange
    let fixture = sql_fixture("byteainput-sql-literal-octal_ascii", &[]);
    // Act
    let result = fixture.execute("SELECT '\\101\\102\\103'::BYTEA AS output");
    // Assert
    assert_eq!(
        result.expect("native BYTEA input").rows(),
        &[vec![Value::String("\\x414243".into())]]
    );
}

#[test]
fn should_decode_octal_low_given_literal_input_when_sql_returns_bytea() {
    // Arrange
    let fixture = sql_fixture("byteainput-sql-literal-octal_low", &[]);
    // Act
    let result =
        fixture.execute("SELECT '\\001\\007\\010\\011\\012\\015\\037\\177'::BYTEA AS output");
    // Assert
    assert_eq!(
        result.expect("native BYTEA input").rows(),
        &[vec![Value::String("\\x010708090a0d1f7f".into())]]
    );
}

#[test]
fn should_decode_hex_empty_given_literal_input_when_sql_returns_bytea() {
    // Arrange
    let fixture = sql_fixture("byteainput-sql-literal-hex_empty", &[]);
    // Act
    let result = fixture.execute("SELECT '\\x'::BYTEA AS output");
    // Assert
    assert_eq!(
        result.expect("native BYTEA input").rows(),
        &[vec![Value::String("\\x".into())]]
    );
}

#[test]
fn should_decode_hex_case_given_literal_input_when_sql_returns_bytea() {
    // Arrange
    let fixture = sql_fixture("byteainput-sql-literal-hex_case", &[]);
    // Act
    let result = fixture.execute("SELECT '\\x00ABff'::BYTEA AS output");
    // Assert
    assert_eq!(
        result.expect("native BYTEA input").rows(),
        &[vec![Value::String("\\x00abff".into())]]
    );
}

#[test]
fn should_decode_hex_spaces_given_literal_input_when_sql_returns_bytea() {
    // Arrange
    let fixture = sql_fixture("byteainput-sql-literal-hex_spaces", &[]);
    // Act
    let result = fixture.execute("SELECT '\\x 00 AB ff '::BYTEA AS output");
    // Assert
    assert_eq!(
        result.expect("native BYTEA input").rows(),
        &[vec![Value::String("\\x00abff".into())]]
    );
}

#[test]
fn should_decode_hex_lines_given_literal_input_when_sql_returns_bytea() {
    // Arrange
    let fixture = sql_fixture("byteainput-sql-literal-hex_lines", &[]);
    // Act
    let result = fixture.execute("SELECT '\\x00\n11\r22\t33'::BYTEA AS output");
    // Assert
    assert_eq!(
        result.expect("native BYTEA input").rows(),
        &[vec![Value::String("\\x00112233".into())]]
    );
}

#[test]
fn should_decode_slash_tail_given_literal_input_when_sql_returns_bytea() {
    // Arrange
    let fixture = sql_fixture("byteainput-sql-literal-slash_tail", &[]);
    // Act
    let result = fixture.execute("SELECT 'abc\\\\'::BYTEA AS output");
    // Assert
    assert_eq!(
        result.expect("native BYTEA input").rows(),
        &[vec![Value::String("\\x6162635c".into())]]
    );
}

#[test]
fn should_decode_quote_given_literal_input_when_sql_returns_bytea() {
    // Arrange
    let fixture = sql_fixture("byteainput-sql-literal-quote", &[]);
    // Act
    let result = fixture.execute("SELECT 'a''b'::BYTEA AS output");
    // Assert
    assert_eq!(
        result.expect("native BYTEA input").rows(),
        &[vec![Value::String("\\x612762".into())]]
    );
}

#[test]
fn should_decode_spaces_given_literal_input_when_sql_returns_bytea() {
    // Arrange
    let fixture = sql_fixture("byteainput-sql-literal-spaces", &[]);
    // Act
    let result = fixture.execute("SELECT ' \t\n '::BYTEA AS output");
    // Assert
    assert_eq!(
        result.expect("native BYTEA input").rows(),
        &[vec![Value::String("\\x20090a20".into())]]
    );
}
