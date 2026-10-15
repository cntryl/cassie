#[test]
fn should_return_diagonal_segment_for_box_bounds() {
    // Arrange
    let sql = "SELECT diagonal('((0,0),(4,3))'::box) AS box_diagonal";

    // Act
    let frames = execute_geometry_query("issue-907-box-diagonal", sql);

    // Assert
    assert_eq!(wire::error_code(&frames), None);
    assert_eq!(geometry_result_types(&frames), vec![(601, 32, -1, 0)]);
    assert_eq!(
        wire::data_rows(&frames),
        vec![vec![Some("[(4,3),(0,0)]".to_owned())]]
    );
}

#[test]
fn should_construct_line_segment_from_point_endpoints() {
    // Arrange
    let sql = "SELECT line(point '(0,0)', point '(4,3)') AS infinite_line, \
                      lseg(point '(0,0)', point '(4,3)') AS finite_segment";

    // Act
    let frames = execute_geometry_query("issue-907-point-endpoint-constructors", sql);

    // Assert
    assert_eq!(wire::error_code(&frames), None);
    assert_eq!(
        geometry_result_types(&frames),
        vec![(628, 24, -1, 0), (601, 32, -1, 0)]
    );
    assert_eq!(
        wire::data_rows(&frames),
        vec![vec![
            Some("{0.75,-1,0}".to_owned()),
            Some("[(0,0),(4,3)]".to_owned()),
        ]]
    );
}

#[test]
fn should_preserve_path_area_length_closure_semantics() {
    // Arrange
    let sql = "SELECT area('[(0,0),(4,0),(4,3)]'::path) AS open_area, \
                      area('((0,0),(4,0),(4,3))'::path) AS closed_area, \
                      length('[(0,0),(4,0),(4,3)]'::path) AS open_length, \
                      length('((0,0),(4,0),(4,3))'::path) AS closed_length, \
                      length('[(0,0),(4,3)]'::lseg) AS segment_length, \
                      isopen('[(0,0),(4,0),(4,3)]'::path) AS open_is_open, \
                      isclosed('[(0,0),(4,0),(4,3)]'::path) AS open_is_closed, \
                      isopen('((0,0),(4,0),(4,3))'::path) AS closed_is_open, \
                      isclosed('((0,0),(4,0),(4,3))'::path) AS closed_is_closed";

    // Act
    let frames = execute_geometry_query("issue-907-path-length-area-closure", sql);

    // Assert
    assert_eq!(wire::error_code(&frames), None);
    assert_eq!(
        geometry_result_types(&frames),
        vec![
            (701, 8, -1, 0),
            (701, 8, -1, 0),
            (701, 8, -1, 0),
            (701, 8, -1, 0),
            (701, 8, -1, 0),
            (16, 1, -1, 0),
            (16, 1, -1, 0),
            (16, 1, -1, 0),
            (16, 1, -1, 0),
        ]
    );
    assert_eq!(
        wire::data_rows(&frames),
        vec![vec![
            None,
            Some("6".to_owned()),
            Some("7".to_owned()),
            Some("12".to_owned()),
            Some("5".to_owned()),
            Some("t".to_owned()),
            Some("f".to_owned()),
            Some("f".to_owned()),
            Some("t".to_owned()),
        ]]
    );
}

#[test]
fn should_classify_parallel_perpendicular_line_segment_pairs() {
    // Arrange
    let sql = "SELECT isparallel('{1,0,-2}'::line, '{2,0,4}'::line), \
                      isparallel('{1,0,-2}'::line, '{0,1,-3}'::line), \
                      isperp('{1,0,-2}'::line, '{0,1,-3}'::line), \
                      isperp('{1,0,-2}'::line, '{2,0,4}'::line), \
                      isparallel('[(0,0),(4,4)]'::lseg, '[(1,0),(3,2)]'::lseg), \
                      isparallel('[(0,0),(4,4)]'::lseg, '[(0,0),(4,0)]'::lseg), \
                      isperp('[(0,0),(4,4)]'::lseg, '[(0,4),(4,0)]'::lseg), \
                      isperp('[(0,0),(4,4)]'::lseg, '[(1,0),(3,2)]'::lseg)";

    // Act
    let frames = execute_geometry_query("issue-907-parallel-perpendicular-predicates", sql);

    // Assert
    assert_eq!(wire::error_code(&frames), None);
    assert_eq!(geometry_result_types(&frames), vec![(16, 1, -1, 0); 8]);
    assert_eq!(
        wire::data_rows(&frames),
        vec![vec![
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
