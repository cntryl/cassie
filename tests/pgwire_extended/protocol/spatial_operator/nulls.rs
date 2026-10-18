#[test]
fn should_return_null_when_closed_paths_are_concatenated() {
    // Arrange
    let sql = "SELECT '[(0,0),(1,0)]'::path + '((2,0),(3,0))'::path, \
                      '((0,0),(1,0))'::path + '[(2,0),(3,0)]'::path, \
                      '[(0,0),(1,0)]'::path + '[(2,0),(3,0)]'::path";

    // Act
    let frames = execute_geometry_query("issue-907-path-concatenation-closure", sql);

    // Assert
    assert_eq!(wire::error_code(&frames), None);
    assert_eq!(geometry_result_types(&frames), vec![(602, -1, -1, 0); 3]);
    assert_eq!(
        wire::data_rows(&frames),
        vec![vec![
            None,
            None,
            Some("[(0,0),(1,0),(2,0),(3,0)]".to_owned()),
        ]]
    );
}
