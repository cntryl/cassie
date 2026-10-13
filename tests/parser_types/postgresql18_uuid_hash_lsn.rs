//! PostgreSQL 18 UUID, hashing, and `pg_lsn` syntax tracked by #816.

use cassie::sql::parse_statement;

#[test]
fn should_parse_uuid_alternative_input_casts() {
    // Arrange
    let sql = "SELECT CAST('A0EEBC99-9C0B-4EF8-BB6D-6BB9BD380A11' AS UUID), '{a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11}'::UUID, 'a0eebc999c0b4ef8bb6d6bb9bd380a11'::UUID";

    // Act
    let parsed = parse_statement(sql);

    // Assert
    assert!(parsed.is_ok(), "UUID alternative input casts: {parsed:?}");
}

#[test]
fn should_parse_postgresql_uuid_generation_extraction_calls() {
    // Arrange
    let sql = "SELECT gen_random_uuid(), uuidv4(), uuidv7(), uuid_extract_version(UUID '019535d9-3df7-79fb-b466-fa907fa17f9e'), uuid_extract_timestamp(UUID '019535d9-3df7-79fb-b466-fa907fa17f9e')";

    // Act
    let parsed = parse_statement(sql);

    // Assert
    assert!(parsed.is_ok(), "UUID generation and extraction: {parsed:?}");
}

#[test]
fn should_parse_pg_lsn_input_operators() {
    // Arrange
    let sql = "SELECT '16/B374D848'::pg_lsn < '16/B374D850'::pg_lsn, '16/B374D848'::pg_lsn + 8, '16/B374D850'::pg_lsn - '16/B374D848'::pg_lsn";

    // Act
    let parsed = parse_statement(sql);

    // Assert
    assert!(parsed.is_ok(), "pg_lsn operators: {parsed:?}");
}

#[test]
fn should_parse_pg_lsn_function_calls() {
    // Arrange
    let sql = "SELECT pg_wal_lsn_diff('16/B374D850'::pg_lsn, '16/B374D848'::pg_lsn), pg_lsn('16'::numeric)";

    // Act
    let parsed = parse_statement(sql);

    // Assert
    assert!(parsed.is_ok(), "pg_lsn functions: {parsed:?}");
}

#[test]
fn should_parse_postgresql_hashing_calls() {
    // Arrange
    let sql = "SELECT md5('abc'), sha256('abc'::bytea), digest('abc', 'sha256'), hashtext('abc'), hashtextextended('abc', 0)";

    // Act
    let parsed = parse_statement(sql);

    // Assert
    assert!(parsed.is_ok(), "hash and digest calls: {parsed:?}");
}
