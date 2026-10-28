//! PostgreSQL 18 UUID, hashing, and `pg_lsn` behavior tracked by #816.

use cassie::types::Value;

use crate::support_sql_fixture::sql_fixture;

#[test]
fn should_accept_uuid_uppercase_input() {
    // Arrange
    let fixture = sql_fixture("issue-816-uuid-uppercase", &[]);

    // Act
    let result = fixture.rows(
        "SELECT CAST('A0EEBC99-9C0B-4EF8-BB6D-6BB9BD380A11' AS UUID) = CAST('a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11' AS UUID)",
    );

    // Assert
    assert_eq!(result, vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_accept_braced_uuid_input() {
    // Arrange
    let fixture = sql_fixture("issue-816-uuid-braces", &[]);

    // Act
    let result = fixture.rows(
        "SELECT CAST('{a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11}' AS UUID) = CAST('a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11' AS UUID)",
    );

    // Assert
    assert_eq!(result, vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_accept_uuid_input_without_hyphens() {
    // Arrange
    let fixture = sql_fixture("issue-816-uuid-compact", &[]);

    // Act
    let result = fixture.rows(
        "SELECT CAST('a0eebc999c0b4ef8bb6d6bb9bd380a11' AS UUID) = CAST('a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11' AS UUID)",
    );

    // Assert
    assert_eq!(result, vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_accept_uuid_hyphens_after_four_digit_groups() {
    // Arrange
    let fixture = sql_fixture("issue-816-uuid-four-digit-groups", &[]);

    // Act
    let result = fixture.rows(
        "SELECT CAST('a0ee-bc99-9c0b-4ef8-bb6d-6bb9-bd38-0a11' AS UUID) = CAST('a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11' AS UUID)",
    );

    // Assert
    assert_eq!(result, vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_render_uuid_input_in_canonical_lowercase_form() {
    // Arrange
    let fixture = sql_fixture("issue-816-uuid-canonical-output", &[]);

    // Act
    let result =
        fixture.rows("SELECT CAST(CAST('{A0EEBC99-9C0B-4EF8-BB6D-6BB9BD380A11}' AS UUID) AS TEXT)");

    // Assert
    assert_eq!(
        result,
        vec![vec![Value::String(
            "a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11".to_owned()
        )]]
    );
}

#[test]
fn should_reject_malformed_uuid_input() {
    // Arrange
    let fixture = sql_fixture("issue-816-uuid-invalid-input", &[]);

    // Act
    let result = fixture.execute("SELECT CAST('a0eebc99-not-a-uuid' AS UUID)");

    // Assert
    assert!(result.is_err(), "malformed UUID input must be rejected");
}

#[test]
fn should_call_gen_random_uuid_as_a_sql_expression() {
    // Arrange
    let fixture = sql_fixture("issue-816-gen-random-uuid-expression", &[]);

    // Act
    let result = fixture.rows("SELECT uuid_extract_version(gen_random_uuid()) = 4");

    // Assert
    assert_eq!(result, vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_generate_version_four_uuids_with_uuidv4() {
    // Arrange
    let fixture = sql_fixture("issue-816-uuidv4", &[]);

    // Act
    let result = fixture.rows("SELECT uuid_extract_version(uuidv4()) = 4");

    // Assert
    assert_eq!(result, vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_generate_version_seven_uuids_with_uuidv7() {
    // Arrange
    let fixture = sql_fixture("issue-816-uuidv7", &[]);

    // Act
    let result = fixture.rows("SELECT uuid_extract_version(uuidv7()) = 7");

    // Assert
    assert_eq!(result, vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_extract_a_timestamp_from_a_version_seven_uuid() {
    // Arrange
    let fixture = sql_fixture("issue-816-uuidv7-extract-timestamp", &[]);

    // Act
    let result = fixture.rows(
        "SELECT uuid_extract_timestamp(UUID '019535d9-3df7-79fb-b466-fa907fa17f9e') IS NOT NULL",
    );

    // Assert
    assert_eq!(result, vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_format_a_version_seven_timestamp_in_the_session_timezone() {
    // Arrange
    let fixture = sql_fixture(
        "issue-816-uuidv7-extract-timezone",
        &["SET TIME ZONE 'America/New_York'"],
    );

    // Act
    let result = fixture
        .execute("SELECT uuid_extract_timestamp(UUID '019535d9-3df7-79fb-b466-fa907fa17f9e')::text")
        .expect("UUIDv7 timestamp casts to text");

    // Assert
    assert_eq!(
        result.rows(),
        &[vec![Value::String("2025-02-23 21:46:24.503-05".to_owned())]]
    );
}

#[test]
fn should_return_null_timestamp_for_a_version_four_uuid() {
    // Arrange
    let fixture = sql_fixture("issue-816-uuidv4-no-timestamp", &[]);

    // Act
    let result = fixture
        .rows("SELECT uuid_extract_timestamp(UUID '41db1265-8bc1-4ab3-992f-885799a4af1d') IS NULL");

    // Assert
    assert_eq!(result, vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_return_null_version_for_a_non_rfc_uuid_variant() {
    // Arrange
    let fixture = sql_fixture("issue-816-uuid-non-rfc-variant", &[]);

    // Act
    let result = fixture
        .rows("SELECT uuid_extract_version(UUID '00000000-0000-0000-0000-000000000000') IS NULL");

    // Assert
    assert_eq!(result, vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_generate_distinct_uuid_values_for_separate_calls() {
    // Arrange
    let fixture = sql_fixture("issue-816-uuid-call-uniqueness", &[]);

    // Act
    let result = fixture.rows("SELECT uuidv4() <> uuidv4()");

    // Assert
    assert_eq!(result, vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_return_postgresql_md5_text_vectors() {
    // Arrange
    let fixture = sql_fixture("issue-816-md5-vectors", &[]);

    // Act
    let result = fixture.rows("SELECT md5('abc'), md5('abc'::BYTEA)");

    // Assert
    assert_eq!(
        result,
        vec![vec![
            Value::String("900150983cd24fb0d6963f7d28e17f72".to_owned()),
            Value::String("900150983cd24fb0d6963f7d28e17f72".to_owned())
        ]]
    );
}

#[test]
fn should_return_the_postgresql_sha256_binary_string_vector() {
    // Arrange
    let fixture = sql_fixture("issue-816-sha256-vector", &[]);

    // Act
    let result = fixture.rows("SELECT encode(sha256('abc'::BYTEA), 'hex')");

    // Assert
    assert_eq!(
        result,
        vec![vec![Value::String(
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad".to_owned()
        )]]
    );
}

#[test]
fn should_return_the_postgresql_sha224_binary_string_vector() {
    // Arrange
    let fixture = sql_fixture("issue-816-sha224-vector", &[]);

    // Act
    let result = fixture.rows("SELECT encode(sha224('abc'::BYTEA), 'hex')");

    // Assert
    assert_eq!(
        result,
        vec![vec![Value::String(
            "23097d223405d8228642a477bda255b32aadbce4bda0b3f7e36c9da7".to_owned()
        )]]
    );
}

#[test]
fn should_return_the_postgresql_sha384_binary_string_vector() {
    // Arrange
    let fixture = sql_fixture("issue-816-sha384-vector", &[]);

    // Act
    let result = fixture.rows("SELECT encode(sha384('abc'::BYTEA), 'hex')");

    // Assert
    assert_eq!(
        result,
        vec![vec![Value::String(
            "cb00753f45a35e8bb5a03d699ac65007272c32ab0eded1631a8b605a43ff5bed8086072ba1e7cc2358baeca134c825a7".to_owned()
        )]]
    );
}

#[test]
fn should_return_the_postgresql_sha512_binary_string_vector() {
    // Arrange
    let fixture = sql_fixture("issue-816-sha512-vector", &[]);

    // Act
    let result = fixture.rows("SELECT encode(sha512('abc'::BYTEA), 'hex')");

    // Assert
    assert_eq!(
        result,
        vec![vec![Value::String(
            "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f".to_owned()
        )]]
    );
}

#[test]
fn should_propagate_null_through_selected_sha256_function() {
    // Arrange
    let fixture = sql_fixture("issue-816-sha256-null-input", &[]);

    // Act
    let result = fixture.rows("SELECT sha256(NULL::BYTEA) IS NULL");

    // Assert
    assert_eq!(result, vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_return_null_for_null_text_hash_inputs() {
    // Arrange
    let fixture = sql_fixture("issue-816-text-hash-null-inputs", &[]);

    // Act
    let result = fixture
        .rows("SELECT hashtext(NULL::TEXT) IS NULL, hashtextextended(NULL::TEXT, 0) IS NULL");

    // Assert
    assert_eq!(result, vec![vec![Value::Bool(true), Value::Bool(true)]]);
}

#[test]
fn should_render_pg_lsn_values_with_uppercase_hexadecimal_words() {
    // Arrange
    let fixture = sql_fixture("issue-816-pg-lsn-text-output", &[]);

    // Act
    let result = fixture.rows("SELECT CAST('00000016/b374d848' AS PG_LSN)::TEXT");

    // Assert
    assert_eq!(result, vec![vec![Value::String("16/B374D848".to_owned())]]);
}

#[test]
fn should_compare_pg_lsn_values_by_their_unsigned_64_bit_positions() {
    // Arrange
    let fixture = sql_fixture("issue-816-pg-lsn-comparison", &[]);

    // Act
    let result = fixture.rows(
        "SELECT CAST('0/FFFFFFFF' AS PG_LSN) < CAST('1/0' AS PG_LSN), CAST('FFFFFFFF/FFFFFFFF' AS PG_LSN) > CAST('7FFFFFFF/FFFFFFFF' AS PG_LSN)",
    );

    // Assert
    assert_eq!(result, vec![vec![Value::Bool(true), Value::Bool(true)]]);
}

#[test]
fn should_add_numeric_byte_offsets_to_pg_lsn() {
    // Arrange
    let fixture = sql_fixture("issue-816-pg-lsn-arithmetic", &[]);

    // Act
    let result = fixture
        .rows("SELECT CAST(CAST('16/B374D848' AS PG_LSN) + 8::NUMERIC AS TEXT) = '16/B374D850'");

    // Assert
    assert_eq!(result, vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_subtract_numeric_byte_offsets_from_pg_lsn() {
    // Arrange
    let fixture = sql_fixture("issue-816-pg-lsn-subtract-offset", &[]);

    // Act
    let result = fixture
        .rows("SELECT CAST(CAST('16/B374D850' AS PG_LSN) - 8::NUMERIC AS TEXT) = '16/B374D848'");

    // Assert
    assert_eq!(result, vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_add_numeric_offsets_before_pg_lsn_values() {
    // Arrange
    let fixture = sql_fixture("issue-816-pg-lsn-reversed-addition", &[]);

    // Act
    let result = fixture.rows("SELECT CAST(8::NUMERIC + CAST('16/B374D848' AS PG_LSN) AS TEXT)");

    // Assert
    assert_eq!(result, vec![vec![Value::String("16/B374D850".to_owned())]]);
}

#[test]
fn should_subtract_pg_lsn_values_to_get_the_byte_distance() {
    // Arrange
    let fixture = sql_fixture("issue-816-pg-lsn-difference", &[]);

    // Act
    let result = fixture.rows(
        "SELECT CAST('16/B374D850' AS PG_LSN) - CAST('16/B374D848' AS PG_LSN) = 8::NUMERIC, CAST('16/B374D848' AS PG_LSN) - CAST('16/B374D850' AS PG_LSN) = -8::NUMERIC",
    );

    // Assert
    assert_eq!(result, vec![vec![Value::Bool(true), Value::Bool(true)]]);
}

#[test]
fn should_convert_pg_lsn_difference_with_pg_wal_lsn_diff() {
    // Arrange
    let fixture = sql_fixture("issue-816-pg-wal-lsn-diff", &[]);

    // Act
    let result = fixture
        .rows("SELECT pg_wal_lsn_diff('16/B374D850'::PG_LSN, '16/B374D848'::PG_LSN) = 8::NUMERIC");

    // Assert
    assert_eq!(result, vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_convert_numeric_positions_to_pg_lsn() {
    // Arrange
    let fixture = sql_fixture("issue-816-pg-lsn-from-numeric", &[]);

    // Act
    let result = fixture.rows("SELECT CAST(pg_lsn(16::NUMERIC) AS TEXT)");

    // Assert
    assert_eq!(result, vec![vec![Value::String("0/10".to_owned())]]);
}

#[test]
fn should_reject_malformed_pg_lsn_input() {
    // Arrange
    let fixture = sql_fixture("issue-816-pg-lsn-invalid-input", &[]);

    // Act
    let result = fixture.execute("SELECT CAST('not-an-lsn' AS PG_LSN)");

    // Assert
    assert!(result.is_err(), "malformed pg_lsn input must be rejected");
}

#[test]
fn should_propagate_null_through_pg_lsn_arithmetic() {
    // Arrange
    let fixture = sql_fixture("issue-816-pg-lsn-null-arithmetic", &[]);

    // Act
    let result = fixture.rows("SELECT (NULL::PG_LSN + 1::NUMERIC) IS NULL");

    // Assert
    assert_eq!(result, vec![vec![Value::Bool(true)]]);
}
