//! PostgreSQL 18 string scalar wire contracts tracked by #803.

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

fn error_fields(frames: &[(u8, Vec<u8>)]) -> Vec<Vec<(char, String)>> {
    frames
        .iter()
        .filter(|(tag, _)| *tag == b'E')
        .map(|(_, payload)| wire::parse_error_fields(payload))
        .collect()
}

#[test]
fn should_bind_to_bin_from_int8_binary_parameter() {
    // Arrange
    let value = 42_i64.to_be_bytes();
    let sql = "SELECT to_bin($1) AS bits";
    let steps = vec![
        Step::ParseFlush(wire::parse_frame_with_types("to_bin_i8", sql, &[20])),
        Step::ThroughReady(vec![
            wire::describe_statement_frame("to_bin_i8"),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "to_bin_portal",
                "to_bin_i8",
                &[1],
                &[Some(&value)],
                &[1],
            ),
            wire::describe_portal_frame("to_bin_portal"),
            wire::execute_frame("to_bin_portal"),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("issue-803-to-bin-binary-wire", steps);

    // Assert
    assert_eq!(
        wire::error_code(&batches[0]),
        None,
        "to_bin Parse errors: {:?}",
        error_fields(&batches[0])
    );
    assert_eq!(
        wire::error_code(&batches[1]),
        None,
        "to_bin Describe errors: {:?}",
        error_fields(&batches[1])
    );
    let statement_description = batches[1]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("to_bin statement RowDescription");
    assert_eq!(
        wire::parse_row_description(&statement_description.1)[0].type_oid,
        25
    );
    let result_description = batches[2]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("to_bin portal RowDescription");
    let result_column = &wire::parse_row_description(&result_description.1)[0];
    assert_eq!(result_column.type_oid, 25);
    assert_eq!(result_column.format_code, 1);
    assert_eq!(
        wire::error_code(&batches[2]),
        None,
        "to_bin Bind/Execute errors: {:?}",
        error_fields(&batches[2])
    );
    assert_eq!(
        wire::data_rows(&batches[2]),
        vec![vec![Some("101010".to_owned())]]
    );
}

#[test]
fn should_bind_negative_int8_to_radix_text_functions_with_binary_results() {
    // Arrange
    let value = (-1_i64).to_be_bytes();
    let sql =
        "SELECT to_hex($1) AS hex_value, to_bin($1) AS binary_value, to_oct($1) AS octal_value";
    let batches = run_script(
        "issue-803-negative-int8-radix-functions-wire",
        vec![
            Step::ParseFlush(wire::parse_frame_with_types(
                "negative_int8_radix",
                sql,
                &[20],
            )),
            Step::ThroughReady(vec![
                wire::describe_statement_frame("negative_int8_radix"),
                wire::sync_frame(),
            ]),
            Step::ThroughReady(vec![
                wire::bind_frame_with_formats(
                    "negative_int8_radix_portal",
                    "negative_int8_radix",
                    &[1],
                    &[Some(&value)],
                    &[1, 1, 1],
                ),
                wire::describe_portal_frame("negative_int8_radix_portal"),
                wire::execute_frame("negative_int8_radix_portal"),
                wire::sync_frame(),
            ]),
        ],
    );

    // Act
    let described = &batches[1];
    let executed = &batches[2];

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    assert_eq!(wire::error_code(described), None);
    assert_eq!(wire::error_code(executed), None);
    assert_eq!(
        wire::parse_parameter_description(
            &described
                .iter()
                .find(|(tag, _)| *tag == b't')
                .expect("negative radix ParameterDescription")
                .1
        ),
        vec![20]
    );
    let columns = wire::parse_row_description(
        &executed
            .iter()
            .find(|(tag, _)| *tag == b'T')
            .expect("negative radix portal RowDescription")
            .1,
    );
    assert_eq!(
        columns
            .iter()
            .map(|column| (column.type_oid, column.format_code))
            .collect::<Vec<_>>(),
        vec![(25, 1), (25, 1), (25, 1)]
    );
    assert_eq!(
        wire::data_rows(executed),
        vec![vec![
            Some("ffffffffffffffff".to_owned()),
            Some("1111111111111111111111111111111111111111111111111111111111111111".to_owned()),
            Some("1777777777777777777777".to_owned()),
        ]]
    );
}

#[test]
fn should_return_character_length_with_postgresql_int4_metadata() {
    // Arrange
    let batches = run_script(
        "issue-803-char-length-wire",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT char_length('aé') AS characters",
        )])],
    );

    // Act
    let result = &batches[0];

    // Assert
    assert_eq!(
        wire::error_code(result),
        None,
        "char_length errors: {:?}",
        error_fields(result)
    );
    let columns = wire::parse_row_description(
        &result
            .iter()
            .find(|(tag, _)| *tag == b'T')
            .expect("char_length RowDescription")
            .1,
    );
    assert_eq!(columns[0].type_oid, 23);
    assert_eq!(wire::data_rows(result), vec![vec![Some("2".to_owned())]]);
}

#[test]
fn should_bind_utf8_parameter_for_binary_int4_character_length() {
    // Arrange
    let text = "Aé🙂Z".as_bytes();
    let batches = run_script(
        "issue-803-char-length-binary-result",
        vec![
            Step::ParseFlush(wire::parse_frame_with_types(
                "char_length_binary",
                "SELECT char_length($1) AS characters",
                &[25],
            )),
            Step::ThroughReady(vec![
                wire::describe_statement_frame("char_length_binary"),
                wire::sync_frame(),
            ]),
            Step::ThroughReady(vec![
                wire::bind_frame_with_formats(
                    "char_length_binary_portal",
                    "char_length_binary",
                    &[1],
                    &[Some(text)],
                    &[1],
                ),
                wire::describe_portal_frame("char_length_binary_portal"),
                wire::execute_frame("char_length_binary_portal"),
                wire::sync_frame(),
            ]),
        ],
    );

    // Act
    let described = &batches[1];
    let executed = &batches[2];

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    assert_eq!(wire::error_code(described), None);
    assert_eq!(wire::error_code(executed), None);
    assert_eq!(
        wire::parse_parameter_description(
            &described
                .iter()
                .find(|(tag, _)| *tag == b't')
                .expect("char_length ParameterDescription")
                .1
        ),
        vec![25]
    );
    let column = wire::parse_row_description(
        &executed
            .iter()
            .find(|(tag, _)| *tag == b'T')
            .expect("char_length portal RowDescription")
            .1,
    )
    .remove(0);
    assert_eq!(column.type_oid, 23);
    assert_eq!(column.format_code, 1);
    assert_eq!(
        executed
            .iter()
            .find(|(tag, _)| *tag == b'D')
            .expect("binary int4 DataRow")
            .1,
        vec![0, 1, 0, 0, 0, 4, 0, 0, 0, 4]
    );
}

#[test]
fn should_report_int4_metadata_for_octet_bit_lengths() {
    // Arrange
    let batches = run_script(
        "issue-803-octet-and-bit-length-wire",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT octet_length('aé') AS bytes, bit_length('aé') AS bits",
        )])],
    );

    // Act
    let result = &batches[0];

    // Assert
    assert_eq!(
        wire::error_code(result),
        None,
        "octet_length and bit_length errors: {:?}",
        error_fields(result)
    );
    let columns = wire::parse_row_description(
        &result
            .iter()
            .find(|(tag, _)| *tag == b'T')
            .expect("length functions RowDescription")
            .1,
    );
    assert_eq!(
        columns
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![23, 23]
    );
    assert_eq!(
        wire::data_rows(result),
        vec![vec![Some("3".to_owned()), Some("24".to_owned())]]
    );
}

#[test]
fn should_preserve_text_transform_values_with_postgresql_metadata() {
    // Arrange
    let batches = run_script(
        "issue-803-text-transform-metadata",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT concat_ws('-', 'a', NULL, 'c') AS joined, translate('abcdef', 'bd', 'XY') AS translated, overlay('Txxxxas' PLACING 'hom' FROM 2 FOR 4) AS overlaid",
        )])],
    );

    // Act
    let result = &batches[0];

    // Assert
    assert_eq!(
        wire::error_code(result),
        None,
        "text transformation errors: {:?}",
        error_fields(result)
    );
    let description = result
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("text transformation RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![25, 25, 25]
    );
    assert_eq!(
        wire::data_rows(result),
        vec![vec![
            Some("a-c".to_owned()),
            Some("aXcYef".to_owned()),
            Some("Thomas".to_owned()),
        ]]
    );
}

#[test]
fn should_return_unicode_assigned_with_postgresql_boolean_metadata() {
    // Arrange
    let batches = run_script(
        "issue-803-unicode-assigned-wire",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT unicode_assigned('A') AS assigned",
        )])],
    );

    // Act
    let result = &batches[0];

    // Assert
    assert_eq!(
        wire::error_code(result),
        None,
        "unicode_assigned errors: {:?}",
        error_fields(result)
    );
    let columns = wire::parse_row_description(
        &result
            .iter()
            .find(|(tag, _)| *tag == b'T')
            .expect("unicode_assigned RowDescription")
            .1,
    );
    assert_eq!(columns[0].type_oid, 16);
    assert_eq!(wire::data_rows(result), vec![vec![Some("t".to_owned())]]);
}

#[test]
fn should_decode_unistr_with_postgresql_text_metadata() {
    // Arrange
    let batches = run_script(
        "issue-803-unistr-wire",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(
            r"SELECT unistr('\0061\+01F600') AS decoded",
        )])],
    );

    // Act
    let result = &batches[0];

    // Assert
    assert_eq!(
        wire::error_code(result),
        None,
        "unistr errors: {:?}",
        error_fields(result)
    );
    let columns = wire::parse_row_description(
        &result
            .iter()
            .find(|(tag, _)| *tag == b'T')
            .expect("unistr RowDescription")
            .1,
    );
    assert_eq!(columns[0].type_oid, 25);
    assert_eq!(wire::data_rows(result), vec![vec![Some("a😀".to_owned())]]);
}

#[test]
fn should_decode_each_unistr_escape_form_with_postgresql_text_metadata() {
    // Arrange
    let batches = run_script(
        "issue-803-unistr-escape-forms-wire",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(
            r"SELECT unistr('\0061') AS four_hex, unistr('\+01F600') AS six_hex, unistr('\u0062') AS four_unicode, unistr('\U0001F603') AS eight_unicode, unistr('\\') AS escaped_backslash",
        )])],
    );

    // Act
    let result = &batches[0];

    // Assert
    assert_eq!(
        wire::error_code(result),
        None,
        "unistr escape errors: {:?}",
        error_fields(result)
    );
    let description = result
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("unistr escape RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![25, 25, 25, 25, 25]
    );
    assert_eq!(
        wire::data_rows(result),
        vec![vec![
            Some("a".to_owned()),
            Some("😀".to_owned()),
            Some("b".to_owned()),
            Some("😃".to_owned()),
            Some("\\".to_owned()),
        ]]
    );
}

#[test]
fn should_report_invalid_surrogate_pairs_in_unistr_with_42601() {
    // Arrange
    let batches = run_script(
        "issue-803-unistr-surrogate-sqlstate",
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT unistr(chr(92)||'u0061')",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT unistr(chr(92)||'uD800')",
            )]),
        ],
    );

    // Act
    let valid = &batches[0];
    let invalid = &batches[1];

    // Assert
    assert_eq!(wire::error_code(valid), None);
    assert_eq!(wire::data_rows(valid), vec![vec![Some("a".to_owned())]]);
    assert_eq!(wire::error_code(invalid).as_deref(), Some("42601"));
    let message = error_fields(invalid)
        .into_iter()
        .flatten()
        .find_map(|(field, message)| (field == 'M').then_some(message))
        .expect("invalid unistr surrogate ErrorResponse message");
    assert_eq!(message, "invalid Unicode surrogate pair");
}

#[test]
fn should_normalize_text_with_postgresql_text_metadata() {
    // Arrange
    let batches = run_script(
        "issue-803-normalize-wire",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT normalize('e\u{301}') AS normalized",
        )])],
    );

    // Act
    let result = &batches[0];

    // Assert
    assert_eq!(
        wire::error_code(result),
        None,
        "normalize errors: {:?}",
        error_fields(result)
    );
    let columns = wire::parse_row_description(
        &result
            .iter()
            .find(|(tag, _)| *tag == b'T')
            .expect("normalize RowDescription")
            .1,
    );
    assert_eq!(columns[0].type_oid, 25);
    assert_eq!(wire::data_rows(result), vec![vec![Some("é".to_owned())]]);
}

#[test]
fn should_return_negated_normalization_predicates_with_postgresql_boolean_metadata() {
    // Arrange
    let batches = run_script(
        "issue-803-normalization-predicate-wire",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(
            r"SELECT U&'e\0301' IS NOT NFC NORMALIZED AS not_nfc, U&'\00E9' IS NOT NFD NORMALIZED AS not_nfd, U&'e\0301' IS NOT NFD NORMALIZED AS nfd_input, NULL::TEXT IS NOT NFC NORMALIZED AS null_input",
        )])],
    );

    // Act
    let result = &batches[0];

    // Assert
    assert_eq!(
        wire::error_code(result),
        None,
        "normalization predicate errors: {:?}",
        error_fields(result)
    );
    let description = result
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("normalization predicate RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![16, 16, 16, 16]
    );
    assert_eq!(
        wire::data_rows(result),
        vec![vec![
            Some("t".to_owned()),
            Some("t".to_owned()),
            Some("f".to_owned()),
            None,
        ]]
    );
}

#[test]
fn should_casefold_text_with_postgresql_text_metadata() {
    // Arrange
    let batches = run_script(
        "issue-803-casefold-wire",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT casefold('Straße') AS folded",
        )])],
    );

    // Act
    let result = &batches[0];

    // Assert
    assert_eq!(
        wire::error_code(result),
        None,
        "casefold errors: {:?}",
        error_fields(result)
    );
    let columns = wire::parse_row_description(
        &result
            .iter()
            .find(|(tag, _)| *tag == b'T')
            .expect("casefold RowDescription")
            .1,
    );
    assert_eq!(columns[0].type_oid, 25);
    assert_eq!(
        wire::data_rows(result),
        vec![vec![Some("straße".to_owned())]]
    );
}

#[test]
fn should_casefold_text_under_an_icu_collation() {
    // Arrange
    let batches = run_script(
        "issue-803-casefold-wire",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT casefold('Straße' COLLATE \"und-x-icu\") AS folded",
        )])],
    );

    // Act
    let result = &batches[0];

    // Assert
    assert_eq!(
        wire::error_code(result),
        None,
        "casefold errors: {:?}",
        error_fields(result)
    );
    let columns = wire::parse_row_description(
        &result
            .iter()
            .find(|(tag, _)| *tag == b'T')
            .expect("casefold RowDescription")
            .1,
    );
    assert_eq!(columns[0].type_oid, 25);
    assert_eq!(
        wire::data_rows(result),
        vec![vec![Some("strasse".to_owned())]]
    );
}

#[test]
fn should_return_split_part_with_postgresql_text_metadata() {
    // Arrange
    let batches = run_script(
        "issue-803-split-part-wire",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT split_part('one,two,three', ',', 2) AS part",
        )])],
    );

    // Act
    let result = &batches[0];

    // Assert
    assert_eq!(
        wire::error_code(result),
        None,
        "split_part errors: {:?}",
        error_fields(result)
    );
    let description = result
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("split_part RowDescription");
    assert_eq!(wire::parse_row_description(&description.1)[0].type_oid, 25);
    assert_eq!(wire::data_rows(result), vec![vec![Some("two".to_owned())]]);
}

#[test]
fn should_enforce_parse_ident_strictness_with_postgresql_sqlstate() {
    // Arrange
    let batches = run_script(
        "issue-803-parse-ident-strictness",
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT parse_ident('foo.bar junk', false)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT parse_ident('foo.bar junk')",
            )]),
        ],
    );

    // Act
    let permissive = &batches[0];
    let strict = &batches[1];

    // Assert
    assert_eq!(wire::error_code(permissive), None);
    let description = permissive
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("parse_ident RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)[0].type_oid,
        1_009
    );
    assert_eq!(
        wire::data_rows(permissive),
        vec![vec![Some("{foo,bar}".to_owned())]]
    );
    assert_eq!(wire::error_code(strict).as_deref(), Some("22023"));
    let message = error_fields(strict)
        .into_iter()
        .flatten()
        .find_map(|(field, message)| (field == 'M').then_some(message))
        .expect("strict parse_ident ErrorResponse message");
    assert_eq!(
        message,
        "string is not a valid identifier: \"foo.bar junk\""
    );
}

#[test]
fn should_report_postgresql_character_codepoint_sqlstates_for_chr() {
    // Arrange
    let batches = run_script(
        "issue-803-chr-codepoint-sqlstates",
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame("SELECT chr(65)")]),
            Step::ThroughReady(vec![wire::simple_query_frame("SELECT chr(0)")]),
            Step::ThroughReady(vec![wire::simple_query_frame("SELECT chr(-1)")]),
        ],
    );

    // Act
    let valid = &batches[0];
    let zero = &batches[1];
    let negative = &batches[2];

    // Assert
    assert_eq!(wire::error_code(valid), None);
    assert_eq!(wire::data_rows(valid), vec![vec![Some("A".to_owned())]]);
    assert_eq!(wire::error_code(zero).as_deref(), Some("54000"));
    assert_eq!(wire::error_code(negative).as_deref(), Some("22023"));
}

#[test]
fn should_return_format_output_with_postgresql_text_metadata() {
    // Arrange
    let batches = run_script(
        "issue-803-format-wire",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT format('%s:%s', 'item', 42) AS formatted",
        )])],
    );

    // Act
    let result = &batches[0];

    // Assert
    assert_eq!(
        wire::error_code(result),
        None,
        "format errors: {:?}",
        error_fields(result)
    );
    let description = result
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("format RowDescription");
    assert_eq!(wire::parse_row_description(&description.1)[0].type_oid, 25);
    assert_eq!(
        wire::data_rows(result),
        vec![vec![Some("item:42".to_owned())]]
    );
}

#[test]
fn should_return_starts_with_as_a_postgresql_boolean_wire_value() {
    // Arrange
    let batches = run_script(
        "issue-803-starts-with-wire",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT starts_with('alphabet', 'alpha') AS matches",
        )])],
    );

    // Act
    let result = &batches[0];

    // Assert
    assert_eq!(
        wire::error_code(result),
        None,
        "starts_with errors: {:?}",
        error_fields(result)
    );
    let description = result
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("starts_with RowDescription");
    assert_eq!(wire::parse_row_description(&description.1)[0].type_oid, 16);
    assert_eq!(wire::data_rows(result), vec![vec![Some("t".to_owned())]]);
}

fn run_null_count(function_name: &str) -> Vec<Vec<Option<String>>> {
    let number = 7_i32.to_be_bytes();
    let text = b"x";
    let sql = format!("SELECT {function_name}($1::INT4, $2::TEXT, $3::TEXT) AS argument_count");
    let batches = run_script(
        &format!("issue-803-{function_name}-binary-bind"),
        vec![
            Step::ParseFlush(wire::parse_frame_with_types(
                "variadic_null_count",
                &sql,
                &[23, 25, 25],
            )),
            Step::ThroughReady(vec![
                wire::describe_statement_frame("variadic_null_count"),
                wire::sync_frame(),
            ]),
            Step::ThroughReady(vec![
                wire::bind_frame_with_formats(
                    "variadic_null_count_portal",
                    "variadic_null_count",
                    &[1, 1, 1],
                    &[Some(&number), None, Some(text)],
                    &[0],
                ),
                wire::describe_portal_frame("variadic_null_count_portal"),
                wire::execute_frame("variadic_null_count_portal"),
                wire::sync_frame(),
            ]),
        ],
    );

    assert_eq!(
        wire::error_code(&batches[0]),
        None,
        "Parse errors: {:?}",
        error_fields(&batches[0])
    );
    let described = &batches[1];
    let executed = &batches[2];
    assert_eq!(
        wire::error_code(described),
        None,
        "Describe errors: {:?}",
        error_fields(described)
    );
    assert_eq!(
        wire::error_code(executed),
        None,
        "Bind/Execute errors: {:?}",
        error_fields(executed)
    );
    let parameters = described
        .iter()
        .find(|(tag, _)| *tag == b't')
        .expect("ParameterDescription");
    assert_eq!(
        wire::parse_parameter_description(&parameters.1),
        vec![23, 25, 25]
    );
    let columns = executed
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("portal RowDescription");
    assert_eq!(
        wire::parse_row_description(&columns.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![23]
    );
    wire::data_rows(executed)
}

#[test]
fn should_count_null_arguments_from_typed_binary_parameters() {
    // Arrange
    let expected_rows = vec![vec![Some("1".to_owned())]];

    // Act
    let rows = run_null_count("num_nulls");

    // Assert
    assert_eq!(rows, expected_rows);
}

#[test]
fn should_count_nonnull_arguments_from_typed_binary_parameters() {
    // Arrange
    let expected_rows = vec![vec![Some("2".to_owned())]];

    // Act
    let rows = run_null_count("num_nonnulls");

    // Assert
    assert_eq!(rows, expected_rows);
}
