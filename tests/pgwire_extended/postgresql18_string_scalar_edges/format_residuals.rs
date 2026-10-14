//! Remaining PostgreSQL 18 format() syntax and error contracts for #803.

use super::{error_fields, run_script, wire, Step};

#[test]
fn should_apply_positional_dynamic_width_format_specifiers() {
    // Arrange
    let sql = "SELECT format('[%2$-*1$s]|[%4$*3$s]|[%5$I]|[%6$L]', 8, 'cat', 4, 'x', 'select', 'O''Reilly') AS formatted";

    // Act
    let batches = run_script(
        "issue-803-format-positional-dynamic-width",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])],
    );
    let result = &batches[0];

    // Assert
    assert_eq!(wire::error_code(result), None, "format errors: {result:?}");
    let description = result
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("format RowDescription");
    assert_eq!(wire::parse_row_description(&description.1)[0].type_oid, 25);
    assert_eq!(
        wire::data_rows(result),
        vec![vec![Some(
            "[cat     ]|[   x]|[\"select\"]|['O''Reilly']".to_owned()
        )]]
    );
}

#[test]
fn should_report_invalid_parameter_value_for_unknown_format_conversion() {
    // Arrange
    let batches = run_script(
        "issue-803-format-unknown-conversion-sqlstate",
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT format('%s', 'valid') AS formatted",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT format('%q', 'x') AS malformed_conversion",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame("SELECT 45 AS recovery")]),
        ],
    );

    // Act
    let valid = &batches[0];
    let invalid = &batches[1];
    let recovery = &batches[2];

    // Assert
    assert_eq!(wire::error_code(valid), None);
    assert_eq!(wire::data_rows(valid), vec![vec![Some("valid".to_owned())]]);
    assert_eq!(wire::error_code(invalid).as_deref(), Some("22023"));
    assert!(wire::data_rows(invalid).is_empty());
    let fields = error_fields(invalid)
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    assert!(
        fields.iter().any(|(field, value)| {
            *field == 'M' && value == "unrecognized format() type specifier \"q\""
        }),
        "invalid format conversion must reach format()'s argument-validation path: {fields:?}"
    );
    assert!(
        fields
            .iter()
            .any(|(field, value)| { *field == 'H' && value == "For a single \"%\" use \"%%\"." }),
        "PostgreSQL 18.6 format() hint: {fields:?}"
    );
    assert_eq!(wire::error_code(recovery), None);
    assert_eq!(wire::data_rows(recovery), vec![vec![Some("45".to_owned())]]);
}
