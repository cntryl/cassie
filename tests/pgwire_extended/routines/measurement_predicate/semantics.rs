#[test]
fn should_convert_path_closure_with_open_close_functions() {
    // Arrange
    let sql = "SELECT pclose('[(0,0),(4,0),(4,3)]'::path) AS closed_path, \
                      popen('((0,0),(4,0),(4,3))'::path) AS open_path";

    // Act
    let frames = execute_geometry_query("issue-907-path-open-close-functions", sql);

    // Assert
    assert_eq!(wire::error_code(&frames), None);
    assert_eq!(
        geometry_result_types(&frames),
        vec![(602, -1, -1, 0), (602, -1, -1, 0)]
    );
    assert_eq!(
        wire::data_rows(&frames),
        vec![vec![
            Some("((0,0),(4,0),(4,3))".to_owned()),
            Some("[(0,0),(4,0),(4,3)]".to_owned()),
        ]]
    );
}
