#[test]
fn should_convert_builtin_geometry_shapes_between_types() {
    // Arrange
    let sql = "SELECT box(point '(1,2)'), \
                      box('((0,0),(4,0),(4,3),(0,3))'::polygon), \
                      box('<(2,3),1.4142135623730951>'::circle), \
                      circle('((0,0),(4,3))'::box), \
                      circle('((0,0),(4,0),(4,3),(0,3))'::polygon), \
                      point('((0,0),(4,3))'::box), \
                      point('<(1,2),3>'::circle), \
                      point('[(0,0),(4,3)]'::lseg), \
                      point('((0,0),(4,0),(4,3),(0,3))'::polygon), \
                      polygon('((0,0),(4,3))'::box), \
                      path('((0,0),(4,0),(4,3),(0,3))'::polygon), \
                      polygon('((0,0),(4,0),(4,3),(0,3))'::path), \
                      polygon(4, '<(2,3),1>'::circle), \
                      lseg('((0,0),(4,3))'::box)";

    // Act
    let frames = execute_geometry_query("issue-907-shape-conversion-constructors", sql);

    // Assert
    assert_eq!(wire::error_code(&frames), None);
    assert_eq!(
        geometry_result_types(&frames),
        vec![
            (603, 32, -1, 0),
            (603, 32, -1, 0),
            (603, 32, -1, 0),
            (718, 24, -1, 0),
            (718, 24, -1, 0),
            (600, 16, -1, 0),
            (600, 16, -1, 0),
            (600, 16, -1, 0),
            (600, 16, -1, 0),
            (604, -1, -1, 0),
            (602, -1, -1, 0),
            (604, -1, -1, 0),
            (604, -1, -1, 0),
            (601, 32, -1, 0),
        ]
    );
    assert_eq!(
        wire::data_rows(&frames),
        vec![vec![
            Some("(1,2),(1,2)".to_owned()),
            Some("(4,3),(0,0)".to_owned()),
            Some("(3,4),(1,2)".to_owned()),
            Some("<(2,1.5),2.5>".to_owned()),
            Some("<(2,1.5),2.5>".to_owned()),
            Some("(2,1.5)".to_owned()),
            Some("(1,2)".to_owned()),
            Some("(2,1.5)".to_owned()),
            Some("(2,1.5)".to_owned()),
            Some("((0,0),(0,3),(4,3),(4,0))".to_owned()),
            Some("((0,0),(4,0),(4,3),(0,3))".to_owned()),
            Some("((0,0),(4,0),(4,3),(0,3))".to_owned()),
            Some("((1,3),(2,4),(3,3),(2,2))".to_owned()),
            Some("[(4,3),(0,0)]".to_owned()),
        ]]
    );
}

#[test]
fn should_convert_circle_to_twelve_point_polygon_with_preserved_center() {
    // Arrange
    let sql = "SELECT pg_typeof(polygon('<(2,3),1>'::circle))::oid AS polygon_oid, \
                      npoints(polygon('<(2,3),1>'::circle)) AS point_count, \
                      point(polygon('<(2,3),1>'::circle)) AS polygon_center, \
                      box(polygon('<(2,3),1>'::circle)) AS polygon_bounds";

    // Act
    let frames = execute_geometry_query("issue-907-circle-to-polygon-conversion", sql);

    // Assert
    assert_eq!(wire::error_code(&frames), None);
    assert_eq!(
        geometry_result_types(&frames),
        vec![
            (26, 4, -1, 0),
            (23, 4, -1, 0),
            (600, 16, -1, 0),
            (603, 32, -1, 0),
        ]
    );
    assert_eq!(
        wire::data_rows(&frames),
        vec![vec![
            Some("604".to_owned()),
            Some("12".to_owned()),
            Some("(2,3)".to_owned()),
            Some("(3,4),(1,2)".to_owned()),
        ]]
    );
}

#[test]
fn should_apply_point_box_geometric_arithmetic() {
    // Arrange
    let sql = "SELECT point '(4,6)' + point '(1,2)', \
                      point '(4,6)' - point '(1,2)', \
                      point '(4,6)' * point '(1,2)', \
                      point '(4,6)' / point '(1,2)', \
                      '((0,0),(2,2))'::box + point '(3,4)', \
                      '((0,0),(2,2))'::box - point '(1,1)', \
                      '((0,0),(2,2))'::box * point '(2,0)', \
                      '((0,0),(2,2))'::box / point '(2,0)'";

    // Act
    let frames = execute_geometry_query("issue-907-point-box-arithmetic", sql);

    // Assert
    assert_eq!(wire::error_code(&frames), None);
    assert_eq!(
        geometry_result_types(&frames),
        vec![
            (600, 16, -1, 0),
            (600, 16, -1, 0),
            (600, 16, -1, 0),
            (600, 16, -1, 0),
            (603, 32, -1, 0),
            (603, 32, -1, 0),
            (603, 32, -1, 0),
            (603, 32, -1, 0),
        ]
    );
    assert_eq!(
        wire::data_rows(&frames),
        vec![vec![
            Some("(5,8)".to_owned()),
            Some("(3,4)".to_owned()),
            Some("(-8,14)".to_owned()),
            Some("(3.2,-0.4)".to_owned()),
            Some("(5,6),(3,4)".to_owned()),
            Some("(1,1),(-1,-1)".to_owned()),
            Some("(4,4),(0,0)".to_owned()),
            Some("(1,1),(0,0)".to_owned()),
        ]]
    );
}

#[test]
fn should_translate_scale_rotate_circle_path_geometry() {
    // Arrange
    let sql = "SELECT '<(1,2),3>'::circle + point '(3,4)', \
                      '<(1,2),3>'::circle - point '(1,2)', \
                      '<(1,2),3>'::circle * point '(2,0)', \
                      '<(1,2),3>'::circle / point '(2,0)', \
                      '[(0,0),(2,0),(2,2)]'::path + point '(3,4)', \
                      '[(0,0),(2,0),(2,2)]'::path - point '(1,1)', \
                      '[(0,0),(2,0),(2,2)]'::path * point '(2,0)', \
                      '[(0,0),(2,0),(2,2)]'::path / point '(2,0)', \
                      '[(0,0),(2,0)]'::path + '[(5,0),(7,0)]'::path";

    // Act
    let frames = execute_geometry_query("issue-907-circle-path-arithmetic", sql);

    // Assert
    assert_eq!(wire::error_code(&frames), None);
    assert_eq!(
        geometry_result_types(&frames),
        vec![
            (718, 24, -1, 0),
            (718, 24, -1, 0),
            (718, 24, -1, 0),
            (718, 24, -1, 0),
            (602, -1, -1, 0),
            (602, -1, -1, 0),
            (602, -1, -1, 0),
            (602, -1, -1, 0),
            (602, -1, -1, 0),
        ]
    );
    assert_eq!(
        wire::data_rows(&frames),
        vec![vec![
            Some("<(4,6),3>".to_owned()),
            Some("<(0,0),3>".to_owned()),
            Some("<(2,4),6>".to_owned()),
            Some("<(0.5,1),1.5>".to_owned()),
            Some("[(3,4),(5,4),(5,6)]".to_owned()),
            Some("[(-1,-1),(1,-1),(1,1)]".to_owned()),
            Some("[(0,0),(4,0),(4,4)]".to_owned()),
            Some("[(0,0),(1,0),(1,1)]".to_owned()),
            Some("[(0,0),(2,0),(5,0),(7,0)]".to_owned()),
        ]]
    );
}
