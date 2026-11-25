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
