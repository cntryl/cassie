use super::*;

#[test]
fn should_preserve_macaddr_truncation_result_identity_over_pgwire() {
    // Arrange
    let sql = "SELECT trunc(CAST('12:34:56:78:90:ab' AS macaddr)) AS mac, \
                     trunc(CAST('12:34:56:78:90:ab:cd:ef' AS macaddr8)) AS eui64";
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])];

    // Act
    let batches = run_script("postgresql18-macaddr-trunc-result-identity", steps);

    // Assert
    let result = &batches[0];
    assert_eq!(wire::error_code(result), None);
    assert_eq!(
        row_columns(result)
            .iter()
            .map(|column| (column.type_oid, column.type_size, column.format_code))
            .collect::<Vec<_>>(),
        vec![(829, 6, 0), (774, 8, 0)]
    );
    assert_eq!(
        wire::data_rows(result),
        vec![vec![
            Some("12:34:56:00:00:00".to_owned()),
            Some("12:34:56:00:00:00:00:00".to_owned()),
        ]]
    );
}
