#[test]
fn should_apply_comparison_matrix_to_subquery_quantifiers() {
    // Arrange
    let sql = "WITH candidates(value) AS (VALUES (1::INT4), (2::INT4), (3::INT4)) \
        SELECT \
            2 = ANY (SELECT value FROM candidates) AS any_eq_true, \
            2 = ANY (SELECT value FROM candidates WHERE value = 3) AS any_eq_false, \
            2 <> ANY (SELECT value FROM candidates) AS any_ne_true, \
            2 <> ANY (SELECT value FROM candidates WHERE value = 2) AS any_ne_false, \
            2 < ANY (SELECT value FROM candidates) AS any_lt_true, \
            2 < ANY (SELECT value FROM candidates WHERE value <= 2) AS any_lt_false, \
            2 <= ANY (SELECT value FROM candidates) AS any_lte_true, \
            2 <= ANY (SELECT value FROM candidates WHERE value = 1) AS any_lte_false, \
            2 > ANY (SELECT value FROM candidates) AS any_gt_true, \
            2 > ANY (SELECT value FROM candidates WHERE value = 3) AS any_gt_false, \
            2 >= ANY (SELECT value FROM candidates) AS any_gte_true, \
            2 >= ANY (SELECT value FROM candidates WHERE value = 3) AS any_gte_false, \
            2 = ALL (SELECT value FROM candidates WHERE value = 2) AS all_eq_true, \
            2 = ALL (SELECT value FROM candidates) AS all_eq_false, \
            2 <> ALL (SELECT value FROM candidates WHERE value = 1) AS all_ne_true, \
            2 <> ALL (SELECT value FROM candidates WHERE value = 2) AS all_ne_false, \
            2 < ALL (SELECT value FROM candidates WHERE value = 3) AS all_lt_true, \
            2 < ALL (SELECT value FROM candidates WHERE value = 1) AS all_lt_false, \
            2 <= ALL (SELECT value FROM candidates WHERE value >= 2) AS all_lte_true, \
            2 <= ALL (SELECT value FROM candidates WHERE value = 1) AS all_lte_false, \
            2 > ALL (SELECT value FROM candidates WHERE value = 1) AS all_gt_true, \
            2 > ALL (SELECT value FROM candidates WHERE value = 3) AS all_gt_false, \
            2 >= ALL (SELECT value FROM candidates WHERE value <= 2) AS all_gte_true, \
            2 >= ALL (SELECT value FROM candidates WHERE value = 3) AS all_gte_false";
    let batches = run_script(
        "issue-762-subquery-any-all-operator-matrix",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])],
    );

    // Act
    let result = &batches[0];

    // Assert
    assert_eq!(wire::error_code(result), None);
    let description = result
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("ANY/ALL Boolean RowDescription");
    let columns = wire::parse_row_description(&description.1);
    assert_eq!(columns.len(), 24);
    assert!(columns.iter().all(|column| column.type_oid == 16));
    assert_eq!(
        wire::data_rows(result),
        vec![vec![
            Some("t".to_owned()),
            Some("f".to_owned()),
            Some("t".to_owned()),
            Some("f".to_owned()),
            Some("t".to_owned()),
            Some("f".to_owned()),
            Some("t".to_owned()),
            Some("f".to_owned()),
            Some("t".to_owned()),
            Some("f".to_owned()),
            Some("t".to_owned()),
            Some("f".to_owned()),
            Some("t".to_owned()),
            Some("f".to_owned()),
            Some("t".to_owned()),
            Some("f".to_owned()),
            Some("t".to_owned()),
            Some("f".to_owned()),
            Some("t".to_owned()),
            Some("f".to_owned()),
            Some("t".to_owned()),
            Some("f".to_owned()),
            Some("t".to_owned()),
            Some("f".to_owned()),
        ]]
    );
}
