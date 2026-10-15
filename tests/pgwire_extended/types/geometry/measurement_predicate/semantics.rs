#[test]
fn should_measure_box_dimensions_with_circle_center() {
    // Arrange
    let sql = "SELECT width('((0,0),(4,3))'::box) AS box_width, \
                      height('((0,0),(4,3))'::box) AS box_height, \
                      center('<(2,3),4>'::circle) AS circle_center";

    // Act
    let frames = execute_geometry_query("issue-907-box-circle-measurements", sql);

    // Assert
    assert_eq!(wire::error_code(&frames), None);
    assert_eq!(
        geometry_result_types(&frames),
        vec![(701, 8, -1, 0), (701, 8, -1, 0), (600, 16, -1, 0)]
    );
    assert_eq!(
        wire::data_rows(&frames),
        vec![vec![
            Some("4".to_owned()),
            Some("3".to_owned()),
            Some("(2,3)".to_owned()),
        ]]
    );
}
