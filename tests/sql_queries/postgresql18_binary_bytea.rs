//! PostgreSQL 18 bit-string and bytea behavior tracked by #816.

use cassie::types::Value;

use crate::support_sql_fixture::{sql_fixture, SqlFixture};

fn assert_true(fixture: &SqlFixture, sql: &str) {
    // Act
    let result = fixture
        .execute(sql)
        .expect("execute compatibility assertion");

    // Assert
    let rows = result.rows();
    assert_eq!(rows.len(), 1, "{sql}");
    assert!(
        !rows[0].is_empty() && rows[0].iter().all(|value| value == &Value::Bool(true)),
        "{sql}: {:?}",
        rows[0]
    );
}

#[test]
fn should_preserve_bit_values_for_fixed_variable_types() {
    // Arrange
    let fixture = sql_fixture("issue-816-bit-types", &[]);

    // Act
    // Assert
    assert_true(
        &fixture,
        "SELECT B'10101'::bit(5) = B'10101', B'00101'::varbit = B'00101'",
    );
}

#[test]
fn should_execute_hexadecimal_bit_string_constants() {
    // Arrange
    let fixture = sql_fixture("issue-816-hex-bit-constants", &[]);

    // Act
    // Assert
    assert_true(
        &fixture,
        "SELECT X'1F'::TEXT = '00011111', x'a0'::TEXT = '10100000', bit_length(X'1F') = 8, bit_count(X'1F') = 5",
    );
}

#[test]
fn should_apply_fixed_bit_padding_truncation_on_the_right() {
    // Arrange
    let fixture = sql_fixture("issue-816-bit-explicit-cast", &[]);

    // Act
    // Assert
    assert_true(
        &fixture,
        "SELECT B'10'::bit(3) = B'100', B'1011'::bit(3) = B'101'",
    );
}

#[test]
fn should_truncate_explicit_varbit_casts_without_padding() {
    // Arrange
    let fixture = sql_fixture("issue-816-varbit-explicit-cast", &[]);

    // Act
    // Assert
    assert_true(&fixture, "SELECT B'1011'::varbit(3) = B'101'");
}

#[test]
fn should_sign_extend_integer_casts_to_bit() {
    // Arrange
    let fixture = sql_fixture("issue-816-bit-integer-signedness", &[]);

    // Act
    // Assert
    assert_true(
        &fixture,
        "SELECT CAST(44 AS bit(10)) = B'0000101100', CAST(44 AS bit(3)) = B'100', CAST(-44 AS bit(12)) = B'111111010100', CAST(44 AS bit) = B'0', CAST(B'1110' AS integer) = 14",
    );
}

#[test]
fn should_store_exact_fixed_bit_lengths() {
    // Arrange
    let fixture = sql_fixture(
        "issue-816-bit-column-length",
        &["CREATE TABLE issue_816_fixed_bit (value BIT(3))"],
    );
    fixture
        .execute("INSERT INTO issue_816_fixed_bit VALUES (B'101')")
        .expect("insert exact fixed bit length");

    // Act
    let rows = fixture.rows("SELECT value = B'101' FROM issue_816_fixed_bit");

    // Assert
    assert_eq!(rows, vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_reject_implicit_fixed_bit_length_mismatches() {
    // Arrange
    let fixture = sql_fixture(
        "issue-816-bit-column-mismatch",
        &["CREATE TABLE issue_816_fixed_bit_mismatch (value BIT(3))"],
    );

    // Act
    let short = fixture.execute("INSERT INTO issue_816_fixed_bit_mismatch VALUES (B'10')");
    let long = fixture.execute("INSERT INTO issue_816_fixed_bit_mismatch VALUES (B'1010')");

    // Assert
    assert!(short.is_err(), "short implicit fixed-bit input must fail");
    assert!(long.is_err(), "long implicit fixed-bit input must fail");
}

#[test]
fn should_reject_varbit_values_above_the_declared_maximum() {
    // Arrange
    let fixture = sql_fixture(
        "issue-816-varbit-column-bound",
        &["CREATE TABLE issue_816_varbit_bound (value VARBIT(3))"],
    );

    // Act
    let within_bound = fixture.execute("INSERT INTO issue_816_varbit_bound VALUES (B'101')");
    let over_bound = fixture.execute("INSERT INTO issue_816_varbit_bound VALUES (B'1010')");

    // Assert
    assert!(
        within_bound.is_ok(),
        "value at the varbit maximum should fit"
    );
    assert!(
        over_bound.is_err(),
        "value above the varbit maximum must fail"
    );
}

#[test]
fn should_reject_zero_length_bit_type_modifiers() {
    // Arrange
    let fixture = sql_fixture("issue-816-bit-type-lower-bound", &[]);

    // Act
    let fixed = fixture.execute("SELECT NULL::BIT(0)");
    let varying = fixture.execute("SELECT NULL::VARBIT(0)");

    // Assert
    assert!(fixed.is_err(), "fixed bit length must be positive");
    assert!(varying.is_err(), "varbit maximum length must be positive");
}

#[test]
fn should_compare_bit_strings_without_discarding_leading_zeroes() {
    // Arrange
    let fixture = sql_fixture("issue-816-bit-comparison", &[]);

    // Act
    // Assert
    assert_true(
        &fixture,
        "SELECT B'00101'::varbit <> B'101'::varbit, B'00101'::bit(5) < B'10100'::bit(5)",
    );
}

#[test]
fn should_evaluate_the_bitwise_operator_truth_table() {
    // Arrange
    let fixture = sql_fixture("issue-816-bit-operators", &[]);

    // Act
    // Assert
    assert_true(
        &fixture,
        "SELECT B'10001' || B'011' = B'10001011', B'10001' & B'01101' = B'00001', B'10001' | B'01101' = B'11101', B'10001' # B'01101' = B'11100', ~B'10001' = B'01110', (B'10001' << 3) = B'01000', (B'10001' >> 2) = B'00100'",
    );
}

#[test]
fn should_reject_bitwise_operations_on_different_lengths() {
    // Arrange
    let fixture = sql_fixture("issue-816-bit-operator-length", &[]);

    // Act
    let result = fixture.execute("SELECT B'101' & B'10'");

    // Assert
    assert!(
        result.is_err(),
        "bitwise operands with different lengths must fail"
    );
}

#[test]
fn should_measure_non_aligned_bit_string_lengths() {
    // Arrange
    let fixture = sql_fixture("issue-816-bit-length-functions", &[]);

    // Act
    // Assert
    assert_true(
        &fixture,
        "SELECT bit_count(B'10111') = 4, bit_length(B'10111') = 5, length(B'10111') = 5, octet_length(B'1011111011') = 2",
    );
}

#[test]
fn should_apply_bit_string_search_slice_overlay() {
    // Arrange
    let fixture = sql_fixture("issue-816-bit-string-functions", &[]);

    // Act
    // Assert
    assert_true(
        &fixture,
        "SELECT position(B'010' IN B'000001101011') = 8, substring(B'110010111111' FROM 3 FOR 2) = B'00', overlay(B'01010101010101010' PLACING B'11111' FROM 2 FOR 3) = B'0111110101010101010'",
    );
}

#[test]
fn should_mutate_left_indexed_bit_string_positions() {
    // Arrange
    let fixture = sql_fixture("issue-816-bit-position-access", &[]);

    // Act
    // Assert
    assert_true(
        &fixture,
        "SELECT get_bit(B'101010101010101010', 6) = 1, set_bit(B'101010101010101010', 6, 0) = B'101010001010101010'",
    );
}

#[test]
fn should_propagate_null_through_bit_string_functions() {
    // Arrange
    let fixture = sql_fixture("issue-816-bit-null-propagation", &[]);

    // Act
    let result = fixture.rows(
        "SELECT bit_count(NULL::bit varying), length(NULL::bit), get_bit(NULL::bit varying, 0)",
    );

    // Assert
    assert_eq!(result, vec![vec![Value::Null, Value::Null, Value::Null]]);
}

#[test]
fn should_compare_bytea_values_after_concatenation() {
    // Arrange
    let fixture = sql_fixture("issue-816-bytea-operators", &[]);

    // Act
    // Assert
    assert_true(
        &fixture,
        r"SELECT '\x123456'::bytea || '\x789a00bcde'::bytea = '\x123456789a00bcde'::bytea, '\x01ff'::bytea < '\x0200'::bytea",
    );
}

#[test]
fn should_report_bytea_lengths_in_bytes() {
    // Arrange
    let fixture = sql_fixture("issue-816-bytea-lengths", &[]);

    // Act
    // Assert
    assert_true(
        &fixture,
        r"SELECT length('\x123456'::bytea) = 3, octet_length('\x123456'::bytea) = 3, bit_length('\x123456'::bytea) = 24, length(decode('c3a9', 'hex'), 'UTF8') = 1, bit_count('\x1234567890'::bytea) = 15",
    );
}

#[test]
fn should_reverse_bytea_without_reversing_bits_inside_bytes() {
    // Arrange
    let fixture = sql_fixture("issue-816-bytea-reverse", &[]);

    // Act
    // Assert
    assert_true(
        &fixture,
        r"SELECT reverse('\xabcd'::bytea) = '\xcdab'::bytea",
    );
}

#[test]
fn should_trim_byte_sets_from_bytea_ends() {
    // Arrange
    let fixture = sql_fixture("issue-816-bytea-trim", &[]);

    // Act
    // Assert
    assert_true(
        &fixture,
        r"SELECT btrim('\x1234567890'::bytea, '\x9012'::bytea) = '\x345678'::bytea, ltrim('\x1234567890'::bytea, '\x9012'::bytea) = '\x34567890'::bytea, rtrim('\x1234567890'::bytea, '\x9012'::bytea) = '\x12345678'::bytea, trim(BOTH '\x9012'::bytea FROM '\x1234567890'::bytea) = '\x345678'::bytea",
    );
}

#[test]
fn should_apply_bytea_search_slice_overlay_by_byte_position() {
    // Arrange
    let fixture = sql_fixture("issue-816-bytea-string-functions", &[]);

    // Act
    // Assert
    assert_true(
        &fixture,
        r"SELECT position('\x5678'::bytea IN '\x1234567890'::bytea) = 3, substring('\x1234567890'::bytea FROM 3 FOR 2) = '\x5678'::bytea, substr('\x1234567890'::bytea, 3, 2) = '\x5678'::bytea, overlay('\x1234567890'::bytea PLACING '\x0203'::bytea FROM 2 FOR 3) = '\x12020390'::bytea",
    );
}

#[test]
fn should_mutate_bytea_bit_positions_from_the_right() {
    // Arrange
    let fixture = sql_fixture("issue-816-bytea-bit-access", &[]);

    // Act
    // Assert
    assert_true(
        &fixture,
        r"SELECT get_bit('\x1234567890'::bytea, 30) = 1, set_bit('\x1234567890'::bytea, 30, 0) = '\x1234563890'::bytea",
    );
}

#[test]
fn should_mutate_zero_indexed_byte_positions() {
    // Arrange
    let fixture = sql_fixture("issue-816-bytea-byte-access", &[]);

    // Act
    // Assert
    assert_true(
        &fixture,
        r"SELECT get_byte('\x1234567890'::bytea, 4) = 144, set_byte('\x1234567890'::bytea, 4, 64) = '\x1234567840'::bytea",
    );
}

#[test]
fn should_propagate_null_through_bytea_functions() {
    // Arrange
    let fixture = sql_fixture("issue-816-bytea-null-propagation", &[]);

    // Act
    let result = fixture.rows(
        "SELECT bit_count(NULL::bytea), get_byte(NULL::bytea, 0), encode(NULL::bytea, 'hex')",
    );

    // Assert
    assert_eq!(result, vec![vec![Value::Null, Value::Null, Value::Null]]);
}

#[test]
fn should_round_trip_bytea_as_lowercase_hex() {
    // Arrange
    let fixture = sql_fixture("issue-816-bytea-hex-codec", &[]);

    // Act
    // Assert
    assert_true(
        &fixture,
        r"SELECT encode(decode('DEad00FF', 'hex'), 'hex') = 'dead00ff', encode(decode('dead00ff', 'hex'), 'hex') = 'dead00ff'",
    );
}

#[test]
fn should_decode_hexadecimal_text_to_bytea() {
    // Arrange
    let fixture = sql_fixture("issue-816-bytea-hex-decode", &[]);

    // Act
    let result = fixture
        .execute("SELECT decode('DEad00FF', 'hex')")
        .unwrap_or_else(|error| panic!("bytea decode failed: {error:?}"))
        .rows()
        .to_vec();

    // Assert
    assert_eq!(result, vec![vec![Value::String(r"\xdead00ff".to_owned())]]);
}

#[test]
fn should_round_trip_bytea_as_base64() {
    // Arrange
    let fixture = sql_fixture("issue-816-bytea-base64-codec", &[]);

    // Act
    // Assert
    assert_true(
        &fixture,
        r"SELECT encode(decode('MTIzAAE=', 'base64'), 'hex') = '3132330001', encode(decode(E'MTIz AA E=\n', 'base64'), 'hex') = '3132330001'",
    );
}

#[test]
fn should_round_trip_bytea_using_escape_format() {
    // Arrange
    let fixture = sql_fixture("issue-816-bytea-escape-codec", &[]);

    // Act
    // Assert
    assert_true(
        &fixture,
        r"SELECT encode(decode('41005cff', 'hex'), 'escape') = E'A\\000\\\\\\377', encode(decode(encode(decode('41005cff', 'hex'), 'escape'), 'escape'), 'hex') = '41005cff'",
    );
}

#[test]
fn should_accept_both_bytea_input_formats() {
    // Arrange
    let fixture = sql_fixture("issue-816-bytea-input", &[]);

    // Act
    // Assert
    assert_true(
        &fixture,
        r"SELECT '\xDE AD BE EF'::bytea = '\xdeadbeef'::bytea, '\000\134\377'::bytea = '\x005cff'::bytea",
    );
}

#[test]
fn should_convert_text_across_utf8_latin1_bytea() {
    // Arrange
    let fixture = sql_fixture("issue-816-bytea-encoding-conversions", &[]);

    // Act
    // Assert
    assert_true(
        &fixture,
        "SELECT encode(convert_to('café', 'UTF8'), 'hex') = '636166c3a9', convert_from(decode('636166c3a9', 'hex'), 'UTF8') = 'café', encode(convert(decode('636166c3a9', 'hex'), 'UTF8', 'LATIN1'), 'hex') = '636166e9', convert_from(decode('636166e9', 'hex'), 'LATIN1') = 'café'",
    );
}

#[test]
fn should_measure_octets_in_utf8_bytea() {
    // Arrange
    let fixture = sql_fixture("issue-816-bytea-octet-length", &[]);

    // Act
    // Assert
    assert_true(
        &fixture,
        "SELECT octet_length(convert_to('café', 'UTF8')) = 5",
    );
}

#[test]
fn should_keep_encoding_choice_visible_in_bytea() {
    // Arrange
    let fixture = sql_fixture("issue-816-convert-to-encoding-identity", &[]);

    // Act
    // Assert
    assert_true(
        &fixture,
        "SELECT convert_to('café', 'UTF8') <> convert_to('café', 'LATIN1')",
    );
}

#[test]
fn should_reject_malformed_bytea_input_forms() {
    // Arrange
    let fixture = sql_fixture("issue-816-bytea-invalid-input", &[]);

    // Act
    let odd_hex = fixture.execute(r"SELECT '\xabc'::bytea");
    let malformed_hex = fixture.execute(r"SELECT '\x0g'::bytea");
    let malformed_escape = fixture.execute(r"SELECT '\12'::bytea");

    // Assert
    assert!(odd_hex.is_err(), "odd hex input must fail");
    assert!(malformed_hex.is_err(), "non-hex input must fail");
    assert!(malformed_escape.is_err(), "short octal escape must fail");
}

#[test]
fn should_reject_malformed_bytea_decoding_formats() {
    // Arrange
    let fixture = sql_fixture("issue-816-bytea-decode-invalid", &[]);

    // Act
    let odd_hex = fixture.execute("SELECT decode('abc', 'hex')");
    let bad_hex = fixture.execute("SELECT decode('0g', 'hex')");
    let bad_base64 = fixture.execute("SELECT decode('MTIzAAE==', 'base64')");
    let bad_escape = fixture.execute(r"SELECT decode('\1', 'escape')");

    // Assert
    assert!(odd_hex.is_err(), "odd-length hex input must fail");
    assert!(bad_hex.is_err(), "invalid hex digits must fail");
    assert!(bad_base64.is_err(), "incorrect base64 padding must fail");
    assert!(bad_escape.is_err(), "incomplete escape input must fail");
}

#[test]
fn should_reject_bytea_access_out_of_bounds() {
    // Arrange
    let fixture = sql_fixture("issue-816-binary-access-bounds", &[]);

    // Act
    let byte_read = fixture.execute(r"SELECT get_byte('\x00'::bytea, 1)");
    let byte_write = fixture.execute(r"SELECT set_byte('\x00'::bytea, -1, 0)");
    let bit_read = fixture.execute(r"SELECT get_bit('\x00'::bytea, 8)");
    let bit_write = fixture.execute(r"SELECT set_bit('\x00'::bytea, -1, 0)");

    // Assert
    assert!(byte_read.is_err(), "byte read outside value must fail");
    assert!(byte_write.is_err(), "byte write outside value must fail");
    assert!(bit_read.is_err(), "bit read outside value must fail");
    assert!(bit_write.is_err(), "bit write outside value must fail");
}

#[test]
fn should_reject_bit_updates_outside_the_value_domain() {
    // Arrange
    let fixture = sql_fixture("issue-816-bit-access-bounds", &[]);

    // Act
    let read = fixture.execute("SELECT get_bit(B'101', 3)");
    let write = fixture.execute("SELECT set_bit(B'101', 3, 1)");
    let invalid_value = fixture.execute("SELECT set_bit(B'101', 1, 2)");

    // Assert
    assert!(read.is_err(), "bit read outside value must fail");
    assert!(write.is_err(), "bit write outside value must fail");
    assert!(
        invalid_value.is_err(),
        "bit update value outside zero or one must fail"
    );
}

#[test]
fn should_cast_integers_to_network_order_bytea() {
    // Arrange
    let fixture = sql_fixture("issue-816-integer-to-bytea", &[]);

    // Act
    // Assert
    assert_true(
        &fixture,
        "SELECT encode(1234::smallint::bytea, 'hex') = '04d2', encode(CAST(1234 AS integer)::bytea, 'hex') = '000004d2', encode(CAST(-1234 AS integer)::bytea, 'hex') = 'fffffb2e', encode(CAST(-1 AS bigint)::bytea, 'hex') = 'ffffffffffffffff'",
    );
}

#[test]
fn should_preserve_signed_values_when_casting_network_order_bytea() {
    // Arrange
    let fixture = sql_fixture("issue-816-bytea-to-integer", &[]);

    // Act
    // Assert
    assert_true(
        &fixture,
        r"SELECT '\x8000'::bytea::smallint = -32768, '\x8000'::bytea::integer = 32768, '\xfffffb2e'::bytea::integer = -1234",
    );
}

#[test]
fn should_reject_bytea_integer_casts_wider_than_the_target_type() {
    // Arrange
    let fixture = sql_fixture("issue-816-bytea-integer-cast-bounds", &[]);

    // Act
    let smallint = fixture.execute(r"SELECT '\x000001'::bytea::smallint");
    let integer = fixture.execute(r"SELECT '\x0000000001'::bytea::integer");

    // Assert
    assert!(smallint.is_err(), "bytea longer than smallint must fail");
    assert!(integer.is_err(), "bytea longer than integer must fail");
}
