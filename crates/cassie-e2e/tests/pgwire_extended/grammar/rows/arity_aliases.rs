//! PostgreSQL row arity, VALUES aliases, and mixed NULL rows for #800.

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

fn assert_query_result(
    frames: &[(u8, Vec<u8>)],
    expected_oids: &[i32],
    expected_rows: &[Vec<Option<String>>],
    context: &str,
) {
    let errors = frames
        .iter()
        .filter(|frame| frame.0 == b'E')
        .map(|frame| wire::parse_error_fields(&frame.1))
        .collect::<Vec<_>>();
    assert_eq!(
        wire::error_code(frames),
        None,
        "{context} ErrorResponse: {errors:?}"
    );
    let description = frames
        .iter()
        .find(|frame| frame.0 == b'T')
        .unwrap_or_else(|| panic!("{context} must return RowDescription"));
    let columns = wire::parse_row_description(&description.1);
    assert_eq!(
        columns
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        expected_oids,
        "{context} result OIDs"
    );
    assert_eq!(
        wire::data_rows(frames).as_slice(),
        expected_rows,
        "{context} rows"
    );
}

#[test]
fn should_select_mixed_type_null_values_through_explicit_column_aliases() {
    // Arrange
    let table_control = "SELECT first_value, second_value \
                         FROM issue_800_values_alias_control ORDER BY id";
    let row_control = "SELECT ROW(1::INT4, 2::INT4) = ROW(1::INT4, 2::INT4) AS row_equal";
    let values_query = "SELECT source.first_value, source.second_value \
                        FROM (VALUES (1::INT4, NULL::TEXT), (2::INT4, 'two'::TEXT)) \
                             AS source(first_value, second_value) \
                        ORDER BY source.first_value";
    let batches = run_script(
        "issue-800-values-row-alias-null-mixed-types",
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame(
                "CREATE TABLE issue_800_values_alias_control \
                 (id INT4 PRIMARY KEY, first_value INT4, second_value TEXT)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "INSERT INTO issue_800_values_alias_control \
                 VALUES (1, 1, NULL), (2, 2, 'two')",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(table_control)]),
            Step::ThroughReady(vec![wire::simple_query_frame(row_control)]),
            Step::ThroughReady(vec![wire::simple_query_frame(values_query)]),
        ],
    );

    // Act
    let table_baseline = &batches[2];
    let row_baseline = &batches[3];
    let values_result = &batches[4];

    // Assert
    assert!(batches[..2]
        .iter()
        .all(|frames| wire::error_code(frames).is_none()));
    assert_query_result(
        table_baseline,
        &[23, 25],
        &[
            vec![Some("1".to_owned()), None],
            vec![Some("2".to_owned()), Some("two".to_owned())],
        ],
        "plain-table mixed-type NULL control",
    );
    assert_query_result(
        row_baseline,
        &[16],
        &[vec![Some("t".to_owned())]],
        "positive row-constructor comparison control",
    );
    assert_query_result(
        values_result,
        &[23, 25],
        &[
            vec![Some("1".to_owned()), None],
            vec![Some("2".to_owned()), Some("two".to_owned())],
        ],
        "VALUES relation with explicit aliases and mixed NULL rows",
    );
    let description = values_result
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("VALUES relation RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.name.as_str())
            .collect::<Vec<_>>(),
        ["first_value", "second_value"]
    );
}

#[test]
fn should_reject_row_comparisons_with_unequal_arity_using_postgres_syntax_error() {
    // Arrange
    let control_sql = "SELECT ROW(1::INT4, 2::INT4) = ROW(1::INT4, 2::INT4) AS row_equal";
    let invalid_sql = "SELECT ROW(1::INT4, 2::INT4) = ROW(1::INT4) AS row_equal";
    let batches = run_script(
        "issue-800-row-comparison-unequal-arity",
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame(control_sql)]),
            Step::ThroughReady(vec![wire::simple_query_frame(invalid_sql)]),
        ],
    );

    // Act
    let control = &batches[0];
    let invalid = &batches[1];

    // Assert
    assert_query_result(
        control,
        &[16],
        &[vec![Some("t".to_owned())]],
        "equal-arity row-comparison control",
    );
    let errors = invalid
        .iter()
        .filter(|frame| frame.0 == b'E')
        .map(|frame| wire::parse_error_fields(&frame.1))
        .collect::<Vec<_>>();
    assert_eq!(
        wire::error_code(invalid).as_deref(),
        Some("42601"),
        "unequal-arity row-comparison ErrorResponse: {errors:?}"
    );
    assert!(errors.iter().flatten().any(|(kind, message)| {
        *kind == 'M' && message == "unequal number of entries in row expressions"
    }));
}
