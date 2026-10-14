//! PostgreSQL 18 row-subquery and row-comparison pgwire semantics for #762/#800.

use crate::support_parameter_occurrence_wire::{Step, run_script};
use crate::support_pgwire as wire;

#[test]
fn should_encode_row_constructor_null_ordering_truth_table_over_pgwire() {
    // Arrange
    let sql = "SELECT \
        ROW(1::INT4, NULL::INT4) = ROW(1::INT4, NULL::INT4) AS null_equal, \
        ROW(1::INT4, NULL::INT4) <> ROW(1::INT4, NULL::INT4) AS null_not_equal, \
        ROW(1::INT4, NULL::INT4) IS DISTINCT FROM ROW(1::INT4, NULL::INT4) AS same_null_distinct, \
        ROW(1::INT4, NULL::INT4) IS NOT DISTINCT FROM ROW(1::INT4, NULL::INT4) AS same_null_not_distinct, \
        ROW(1::INT4, NULL::INT4) IS DISTINCT FROM ROW(2::INT4, 3::INT4) AS different_prefix_distinct, \
        ROW(1::INT4, NULL::INT4) IS NOT DISTINCT FROM ROW(2::INT4, 3::INT4) AS different_prefix_not_distinct, \
        ROW(1::INT4, NULL::INT4) < ROW(2::INT4, 0::INT4) AS ordering_stops_before_null, \
        ROW(1::INT4, NULL::INT4) < ROW(1::INT4, 2::INT4) AS ordering_reaches_null";
    let batches = run_script(
        "issue-800-row-constructor-null-wire-truth-table",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])],
    );

    // Act
    let result = &batches[0];

    // Assert
    assert_eq!(wire::error_code(result), None);
    let description = result
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("row-comparison Boolean RowDescription");
    let columns = wire::parse_row_description(&description.1);
    assert_eq!(
        columns
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![16; 8]
    );
    assert_eq!(
        columns
            .iter()
            .map(|column| column.name.as_str())
            .collect::<Vec<_>>(),
        [
            "null_equal",
            "null_not_equal",
            "same_null_distinct",
            "same_null_not_distinct",
            "different_prefix_distinct",
            "different_prefix_not_distinct",
            "ordering_stops_before_null",
            "ordering_reaches_null",
        ]
    );
    assert_eq!(
        wire::data_rows(result),
        vec![vec![
            None,
            None,
            Some("f".to_owned()),
            Some("t".to_owned()),
            Some("t".to_owned()),
            Some("f".to_owned()),
            Some("t".to_owned()),
            None,
        ]]
    );
}

#[test]
fn should_return_empty_row_subquery_truth_values_over_pgwire() {
    // Arrange
    let sql = "WITH candidates(a, b) AS (VALUES (1::INT4, 100::INT4)) \
        SELECT \
            ROW(1::INT4, 100::INT4) = \
                (SELECT a, b FROM candidates WHERE false) AS empty_row_comparison, \
            ROW(1::INT4, NULL::INT4) IN \
                (SELECT a, b FROM candidates WHERE false) AS empty_row_in, \
            ROW(1::INT4, NULL::INT4) NOT IN \
                (SELECT a, b FROM candidates WHERE false) AS empty_row_not_in";
    let batches = run_script(
        "issue-762-empty-row-subquery-wire-truth-table",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])],
    );

    // Act
    let result = &batches[0];

    // Assert
    assert_eq!(wire::error_code(result), None);
    let description = result
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("empty row-subquery Boolean RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![16; 3]
    );
    assert_eq!(
        wire::data_rows(result),
        vec![vec![None, Some("f".to_owned()), Some("t".to_owned())]]
    );
}

#[test]
fn should_raise_cardinality_violation_for_multirow_row_subquery_over_pgwire() {
    // Arrange
    let sql = "WITH candidates(a, b) AS (VALUES \
        (1::INT4, 100::INT4), (2::INT4, 200::INT4)) \
        SELECT ROW(1::INT4, 100::INT4) = (SELECT a, b FROM candidates)";
    let batches = run_script(
        "issue-762-multirow-row-subquery-cardinality-wire",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])],
    );

    // Act
    let result = &batches[0];
    let error = result
        .iter()
        .find(|frame| frame.0 == b'E')
        .expect("multirow row-subquery ErrorResponse");
    let fields = wire::parse_error_fields(&error.1);
    let state = fields
        .iter()
        .find(|(kind, _)| *kind == 'C')
        .map(|(_, value)| value.as_str());
    let message = fields
        .iter()
        .find(|(kind, _)| *kind == 'M')
        .map(|(_, value)| value.as_str());

    // Assert
    assert_eq!(state, Some("21000"));
    assert_eq!(
        message,
        Some("more than one row returned by a subquery used as an expression")
    );
}

#[test]
fn should_report_too_many_columns_for_row_subqueries_over_pgwire() {
    // Arrange
    let cases = [
        "SELECT ROW(1::INT4) = \
            (SELECT id, value FROM row_subquery_width_values WHERE id = 1)",
        "SELECT ROW(1::INT4) IN \
            (SELECT id, value FROM row_subquery_width_values WHERE id = 1)",
    ];
    let mut steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(
            "CREATE TABLE row_subquery_width_values (id INT4, value INT4)",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "INSERT INTO row_subquery_width_values (id, value) VALUES (1, 100)",
        )]),
    ];
    steps.extend(
        cases
            .iter()
            .map(|sql| Step::ThroughReady(vec![wire::simple_query_frame(sql)])),
    );

    // Act
    let batches = run_script("issue-762-row-subquery-too-many-columns-wire", steps);
    let setup_errors = batches[..2]
        .iter()
        .filter_map(|batch| wire::error_code(batch))
        .collect::<Vec<_>>();
    let actual_errors = batches
        .iter()
        .skip(2)
        .map(|batch| {
            let error = batch
                .iter()
                .find(|frame| frame.0 == b'E')
                .expect("wide row-subquery ErrorResponse");
            let fields = wire::parse_error_fields(&error.1);
            let state = fields
                .iter()
                .find(|(kind, _)| *kind == 'C')
                .map(|(_, value)| value.clone());
            let message = fields
                .iter()
                .find(|(kind, _)| *kind == 'M')
                .map(|(_, value)| value.clone());
            (state, message)
        })
        .collect::<Vec<_>>();

    // Assert
    assert!(
        setup_errors.is_empty(),
        "table setup errors: {setup_errors:?}"
    );
    assert_eq!(
        actual_errors,
        vec![
            (
                Some("42601".to_owned()),
                Some("subquery has too many columns".to_owned()),
            );
            cases.len()
        ]
    );
}
