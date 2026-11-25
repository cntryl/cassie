//! Native BYTEA text conversion values.
use crate::support_sql_fixture::sql_fixture;
use cassie::types::Value;

#[test]
fn should_decode_empty_given_text_input_when_sql_returns_bytea() {
    // Arrange
    let fixture = sql_fixture("byteainput-sql-text-empty", &[]);
    // Act
    let result = fixture.execute("SELECT CAST('' AS TEXT)::BYTEA AS output");
    // Assert
    assert_eq!(
        result.expect("native BYTEA input").rows(),
        &[vec![Value::String("\\x".into())]]
    );
}

#[test]
fn should_decode_ascii_given_text_input_when_sql_returns_bytea() {
    // Arrange
    let fixture = sql_fixture("byteainput-sql-text-ascii", &[]);
    // Act
    let result = fixture.execute("SELECT CAST('abc' AS TEXT)::BYTEA AS output");
    // Assert
    assert_eq!(
        result.expect("native BYTEA input").rows(),
        &[vec![Value::String("\\x616263".into())]]
    );
}

#[test]
fn should_decode_utf8_given_text_input_when_sql_returns_bytea() {
    // Arrange
    let fixture = sql_fixture("byteainput-sql-text-utf8", &[]);
    // Act
    let result = fixture.execute("SELECT CAST('é🙂' AS TEXT)::BYTEA AS output");
    // Assert
    assert_eq!(
        result.expect("native BYTEA input").rows(),
        &[vec![Value::String("\\xc3a9f09f9982".into())]]
    );
}

#[test]
fn should_decode_nul_given_text_input_when_sql_returns_bytea() {
    // Arrange
    let fixture = sql_fixture("byteainput-sql-text-nul", &[]);
    // Act
    let result = fixture.execute("SELECT CAST('\\000' AS TEXT)::BYTEA AS output");
    // Assert
    assert_eq!(
        result.expect("native BYTEA input").rows(),
        &[vec![Value::String("\\x00".into())]]
    );
}

#[test]
fn should_decode_high_given_text_input_when_sql_returns_bytea() {
    // Arrange
    let fixture = sql_fixture("byteainput-sql-text-high", &[]);
    // Act
    let result = fixture.execute("SELECT CAST('\\377' AS TEXT)::BYTEA AS output");
    // Assert
    assert_eq!(
        result.expect("native BYTEA input").rows(),
        &[vec![Value::String("\\xff".into())]]
    );
}

#[test]
fn should_decode_slash_given_text_input_when_sql_returns_bytea() {
    // Arrange
    let fixture = sql_fixture("byteainput-sql-text-slash", &[]);
    // Act
    let result = fixture.execute("SELECT CAST('\\\\' AS TEXT)::BYTEA AS output");
    // Assert
    assert_eq!(
        result.expect("native BYTEA input").rows(),
        &[vec![Value::String("\\x5c".into())]]
    );
}

#[test]
fn should_decode_mixed_given_text_input_when_sql_returns_bytea() {
    // Arrange
    let fixture = sql_fixture("byteainput-sql-text-mixed", &[]);
    // Act
    let result = fixture.execute("SELECT CAST('a\\000b\\\\c\\377' AS TEXT)::BYTEA AS output");
    // Assert
    assert_eq!(
        result.expect("native BYTEA input").rows(),
        &[vec![Value::String("\\x6100625c63ff".into())]]
    );
}

#[test]
fn should_decode_octal_ascii_given_text_input_when_sql_returns_bytea() {
    // Arrange
    let fixture = sql_fixture("byteainput-sql-text-octal_ascii", &[]);
    // Act
    let result = fixture.execute("SELECT CAST('\\101\\102\\103' AS TEXT)::BYTEA AS output");
    // Assert
    assert_eq!(
        result.expect("native BYTEA input").rows(),
        &[vec![Value::String("\\x414243".into())]]
    );
}

#[test]
fn should_decode_octal_low_given_text_input_when_sql_returns_bytea() {
    // Arrange
    let fixture = sql_fixture("byteainput-sql-text-octal_low", &[]);
    // Act
    let result = fixture.execute(
        "SELECT CAST('\\001\\007\\010\\011\\012\\015\\037\\177' AS TEXT)::BYTEA AS output",
    );
    // Assert
    assert_eq!(
        result.expect("native BYTEA input").rows(),
        &[vec![Value::String("\\x010708090a0d1f7f".into())]]
    );
}

#[test]
fn should_decode_hex_empty_given_text_input_when_sql_returns_bytea() {
    // Arrange
    let fixture = sql_fixture("byteainput-sql-text-hex_empty", &[]);
    // Act
    let result = fixture.execute("SELECT CAST('\\x' AS TEXT)::BYTEA AS output");
    // Assert
    assert_eq!(
        result.expect("native BYTEA input").rows(),
        &[vec![Value::String("\\x".into())]]
    );
}

#[test]
fn should_decode_hex_case_given_text_input_when_sql_returns_bytea() {
    // Arrange
    let fixture = sql_fixture("byteainput-sql-text-hex_case", &[]);
    // Act
    let result = fixture.execute("SELECT CAST('\\x00ABff' AS TEXT)::BYTEA AS output");
    // Assert
    assert_eq!(
        result.expect("native BYTEA input").rows(),
        &[vec![Value::String("\\x00abff".into())]]
    );
}

#[test]
fn should_decode_hex_spaces_given_text_input_when_sql_returns_bytea() {
    // Arrange
    let fixture = sql_fixture("byteainput-sql-text-hex_spaces", &[]);
    // Act
    let result = fixture.execute("SELECT CAST('\\x 00 AB ff ' AS TEXT)::BYTEA AS output");
    // Assert
    assert_eq!(
        result.expect("native BYTEA input").rows(),
        &[vec![Value::String("\\x00abff".into())]]
    );
}

#[test]
fn should_decode_hex_lines_given_text_input_when_sql_returns_bytea() {
    // Arrange
    let fixture = sql_fixture("byteainput-sql-text-hex_lines", &[]);
    // Act
    let result = fixture.execute("SELECT CAST('\\x00\n11\r22\t33' AS TEXT)::BYTEA AS output");
    // Assert
    assert_eq!(
        result.expect("native BYTEA input").rows(),
        &[vec![Value::String("\\x00112233".into())]]
    );
}

#[test]
fn should_decode_slash_tail_given_text_input_when_sql_returns_bytea() {
    // Arrange
    let fixture = sql_fixture("byteainput-sql-text-slash_tail", &[]);
    // Act
    let result = fixture.execute("SELECT CAST('abc\\\\' AS TEXT)::BYTEA AS output");
    // Assert
    assert_eq!(
        result.expect("native BYTEA input").rows(),
        &[vec![Value::String("\\x6162635c".into())]]
    );
}

#[test]
fn should_decode_quote_given_text_input_when_sql_returns_bytea() {
    // Arrange
    let fixture = sql_fixture("byteainput-sql-text-quote", &[]);
    // Act
    let result = fixture.execute("SELECT CAST('a''b' AS TEXT)::BYTEA AS output");
    // Assert
    assert_eq!(
        result.expect("native BYTEA input").rows(),
        &[vec![Value::String("\\x612762".into())]]
    );
}

#[test]
fn should_decode_spaces_given_text_input_when_sql_returns_bytea() {
    // Arrange
    let fixture = sql_fixture("byteainput-sql-text-spaces", &[]);
    // Act
    let result = fixture.execute("SELECT CAST(' \t\n ' AS TEXT)::BYTEA AS output");
    // Assert
    assert_eq!(
        result.expect("native BYTEA input").rows(),
        &[vec![Value::String("\\x20090a20".into())]]
    );
}
