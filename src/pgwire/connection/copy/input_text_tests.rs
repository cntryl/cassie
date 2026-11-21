use super::input::parse_text_payload;
use crate::config::CassieRuntimeLimits;
use crate::runtime::QueryExecutionControls;
use std::time::Instant;

type Rows = Vec<Vec<Option<String>>>;

fn parse(payload: &[u8], delimiter: char, null_marker: &str) -> Result<Rows, String> {
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    parse_text_payload(payload, delimiter, null_marker, &controls)
        .map(|(rows, _memory)| rows)
        .map_err(|error| error.to_string())
}

fn some(values: &[&str]) -> Vec<Option<String>> {
    values.iter().map(|value| Some((*value).into())).collect()
}

#[test]
fn should_keep_backslash_escaped_tab_inside_one_field() {
    // Arrange
    let payload = b"x\\\ty\tz\n";

    // Act
    let rows = parse(payload, '\t', "\\N").expect("parse");

    // Assert
    assert_eq!(rows, vec![some(&["x\ty", "z"])]);
}

#[test]
fn should_keep_backslash_escaped_newline_inside_one_row() {
    // Arrange
    let payload = b"a\\\nb\tc\n";

    // Act
    let rows = parse(payload, '\t', "\\N").expect("parse");

    // Assert
    assert_eq!(rows, vec![some(&["a\nb", "c"])]);
}

#[test]
fn should_keep_backslash_escaped_custom_delimiter_inside_one_field() {
    // Arrange
    let payload = b"x\\|y|z\n";

    // Act
    let rows = parse(payload, '|', "\\N").expect("parse");

    // Assert
    assert_eq!(rows, vec![some(&["x|y", "z"])]);
}

#[test]
fn should_treat_raw_tab_as_data_under_custom_delimiter() {
    // Arrange
    let payload = b"x\ty|z\n";

    // Act
    let rows = parse(payload, '|', "\\N").expect("parse");

    // Assert
    assert_eq!(rows, vec![some(&["x\ty", "z"])]);
}

#[test]
fn should_stop_text_copy_at_alone_end_marker() {
    // Arrange
    let payload = b"a\tb\n\\.\nc\td\n";

    // Act
    let rows = parse(payload, '\t', "\\N").expect("parse");

    // Assert
    assert_eq!(rows, vec![some(&["a", "b"])]);
}

#[test]
fn should_stop_one_column_text_copy_at_alone_end_marker() {
    // Arrange
    let payload = b"a\n\\.\nc\n";

    // Act
    let rows = parse(payload, '\t', "\\N").expect("parse");

    // Assert
    assert_eq!(rows, vec![some(&["a"])]);
}

#[test]
fn should_stop_at_end_marker_with_crlf_line_endings() {
    // Arrange
    let payload = b"a\tb\r\n\\.\r\nc\td\r\n";

    // Act
    let rows = parse(payload, '\t', "\\N").expect("parse");

    // Assert
    assert_eq!(rows, vec![some(&["a", "b"])]);
}

#[test]
fn should_accept_end_marker_as_the_only_content() {
    // Arrange
    let payload = b"\\.\n";

    // Act
    let rows = parse(payload, '\t', "\\N").expect("parse");

    // Assert
    assert_eq!(rows.len(), 0);
}

#[test]
fn should_ignore_data_after_end_marker_even_when_malformed() {
    // Arrange
    let payload = b"a\n\\.\nzzz\twrong\n";

    // Act
    let rows = parse(payload, '\t', "\\N").expect("parse");

    // Assert
    assert_eq!(rows, vec![some(&["a"])]);
}

#[test]
fn should_reject_end_marker_followed_by_text() {
    // Arrange
    let payload = b"a\n\\.x\n";

    // Act
    let error = parse(payload, '\t', "\\N").expect_err("not alone");

    // Assert
    assert!(error.contains("end-of-copy marker is not alone on its line"));
}

#[test]
fn should_reject_end_marker_after_other_field_data() {
    // Arrange
    let payload = b"a\\.\nb\n";

    // Act
    let error = parse(payload, '\t', "\\N").expect_err("not alone");

    // Assert
    assert!(error.contains("end-of-copy marker is not alone on its line"));
}

#[test]
fn should_reject_end_marker_without_final_newline() {
    // Arrange
    let payload = b"a\n\\.";

    // Act
    let error = parse(payload, '\t', "\\N").expect_err("not alone");

    // Assert
    assert!(error.contains("end-of-copy marker is not alone on its line"));
}

#[test]
fn should_reject_end_marker_whose_newline_style_differs() {
    // Arrange
    let payload = b"a\tb\n\\.\r\n";

    // Act
    let error = parse(payload, '\t', "\\N").expect_err("style mismatch");

    // Assert
    assert!(error.contains("end-of-copy marker does not match previous newline style"));
}

#[test]
fn should_keep_escaped_backslash_before_dot_as_data() {
    // Arrange
    let payload = b"a\n\\\\.\n";

    // Act
    let rows = parse(payload, '\t', "\\N").expect("parse");

    // Assert
    assert_eq!(rows, vec![some(&["a"]), some(&["\\."])]);
}

#[test]
fn should_decode_octal_and_hex_escapes() {
    // Arrange
    let payload = b"\\101\\x42\tz\n";

    // Act
    let rows = parse(payload, '\t', "\\N").expect("parse");

    // Assert
    assert_eq!(rows, vec![some(&["AB", "z"])]);
}

#[test]
fn should_decode_one_to_three_octal_digits() {
    // Arrange
    let payload = b"\\1\\12\\1234\n";

    // Act
    let rows = parse(payload, '\t', "\\N").expect("parse");

    // Assert
    assert_eq!(rows, vec![some(&["\u{1}\nS4"])]);
}

#[test]
fn should_decode_one_or_two_hex_digits() {
    // Arrange
    let payload = b"\\x4\\x41g\n";

    // Act
    let rows = parse(payload, '\t', "\\N").expect("parse");

    // Assert
    assert_eq!(rows, vec![some(&["\u{4}Ag"])]);
}

#[test]
fn should_take_hex_escape_without_digits_literally() {
    // Arrange
    let payload = b"\\xzz\n";

    // Act
    let rows = parse(payload, '\t', "\\N").expect("parse");

    // Assert
    assert_eq!(rows, vec![some(&["xzz"])]);
}

#[test]
fn should_decode_multibyte_character_written_as_octal_bytes() {
    // Arrange
    let payload = b"\\303\\251\n";

    // Act
    let rows = parse(payload, '\t', "\\N").expect("parse");

    // Assert
    assert_eq!(rows, vec![some(&["\u{e9}"])]);
}

#[test]
fn should_reject_octal_escape_that_forms_invalid_utf8() {
    // Arrange
    let payload = b"\\377\n";

    // Act
    let result = parse(payload, '\t', "\\N");

    // Assert
    assert!(result.is_err());
}

#[test]
fn should_reject_escape_that_decodes_to_nul() {
    // Arrange
    let payload = b"\\0\n";

    // Act
    let result = parse(payload, '\t', "\\N");

    // Assert
    assert!(result.is_err());
}

#[test]
fn should_decode_control_character_escapes() {
    // Arrange
    let payload = b"\\b\\f\\n\\r\\t\\v\n";

    // Act
    let rows = parse(payload, '\t', "\\N").expect("parse");

    // Assert
    assert_eq!(rows, vec![some(&["\u{8}\u{c}\n\r\t\u{b}"])]);
}

#[test]
fn should_take_backslash_before_other_characters_literally() {
    // Arrange
    let payload = b"\\8\\q\t\\\\N\n";

    // Act
    let rows = parse(payload, '\t', "\\N").expect("parse");

    // Assert
    assert_eq!(rows, vec![some(&["8q", "\\N"])]);
}

#[test]
fn should_treat_backslash_n_as_null_only_for_the_whole_field() {
    // Arrange
    let payload = b"\\N\tx\\N\n";

    // Act
    let rows = parse(payload, '\t', "\\N").expect("parse");

    // Assert
    assert_eq!(rows, vec![vec![None, Some("xN".into())]]);
}

#[test]
fn should_match_custom_null_string_against_the_raw_field() {
    // Arrange
    let payload = b"zz\t\\N\n";

    // Act
    let rows = parse(payload, '\t', "zz").expect("parse");

    // Assert
    assert_eq!(rows, vec![vec![None, Some("N".into())]]);
}

#[test]
fn should_drop_trailing_lone_backslash_at_end_of_payload() {
    // Arrange
    let payload = b"end\\";

    // Act
    let rows = parse(payload, '\t', "\\N").expect("parse");

    // Assert
    assert_eq!(rows, vec![some(&["end"])]);
}
