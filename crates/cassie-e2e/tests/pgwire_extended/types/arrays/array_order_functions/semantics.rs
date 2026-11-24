#[test]
fn should_reverse_array_elements_in_order() {
    // Arrange
    let sql = "SELECT array_reverse(ARRAY[1,NULL,3]) AS reversed";

    // Act
    let frames = run_array_query(sql, "issue-807-array-reverse-null-element");

    // Assert
    fixture::assert_success(&frames);
    fixture::assert_single_column_descriptors(&frames, (1_007, -1, -1), 0, true);
    assert_eq!(
        wire::data_rows(&frames),
        vec![vec![Some("{3,NULL,1}".to_owned())]]
    );
}
