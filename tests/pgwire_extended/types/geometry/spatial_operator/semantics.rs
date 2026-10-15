#[test]
fn should_compute_geometry_bounds_centers_intersections() {
    // Arrange
    let sql = "SELECT bound_box('((0,0),(2,2))'::box, '((4,1),(7,5))'::box), \
                      @@ '((0,0),(4,3))'::box, \
                      @@ '<(2,3),4>'::circle, \
                      @@ '[(0,0),(4,3)]'::lseg, \
                      @@ '((0,0),(4,0),(4,3),(0,3))'::polygon, \
                      '((0,0),(4,4))'::box # '((2,2),(6,6))'::box, \
                      '((0,0),(4,4))'::box # '((5,5),(7,7))'::box";

    // Act
    let frames = execute_geometry_query("issue-907-bounds-centers-box-intersection", sql);

    // Assert
    assert_eq!(wire::error_code(&frames), None);
    assert_eq!(
        geometry_result_types(&frames),
        vec![
            (603, 32, -1, 0),
            (600, 16, -1, 0),
            (600, 16, -1, 0),
            (600, 16, -1, 0),
            (600, 16, -1, 0),
            (603, 32, -1, 0),
            (603, 32, -1, 0),
        ]
    );
    assert_eq!(
        wire::data_rows(&frames),
        vec![vec![
            Some("(7,5),(0,0)".to_owned()),
            Some("(2,1.5)".to_owned()),
            Some("(2,3)".to_owned()),
            Some("(2,1.5)".to_owned()),
            Some("(2,1.5)".to_owned()),
            Some("(4,4),(2,2)".to_owned()),
            None,
        ]]
    );
}

#[test]
fn should_return_geometry_intersections_vertex_counts() {
    // Arrange
    let sql = "SELECT # '((0,0),(4,0),(4,3),(0,3))'::polygon, \
                      # '[(0,0),(4,0),(4,3)]'::path, \
                      '{1,0,-2}'::line # '{0,1,-2}'::line, \
                      '[(0,0),(4,0)]'::lseg # '[(2,-2),(2,2)]'::lseg";

    // Act
    let frames = execute_geometry_query("issue-907-intersection-and-vertex-count-operators", sql);

    // Assert
    assert_eq!(wire::error_code(&frames), None);
    assert_eq!(
        geometry_result_types(&frames),
        vec![
            (23, 4, -1, 0),
            (23, 4, -1, 0),
            (600, 16, -1, 0),
            (600, 16, -1, 0)
        ]
    );
    assert_eq!(
        wire::data_rows(&frames),
        vec![vec![
            Some("4".to_owned()),
            Some("3".to_owned()),
            Some("(2,2)".to_owned()),
            Some("(2,0)".to_owned()),
        ]]
    );
}

#[test]
fn should_subscript_point_box_lseg_coordinates() {
    // Arrange
    let sql = "SELECT (point '(1,2)')[0], (point '(1,2)')[1], \
                      ('((4,3),(0,0))'::box)[0], ('((4,3),(0,0))'::box)[1], \
                      ('[(0,0),(4,3)]'::lseg)[0], ('[(0,0),(4,3)]'::lseg)[1]";

    // Act
    let frames = execute_geometry_query("issue-907-geometric-value-subscripts", sql);

    // Assert
    assert_eq!(wire::error_code(&frames), None);
    assert_eq!(
        geometry_result_types(&frames),
        vec![
            (701, 8, -1, 0),
            (701, 8, -1, 0),
            (600, 16, -1, 0),
            (600, 16, -1, 0),
            (600, 16, -1, 0),
            (600, 16, -1, 0),
        ]
    );
    assert_eq!(
        wire::data_rows(&frames),
        vec![vec![
            Some("1".to_owned()),
            Some("2".to_owned()),
            Some("(4,3)".to_owned()),
            Some("(0,0)".to_owned()),
            Some("(0,0)".to_owned()),
            Some("(4,3)".to_owned()),
        ]]
    );
}

#[test]
fn should_return_closest_points_for_all_documented_geometry_pairs() {
    // Arrange
    let sql = "SELECT point '(9,3)' ## '((0,0),(4,4))'::box, \
                      point '(4,3)' ## '[(0,0),(2,0)]'::lseg, \
                      point '(4,3)' ## '{0,1,-1}'::line, \
                      '[(0,0),(2,0)]'::lseg ## '((4,3),(6,5))'::box, \
                      '[(0,0),(4,0)]'::lseg ## '[(5,2),(5,4)]'::lseg, \
                      '{0,1,-1}'::line ## '[(0,0),(4,4)]'::lseg";

    // Act
    let frames = execute_geometry_query("issue-907-closest-point-overloads", sql);

    // Assert
    assert_eq!(wire::error_code(&frames), None);
    assert_eq!(geometry_result_types(&frames), vec![(600, 16, -1, 0); 6]);
    assert_eq!(
        wire::data_rows(&frames),
        vec![vec![
            Some("(4,3)".to_owned()),
            Some("(2,0)".to_owned()),
            Some("(4,1)".to_owned()),
            Some("(4,3)".to_owned()),
            Some("(5,2)".to_owned()),
            Some("(1,1)".to_owned()),
        ]]
    );
}

#[test]
fn should_report_intersection_for_each_supported_geometry_pair() {
    // Arrange
    let sql = "SELECT '((0,0),(4,4))'::box ?# '((2,2),(6,6))'::box, \
                      '((0,0),(4,4))'::box ?# '((5,5),(7,7))'::box, \
                      '{1,0,-2}'::line ?# '((0,0),(4,4))'::box, \
                      '{1,0,-9}'::line ?# '((0,0),(4,4))'::box, \
                      '[(0,0),(4,0)]'::lseg ?# '((2,-1),(6,2))'::box, \
                      '[(0,0),(4,0)]'::lseg ?# '((5,5),(7,7))'::box, \
                      '[(0,0),(4,0)]'::lseg ?# '[(2,-2),(2,2)]'::line, \
                      '[(0,0),(4,0)]'::lseg ?# '[(0,2),(4,2)]'::line, \
                      '[(0,0),(4,0)]'::lseg ?# '[(2,-2),(2,2)]'::lseg, \
                      '[(0,0),(4,0)]'::lseg ?# '[(5,2),(7,2)]'::lseg, \
                      '{1,0,-2}'::line ?# '{0,1,-2}'::line, \
                      '{1,0,-2}'::line ?# '{1,0,-9}'::line, \
                      '[(0,0),(4,0)]'::path ?# '[(2,-1),(2,1)]'::path, \
                      '[(0,0),(4,0)]'::path ?# '[(5,2),(7,2)]'::path";

    // Act
    let frames = execute_geometry_query("issue-907-intersection-operator-overloads", sql);

    // Assert
    assert_eq!(wire::error_code(&frames), None);
    assert_eq!(geometry_result_types(&frames), vec![(16, 1, -1, 0); 14]);
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
            Some("t".to_owned()),
            Some("f".to_owned()),
            Some("t".to_owned()),
            Some("f".to_owned()),
            Some("t".to_owned()),
            Some("f".to_owned()),
        ]]
    );
}

#[test]
fn should_check_geometry_shape_containment_identity() {
    // Arrange
    let sql = "SELECT '((0,0),(8,8))'::box @> '((2,2),(4,4))'::box, \
                      '((2,2),(4,4))'::box <@ '((0,0),(8,8))'::box, \
                      '((0,0),(8,8))'::box @> '((10,10),(11,11))'::box, \
                      '<(0,0),5>'::circle @> '<(1,1),2>'::circle, \
                      '<(1,1),2>'::circle <@ '<(0,0),5>'::circle, \
                      '<(0,0),5>'::circle @> '<(8,0),1>'::circle, \
                      '((0,0),(8,0),(8,8),(0,8))'::polygon @> \
                          '((2,2),(4,2),(4,4),(2,4))'::polygon, \
                      '((2,2),(4,2),(4,4),(2,4))'::polygon <@ \
                          '((0,0),(8,0),(8,8),(0,8))'::polygon, \
                      '((0,0),(8,0),(8,8),(0,8))'::polygon @> \
                          '((10,10),(11,10),(11,11),(10,11))'::polygon, \
                      point '(1,2)' ~= point '(1,2)', \
                      point '(1,2)' ~= point '(1,2.0001)'";

    // Act
    let frames = execute_geometry_query("issue-907-shape-containment-and-point-sameness", sql);

    // Assert
    assert_eq!(wire::error_code(&frames), None);
    assert_eq!(geometry_result_types(&frames), vec![(16, 1, -1, 0); 11]);
    assert_eq!(
        wire::data_rows(&frames),
        vec![vec![
            Some("t".to_owned()),
            Some("t".to_owned()),
            Some("f".to_owned()),
            Some("t".to_owned()),
            Some("t".to_owned()),
            Some("f".to_owned()),
            Some("t".to_owned()),
            Some("t".to_owned()),
            Some("f".to_owned()),
            Some("t".to_owned()),
            Some("f".to_owned()),
        ]]
    );
}

#[test]
fn should_order_geometry_by_area_length_point_count() {
    // Arrange
    let sql = "SELECT '((0,0),(4,4))'::box = '((10,0),(12,8))'::box, \
                      '((0,0),(4,4))'::box ~= '((10,0),(12,8))'::box, \
                      '((0,0),(4,4))'::box < '((0,0),(8,8))'::box, \
                      '((0,0),(8,8))'::box <= '((0,0),(8,8))'::box, \
                      '((0,0),(8,8))'::box > '((0,0),(4,4))'::box, \
                      '((0,0),(8,8))'::box >= '((0,0),(8,8))'::box, \
                      '<(0,0),2>'::circle = '<(10,0),2>'::circle, \
                      '<(0,0),2>'::circle ~= '<(10,0),2>'::circle, \
                      '<(0,0),2>'::circle < '<(0,0),3>'::circle, \
                      '<(0,0),3>'::circle <= '<(0,0),3>'::circle, \
                      '<(0,0),3>'::circle > '<(0,0),2>'::circle, \
                      '<(0,0),2>'::circle >= '<(0,0),2>'::circle, \
                      '<(0,0),2>'::circle <> '<(0,0),3>'::circle, \
                      '[(0,0),(3,4)]'::lseg = '[(0,0),(3,4)]'::lseg, \
                      '[(0,0),(3,4)]'::lseg < '[(0,0),(6,8)]'::lseg, \
                      '[(0,0),(3,4)]'::lseg <= '[(0,0),(3,4)]'::lseg, \
                      '[(0,0),(6,8)]'::lseg > '[(0,0),(3,4)]'::lseg, \
                      '[(0,0),(3,4)]'::lseg >= '[(0,0),(3,4)]'::lseg, \
                      '[(0,0),(3,4)]'::lseg <> '[(0,0),(6,8)]'::lseg, \
                      '[(0,0),(3,4)]'::path = '[(0,0),(3,4)]'::path, \
                      '[(0,0),(3,4)]'::path < '[(0,0),(3,4),(6,4)]'::path, \
                      '[(0,0),(3,4)]'::path <= '[(0,0),(3,4)]'::path, \
                      '[(0,0),(3,4),(6,4)]'::path > '[(0,0),(3,4)]'::path, \
                      '[(0,0),(3,4)]'::path >= '[(0,0),(3,4)]'::path, \
                      '((0,0),(4,0),(4,4),(0,4))'::polygon ~= \
                          '((4,4),(4,0),(0,0),(0,4))'::polygon";

    // Act
    let frames = execute_geometry_query("issue-907-area-length-count-comparisons", sql);

    // Assert
    assert_eq!(wire::error_code(&frames), None);
    assert_eq!(geometry_result_types(&frames), vec![(16, 1, -1, 0); 25]);
    assert_eq!(
        wire::data_rows(&frames),
        vec![vec![
            Some("t".to_owned()),
            Some("f".to_owned()),
            Some("t".to_owned()),
            Some("t".to_owned()),
            Some("t".to_owned()),
            Some("t".to_owned()),
            Some("t".to_owned()),
            Some("f".to_owned()),
            Some("t".to_owned()),
            Some("t".to_owned()),
            Some("t".to_owned()),
            Some("t".to_owned()),
            Some("t".to_owned()),
            Some("t".to_owned()),
            Some("t".to_owned()),
            Some("t".to_owned()),
            Some("t".to_owned()),
            Some("t".to_owned()),
            Some("t".to_owned()),
            Some("t".to_owned()),
            Some("t".to_owned()),
            Some("t".to_owned()),
            Some("t".to_owned()),
            Some("t".to_owned()),
            Some("t".to_owned()),
        ]]
    );
}
