//! PostgreSQL 18 string scalar wire contracts tracked by #803.

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

#[path = "../string_format.rs"]
mod format_residuals;

fn error_fields(frames: &[(u8, Vec<u8>)]) -> Vec<Vec<(char, String)>> {
    frames
        .iter()
        .filter(|(tag, _)| *tag == b'E')
        .map(|(_, payload)| wire::parse_error_fields(payload))
        .collect()
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
fn should_reject_casefold_text_under_an_icu_collation() {
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
    assert_eq!(wire::error_code(result).as_deref(), Some("0A000"));
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
