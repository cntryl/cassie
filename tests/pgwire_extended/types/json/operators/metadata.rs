#[test]
fn should_type_jsonpath_literals_with_postgresql_oid() {
    // Arrange
    let sql = "SELECT '$.items[*].double()'::JSONPATH";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("issue-809-jsonpath-literal-type", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (4072, -1, -1), 0, true);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("$.\"items\"[*].double()".to_owned())]]
    );
}
