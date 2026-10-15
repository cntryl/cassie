//! PostgreSQL 18 row-subquery and row-comparison pgwire semantics for #762/#800.

use crate::support_parameter_occurrence_wire::{run_script, Step};
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
