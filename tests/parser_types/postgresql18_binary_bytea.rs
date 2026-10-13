//! PostgreSQL 18 bit-string and binary-string grammar tracked by #816.

#[test]
fn should_parse_binary_plus_hex_bit_string_constants() {
    // Arrange
    let sql = "SELECT B'10101', b'001', X'1F', x'a0'";

    // Act
    let parsed = cassie::sql::parse_statement(sql);

    // Assert
    assert!(parsed.is_ok(), "bit-string constants: {parsed:?}");
}

#[test]
fn should_parse_bit_type_names_with_fixed_variable_modifiers() {
    // Arrange
    let cases = [
        "SELECT CAST(B'101' AS BIT(3))",
        "SELECT CAST(B'101' AS BIT VARYING(5))",
        "SELECT CAST(B'101' AS VARBIT(5))",
        "CREATE TABLE issue_816_bits (fixed BIT(3), variable BIT VARYING, alias VARBIT(5))",
    ];

    // Act
    let results = cases
        .iter()
        .map(|sql| cassie::sql::parse_statement(sql))
        .collect::<Vec<_>>();

    // Assert
    let failures = cases
        .iter()
        .zip(results)
        .filter_map(|(sql, result)| result.err().map(|error| format!("{sql}: {error}")))
        .collect::<Vec<_>>();
    assert!(
        failures.is_empty(),
        "bit type grammar:\n{}",
        failures.join("\n")
    );
}

#[test]
fn should_parse_bitwise_operator_forms() {
    // Arrange
    let sql = "SELECT B'10001' || B'011', B'10001' & B'01101', B'10001' | B'01101', B'10001' # B'01101', ~B'10001', B'10001' << 3, B'10001' >> 2";

    // Act
    let parsed = cassie::sql::parse_statement(sql);

    // Assert
    assert!(parsed.is_ok(), "bitwise operators: {parsed:?}");
}

#[test]
fn should_parse_bit_string_function_forms() {
    // Arrange
    let cases = [
        "SELECT position(B'010' IN B'000001101011')",
        "SELECT substring(B'110010111111' FROM 3 FOR 2)",
        "SELECT overlay(B'01010101010101010' PLACING B'11111' FROM 2 FOR 3)",
        "SELECT get_bit(B'101010', 2), set_bit(B'101010', 2, 0)",
    ];

    // Act
    let results = cases
        .iter()
        .map(|sql| cassie::sql::parse_statement(sql))
        .collect::<Vec<_>>();

    // Assert
    let failures = cases
        .iter()
        .zip(results)
        .filter_map(|(sql, result)| result.err().map(|error| format!("{sql}: {error}")))
        .collect::<Vec<_>>();
    assert!(
        failures.is_empty(),
        "bit string functions:\n{}",
        failures.join("\n")
    );
}

#[test]
fn should_parse_bytea_operator_cast_forms() {
    // Arrange
    let sql = "SELECT '\\x1234'::bytea || '\\x00ff'::bytea, 1234::smallint::bytea, '\\x8000'::bytea::integer";

    // Act
    let parsed = cassie::sql::parse_statement(sql);

    // Assert
    assert!(parsed.is_ok(), "bytea operators and casts: {parsed:?}");
}

#[test]
fn should_parse_bytea_keyword_string_functions() {
    // Arrange
    let cases = [
        "SELECT substring('\\x010203'::bytea FROM 2 FOR 1)",
        "SELECT overlay('\\x010203'::bytea PLACING '\\xff'::bytea FROM 2 FOR 1)",
        "SELECT position('\\x02'::bytea IN '\\x010203'::bytea)",
        "SELECT trim(BOTH '\\x01'::bytea FROM '\\x010201'::bytea)",
    ];

    // Act
    let results = cases
        .iter()
        .map(|sql| cassie::sql::parse_statement(sql))
        .collect::<Vec<_>>();

    // Assert
    let failures = cases
        .iter()
        .zip(results)
        .filter_map(|(sql, result)| result.err().map(|error| format!("{sql}: {error}")))
        .collect::<Vec<_>>();
    assert!(
        failures.is_empty(),
        "bytea string grammar:\n{}",
        failures.join("\n")
    );
}

#[test]
fn should_parse_bytea_encoding_conversion_access_functions() {
    // Arrange
    let sql = "SELECT encode(decode('00ff', 'hex'), 'base64'), convert_to('café', 'UTF8'), convert_from(decode('636166c3a9', 'hex'), 'UTF8'), convert(decode('636166c3a9', 'hex'), 'UTF8', 'LATIN1'), get_byte('\\x0102'::bytea, 1), set_byte('\\x0102'::bytea, 1, 3), get_bit('\\x80'::bytea, 7), set_bit('\\x80'::bytea, 7, 0)";

    // Act
    let parsed = cassie::sql::parse_statement(sql);

    // Assert
    assert!(parsed.is_ok(), "bytea function grammar: {parsed:?}");
}
