#[test]
fn should_compare_two_column_subquery_rows_with_null_truth_values() {
    // Arrange
    let sql = "WITH candidates(a, b) AS (VALUES \
        (1::INT4, NULL::INT4), (1::INT4, 2::INT4), (2::INT4, 2::INT4)) \
        SELECT \
            ROW(1::INT4, 2::INT4) = \
                (SELECT a, b FROM candidates WHERE a = 1 AND b = 2) AS exact_equal, \
            ROW(1::INT4, 2::INT4) IN \
                (SELECT a, b FROM candidates WHERE a = 1) AS in_contains_equal, \
            ROW(1::INT4, NULL::INT4) = \
                (SELECT a, b FROM candidates WHERE a = 1 AND b IS NULL) AS null_field_equal, \
            ROW(1::INT4, NULL::INT4) IN \
                (SELECT a, b FROM candidates WHERE a = 1 AND b = 2) AS null_field_indeterminate, \
            ROW(1::INT4, NULL::INT4) IN \
                (SELECT a, b FROM candidates WHERE a = 2) AS nonnull_field_mismatch";
    let batches = run_script(
        "issue-762-row-subquery-null-wire",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])],
    );

    // Act
    let result = &batches[0];

    // Assert
    assert_eq!(wire::error_code(result), None);
    let description = result
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("row-subquery Boolean RowDescription");
    let columns = wire::parse_row_description(&description.1);
    assert_eq!(
        columns
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![16; 5]
    );
    assert_eq!(
        columns
            .iter()
            .map(|column| column.name.as_str())
            .collect::<Vec<_>>(),
        [
            "exact_equal",
            "in_contains_equal",
            "null_field_equal",
            "null_field_indeterminate",
            "nonnull_field_mismatch",
        ]
    );
    assert_eq!(
        wire::data_rows(result),
        vec![vec![
            Some("t".to_owned()),
            Some("t".to_owned()),
            None,
            None,
            Some("f".to_owned()),
        ]]
    );
}
