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
fn should_return_octet_length_with_postgresql_int4_metadata() {
    // Arrange
    let batches = run_script(
        "issue-803-octet-length-wire",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT octet_length('aé') AS bytes",
        )])],
    );

    // Act
    let result = &batches[0];

    // Assert
    assert_eq!(
        wire::error_code(result),
        None,
        "octet_length errors: {:?}",
        error_fields(result)
    );
    let columns = wire::parse_row_description(
        &result
            .iter()
            .find(|(tag, _)| *tag == b'T')
            .expect("octet_length RowDescription")
            .1,
    );
    assert_eq!(columns[0].type_oid, 23);
    assert_eq!(wire::data_rows(result), vec![vec![Some("3".to_owned())]]);
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
