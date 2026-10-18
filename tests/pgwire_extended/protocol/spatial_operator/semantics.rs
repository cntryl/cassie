#[test]
fn should_measure_box_area_center_shape_lengths() {
    // Arrange
    let sql = "SELECT area('((0,0),(4,3))'::box), \
                      center('((0,0),(4,3))'::box), \
                      @-@ '[(0,0),(4,3)]'::lseg, \
                      @-@ '[(0,0),(4,0),(4,3)]'::path";

    // Act
    let frames = execute_geometry_query("issue-907-box-measurements-geometric-lengths", sql);

    // Assert
    assert_eq!(wire::error_code(&frames), None);
    assert_eq!(
        geometry_result_types(&frames),
        vec![
            (701, 8, -1, 0),
            (600, 16, -1, 0),
            (701, 8, -1, 0),
            (701, 8, -1, 0)
        ]
    );
    assert_eq!(
        wire::data_rows(&frames),
        vec![vec![
            Some("12".to_owned()),
            Some("(2,1.5)".to_owned()),
            Some("5".to_owned()),
            Some("7".to_owned()),
        ]]
    );
}

#[test]
fn should_test_remaining_shape_point_containment_signatures() {
    // Arrange
    let sql = "SELECT '[(0,0),(4,0)]'::path @> point '(2,0)', \
                      point '(2,0)' <@ '[(0,0),(4,0)]'::path, \
                      point '(2,0)' <@ '[(0,0),(4,0)]'::lseg, \
                      point '(2,0)' <@ '{0,1,0}'::line, \
                      point '(1,2)' <@ '<(1,2),2>'::circle, \
                      '[(0,0),(4,0)]'::lseg <@ '((0,-1),(4,1))'::box, \
                      '[(0,0),(4,0)]'::lseg <@ '{0,1,0}'::line";

    // Act
    let frames = execute_geometry_query("issue-907-remaining-shape-containment-signatures", sql);

    // Assert
    assert_eq!(wire::error_code(&frames), None);
    assert_eq!(geometry_result_types(&frames), vec![(16, 1, -1, 0); 7]);
    assert_eq!(
        wire::data_rows(&frames),
        vec![vec![Some("t".to_owned()); 7]]
    );
}
