//! Native BYTEA text conversion values.
use crate::support_sql_fixture::sql_fixture;
use cassie::types::Value;

#[test]
fn should_decode_empty_given_cte_input_when_sql_returns_bytea() {
    // Arrange
    let fixture = sql_fixture("byteainput-sql-cte-empty", &[]);
    // Act
    let result = fixture.execute("WITH q(v) AS (VALUES (''::BYTEA)) SELECT v AS output FROM q");
    // Assert
    assert_eq!(
        result.expect("native BYTEA input").rows(),
        &[vec![Value::String("\\x".into())]]
    );
}

#[test]
fn should_decode_ascii_given_cte_input_when_sql_returns_bytea() {
    // Arrange
    let fixture = sql_fixture("byteainput-sql-cte-ascii", &[]);
    // Act
    let result = fixture.execute("WITH q(v) AS (VALUES ('abc'::BYTEA)) SELECT v AS output FROM q");
    // Assert
    assert_eq!(
        result.expect("native BYTEA input").rows(),
        &[vec![Value::String("\\x616263".into())]]
    );
}

#[test]
fn should_decode_utf8_given_cte_input_when_sql_returns_bytea() {
    // Arrange
    let fixture = sql_fixture("byteainput-sql-cte-utf8", &[]);
    // Act
    let result = fixture.execute("WITH q(v) AS (VALUES ('é🙂'::BYTEA)) SELECT v AS output FROM q");
    // Assert
    assert_eq!(
        result.expect("native BYTEA input").rows(),
        &[vec![Value::String("\\xc3a9f09f9982".into())]]
    );
}

#[test]
fn should_decode_nul_given_cte_input_when_sql_returns_bytea() {
    // Arrange
    let fixture = sql_fixture("byteainput-sql-cte-nul", &[]);
    // Act
    let result =
        fixture.execute("WITH q(v) AS (VALUES ('\\000'::BYTEA)) SELECT v AS output FROM q");
    // Assert
    assert_eq!(
        result.expect("native BYTEA input").rows(),
        &[vec![Value::String("\\x00".into())]]
    );
}

#[test]
fn should_decode_high_given_cte_input_when_sql_returns_bytea() {
    // Arrange
    let fixture = sql_fixture("byteainput-sql-cte-high", &[]);
    // Act
    let result =
        fixture.execute("WITH q(v) AS (VALUES ('\\377'::BYTEA)) SELECT v AS output FROM q");
    // Assert
    assert_eq!(
        result.expect("native BYTEA input").rows(),
        &[vec![Value::String("\\xff".into())]]
    );
}

#[test]
fn should_decode_slash_given_cte_input_when_sql_returns_bytea() {
    // Arrange
    let fixture = sql_fixture("byteainput-sql-cte-slash", &[]);
    // Act
    let result = fixture.execute("WITH q(v) AS (VALUES ('\\\\'::BYTEA)) SELECT v AS output FROM q");
    // Assert
    assert_eq!(
        result.expect("native BYTEA input").rows(),
        &[vec![Value::String("\\x5c".into())]]
    );
}

#[test]
fn should_decode_mixed_given_cte_input_when_sql_returns_bytea() {
    // Arrange
    let fixture = sql_fixture("byteainput-sql-cte-mixed", &[]);
    // Act
    let result = fixture
        .execute("WITH q(v) AS (VALUES ('a\\000b\\\\c\\377'::BYTEA)) SELECT v AS output FROM q");
    // Assert
    assert_eq!(
        result.expect("native BYTEA input").rows(),
        &[vec![Value::String("\\x6100625c63ff".into())]]
    );
}

#[test]
fn should_decode_octal_ascii_given_cte_input_when_sql_returns_bytea() {
    // Arrange
    let fixture = sql_fixture("byteainput-sql-cte-octal_ascii", &[]);
    // Act
    let result = fixture
        .execute("WITH q(v) AS (VALUES ('\\101\\102\\103'::BYTEA)) SELECT v AS output FROM q");
    // Assert
    assert_eq!(
        result.expect("native BYTEA input").rows(),
        &[vec![Value::String("\\x414243".into())]]
    );
}

#[test]
fn should_decode_octal_low_given_cte_input_when_sql_returns_bytea() {
    // Arrange
    let fixture = sql_fixture("byteainput-sql-cte-octal_low", &[]);
    // Act
    let result = fixture.execute("WITH q(v) AS (VALUES ('\\001\\007\\010\\011\\012\\015\\037\\177'::BYTEA)) SELECT v AS output FROM q");
    // Assert
    assert_eq!(
        result.expect("native BYTEA input").rows(),
        &[vec![Value::String("\\x010708090a0d1f7f".into())]]
    );
}

#[test]
fn should_decode_hex_empty_given_cte_input_when_sql_returns_bytea() {
    // Arrange
    let fixture = sql_fixture("byteainput-sql-cte-hex_empty", &[]);
    // Act
    let result = fixture.execute("WITH q(v) AS (VALUES ('\\x'::BYTEA)) SELECT v AS output FROM q");
    // Assert
    assert_eq!(
        result.expect("native BYTEA input").rows(),
        &[vec![Value::String("\\x".into())]]
    );
}

#[test]
fn should_decode_hex_case_given_cte_input_when_sql_returns_bytea() {
    // Arrange
    let fixture = sql_fixture("byteainput-sql-cte-hex_case", &[]);
    // Act
    let result =
        fixture.execute("WITH q(v) AS (VALUES ('\\x00ABff'::BYTEA)) SELECT v AS output FROM q");
    // Assert
    assert_eq!(
        result.expect("native BYTEA input").rows(),
        &[vec![Value::String("\\x00abff".into())]]
    );
}

#[test]
fn should_decode_hex_spaces_given_cte_input_when_sql_returns_bytea() {
    // Arrange
    let fixture = sql_fixture("byteainput-sql-cte-hex_spaces", &[]);
    // Act
    let result =
        fixture.execute("WITH q(v) AS (VALUES ('\\x 00 AB ff '::BYTEA)) SELECT v AS output FROM q");
    // Assert
    assert_eq!(
        result.expect("native BYTEA input").rows(),
        &[vec![Value::String("\\x00abff".into())]]
    );
}

#[test]
fn should_decode_hex_lines_given_cte_input_when_sql_returns_bytea() {
    // Arrange
    let fixture = sql_fixture("byteainput-sql-cte-hex_lines", &[]);
    // Act
    let result = fixture
        .execute("WITH q(v) AS (VALUES ('\\x00\n11\r22\t33'::BYTEA)) SELECT v AS output FROM q");
    // Assert
    assert_eq!(
        result.expect("native BYTEA input").rows(),
        &[vec![Value::String("\\x00112233".into())]]
    );
}

#[test]
fn should_decode_slash_tail_given_cte_input_when_sql_returns_bytea() {
    // Arrange
    let fixture = sql_fixture("byteainput-sql-cte-slash_tail", &[]);
    // Act
    let result =
        fixture.execute("WITH q(v) AS (VALUES ('abc\\\\'::BYTEA)) SELECT v AS output FROM q");
    // Assert
    assert_eq!(
        result.expect("native BYTEA input").rows(),
        &[vec![Value::String("\\x6162635c".into())]]
    );
}

#[test]
fn should_decode_quote_given_cte_input_when_sql_returns_bytea() {
    // Arrange
    let fixture = sql_fixture("byteainput-sql-cte-quote", &[]);
    // Act
    let result = fixture.execute("WITH q(v) AS (VALUES ('a''b'::BYTEA)) SELECT v AS output FROM q");
    // Assert
    assert_eq!(
        result.expect("native BYTEA input").rows(),
        &[vec![Value::String("\\x612762".into())]]
    );
}

#[test]
fn should_decode_spaces_given_cte_input_when_sql_returns_bytea() {
    // Arrange
    let fixture = sql_fixture("byteainput-sql-cte-spaces", &[]);
    // Act
    let result =
        fixture.execute("WITH q(v) AS (VALUES (' \t\n '::BYTEA)) SELECT v AS output FROM q");
    // Assert
    assert_eq!(
        result.expect("native BYTEA input").rows(),
        &[vec![Value::String("\\x20090a20".into())]]
    );
}
