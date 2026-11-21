use super::output::{encode_header, encode_row};
use super::request::{parse_copy_request, CopyRequest};
use crate::executor::ColumnMeta;
use crate::types::Value;

fn request(options: &str) -> CopyRequest {
    parse_copy_request(&format!("COPY (SELECT 1) TO STDOUT {options}"))
        .expect("valid COPY")
        .expect("COPY request")
}

fn columns(count: usize) -> Vec<ColumnMeta> {
    (0..count)
        .map(|index| ColumnMeta::text(format!("c{index}")))
        .collect()
}

fn strings(values: &[&str]) -> Vec<Value> {
    values
        .iter()
        .map(|value| Value::String((*value).into()))
        .collect()
}

fn encode(options: &str, values: &[&str]) -> String {
    let bytes = encode_row(&request(options), &columns(values.len()), &strings(values))
        .expect("encode row");
    String::from_utf8(bytes).expect("UTF-8 output")
}

#[test]
fn should_escape_custom_delimiter_inside_text_values() {
    // Arrange
    let options = "(DELIMITER '|')";

    // Act
    let line = encode(options, &["x|y", "q"]);

    // Assert
    assert_eq!(line, "x\\|y|q\n");
}

#[test]
fn should_escape_tab_inside_text_values_under_custom_delimiter() {
    // Arrange
    let options = "(DELIMITER '|')";

    // Act
    let line = encode(options, &["x\ty", "q"]);

    // Assert
    assert_eq!(line, "x\\ty|q\n");
}

#[test]
fn should_write_other_control_characters_raw_in_text_values() {
    // Arrange
    let options = "";

    // Act
    let line = encode(options, &["a\u{1}b\\c"]);

    // Assert
    assert_eq!(line, "a\u{1}b\\\\c\n");
}

#[test]
fn should_use_custom_quote_character_in_csv_output() {
    // Arrange
    let options = "(FORMAT CSV, QUOTE '''')";

    // Act
    let line = encode(options, &["it's"]);

    // Assert
    assert_eq!(line, "'it''s'\n");
}

#[test]
fn should_not_quote_double_quote_when_quote_character_is_custom() {
    // Arrange
    let options = "(FORMAT CSV, QUOTE '''')";

    // Act
    let line = encode(options, &["say \"hi\"", "plain"]);

    // Assert
    assert_eq!(line, "say \"hi\",plain\n");
}

#[test]
fn should_use_custom_escape_character_in_csv_output() {
    // Arrange
    let options = "(FORMAT CSV, ESCAPE '\\')";

    // Act
    let line = encode(options, &["say \"hi\""]);

    // Assert
    assert_eq!(line, "\"say \\\"hi\\\"\"\n");
}

#[test]
fn should_escape_the_escape_character_inside_quoted_csv_values() {
    // Arrange
    let options = "(FORMAT CSV, ESCAPE '\\')";

    // Act
    let line = encode(options, &["a\\b\"", "c"]);

    // Assert
    assert_eq!(line, "\"a\\\\b\\\"\",c\n");
}

#[test]
fn should_not_quote_value_that_only_contains_the_escape_character() {
    // Arrange
    let options = "(FORMAT CSV, ESCAPE '\\')";

    // Act
    let line = encode(options, &["a\\b"]);

    // Assert
    assert_eq!(line, "a\\b\n");
}

#[test]
fn should_quote_csv_values_that_contain_delimiter_quote_or_line_breaks() {
    // Arrange
    let options = "(FORMAT CSV, DELIMITER ';')";

    // Act
    let line = encode(options, &["a;b", "q\"x", "n\nx", "r\rx", ",", " pad "]);

    // Assert
    assert_eq!(line, "\"a;b\";\"q\"\"x\";\"n\nx\";\"r\rx\";,; pad \n");
}

#[test]
fn should_quote_csv_value_equal_to_the_null_string() {
    // Arrange
    let options = "(FORMAT CSV, NULL 'NULL')";

    // Act
    let line = encode(options, &["NULL", "", "x"]);

    // Assert
    assert_eq!(line, "\"NULL\",,x\n");
}

#[test]
fn should_quote_empty_csv_value_when_the_null_string_is_empty() {
    // Arrange
    let options = "(FORMAT CSV)";

    // Act
    let line = encode(options, &["", "x"]);

    // Assert
    assert_eq!(line, "\"\",x\n");
}

#[test]
fn should_quote_end_marker_text_in_a_single_column_csv_row() {
    // Arrange
    let options = "(FORMAT CSV)";

    // Act
    let single = encode(options, &["\\."]);
    let multiple = encode(options, &["x", "\\."]);

    // Assert
    assert_eq!(single, "\"\\.\"\n");
    assert_eq!(multiple, "x,\\.\n");
}

#[test]
fn should_apply_force_quote_to_named_csv_columns() {
    // Arrange
    let options = "(FORMAT CSV, FORCE_QUOTE (c1))";

    // Act
    let line = encode(options, &["a", "b"]);

    // Assert
    assert_eq!(line, "a,\"b\"\n");
}

#[test]
fn should_quote_csv_header_with_custom_quote_character() {
    // Arrange
    let request = request("(FORMAT CSV, HEADER, QUOTE '''')");
    let columns = vec![ColumnMeta::text("it's"), ColumnMeta::text("b")];

    // Act
    let header = encode_header(&request, &columns).expect("header");

    // Assert
    assert_eq!(header, b"'it''s',b\n");
}

fn roundtrip_values() -> Vec<&'static str> {
    vec![
        "plain",
        "a|b",
        "a,b",
        "q\"uote",
        "it's",
        "back\\slash",
        "new\nline",
        "carriage\rreturn",
        "tab\tchar",
        "\\.",
        "\\N",
        "\u{e9}t\u{e9}",
        " padded ",
        "",
    ]
}

fn decode_text(options: &str, payload: &[u8]) -> Vec<Vec<Option<String>>> {
    let from = parse_copy_request(&format!("COPY t FROM STDIN {options}"))
        .expect("valid COPY")
        .expect("COPY request");
    let controls = crate::runtime::QueryExecutionControls::from_limits(
        &crate::config::CassieRuntimeLimits::default(),
        std::time::Instant::now(),
    );
    super::input::parse_text_payload(
        payload,
        from.text_delimiter(),
        from.null_marker(),
        &controls,
    )
    .expect("decode text")
    .0
}

fn decode_csv(options: &str, payload: &[u8]) -> Vec<Vec<Option<String>>> {
    let from = parse_copy_request(&format!("COPY t FROM STDIN {options}"))
        .expect("valid COPY")
        .expect("COPY request");
    let controls = crate::runtime::QueryExecutionControls::from_limits(
        &crate::config::CassieRuntimeLimits::default(),
        std::time::Instant::now(),
    );
    crate::app::parse_copy_csv_payload_controlled(
        payload,
        crate::app::CopyCsvOptions {
            delimiter: from.csv_delimiter(),
            quote: from.quote,
            escape: from.escape,
            null_marker: from.null_marker(),
            force_null: &[],
            force_not_null: &[],
        },
        from.null_marker().is_empty(),
        &controls,
    )
    .expect("decode CSV")
    .0
}

fn roundtrip(options: &str, csv: bool, values: &[&str]) -> Vec<Vec<Option<String>>> {
    let mut payload = encode(options, values).into_bytes();
    payload.extend(encode(options, &values[..1]).into_bytes());
    if csv {
        decode_csv(options, &payload)
    } else {
        decode_text(options, &payload)
    }
}

fn expected(values: &[&str]) -> Vec<Vec<Option<String>>> {
    vec![
        values.iter().map(|value| Some((*value).into())).collect(),
        vec![Some(values[0].into())],
    ]
}

#[test]
fn should_roundtrip_values_through_default_text_copy() {
    // Arrange
    let values = roundtrip_values();

    // Act
    let rows = roundtrip("", false, &values);

    // Assert
    assert_eq!(rows, expected(&values));
}

#[test]
fn should_roundtrip_values_through_text_copy_with_pipe_delimiter() {
    // Arrange
    let values = roundtrip_values();

    // Act
    let rows = roundtrip("(DELIMITER '|')", false, &values);

    // Assert
    assert_eq!(rows, expected(&values));
}

#[test]
fn should_roundtrip_each_value_alone_through_text_copy() {
    // Arrange
    let values = roundtrip_values();

    // Act
    let rows: Vec<_> = values
        .iter()
        .map(|value| roundtrip("(DELIMITER '|')", false, &[value, "z"]))
        .collect();

    // Assert
    for (row, value) in rows.iter().zip(&values) {
        assert_eq!(row, &expected(&[value, "z"]));
    }
}

#[test]
fn should_roundtrip_values_through_default_csv_copy() {
    // Arrange
    let values = roundtrip_values();

    // Act
    let rows = roundtrip("(FORMAT CSV)", true, &values);

    // Assert
    assert_eq!(rows, expected(&values));
}

#[test]
fn should_roundtrip_values_through_csv_copy_with_custom_quote() {
    // Arrange
    let values = roundtrip_values();

    // Act
    let rows = roundtrip("(FORMAT CSV, QUOTE '''', DELIMITER '|')", true, &values);

    // Assert
    assert_eq!(rows, expected(&values));
}

#[test]
fn should_roundtrip_values_through_csv_copy_with_custom_escape() {
    // Arrange
    let values = roundtrip_values();

    // Act
    let rows = roundtrip("(FORMAT CSV, ESCAPE '\\')", true, &values);

    // Assert
    assert_eq!(rows, expected(&values));
}

#[test]
fn should_roundtrip_the_null_string_through_csv_copy() {
    // Arrange
    let values = ["NULL", "x"];

    // Act
    let rows = roundtrip("(FORMAT CSV, NULL 'NULL')", true, &values);

    // Assert
    assert_eq!(rows, expected(&values));
}

#[test]
fn should_roundtrip_single_column_end_marker_through_csv_copy() {
    // Arrange
    let options = "(FORMAT CSV)";
    let payload = encode(options, &["\\."]).into_bytes();

    // Act
    let rows = decode_csv(options, &payload);

    // Assert
    assert_eq!(rows, vec![vec![Some("\\.".to_string())]]);
}

#[test]
fn should_roundtrip_escape_character_before_quote_through_csv_copy() {
    // Arrange
    let values = ["a\\b\"", "c\\\\d\"e"];

    // Act
    let rows = roundtrip("(FORMAT CSV, ESCAPE '\\')", true, &values);

    // Assert
    assert_eq!(rows, expected(&values));
}
