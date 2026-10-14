use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

fn execute_geometry_query(label: &str, sql: &str) -> Vec<(u8, Vec<u8>)> {
    let batches = run_script(
        label,
        vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])],
    );
    batches
        .into_iter()
        .next()
        .expect("one geometry query batch")
}

fn geometry_result_types(frames: &[(u8, Vec<u8>)]) -> Vec<(i32, i16, i32, i16)> {
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("geometry RowDescription");
    wire::parse_row_description(&description.1)
        .iter()
        .map(|column| {
            (
                column.type_oid,
                column.type_size,
                column.type_mod,
                column.format_code,
            )
        })
        .collect()
}

#[test]
fn should_describe_a_typed_null_point_with_postgresql_identity() {
    // Arrange
    let sql = "SELECT NULL::point AS absent_point";

    // Act
    let frames = execute_geometry_query("issue-907-typed-null-point-identity", sql);

    // Assert
    assert_eq!(
        wire::error_code(&frames),
        None,
        "wire errors: {:?}",
        frames
            .iter()
            .filter(|(tag, _)| *tag == b'E')
            .map(|(_, payload)| wire::parse_error_fields(payload))
            .collect::<Vec<_>>()
    );
    assert_eq!(geometry_result_types(&frames), vec![(600, 16, -1, 0)]);
    assert_eq!(wire::data_rows(&frames), vec![vec![None]]);
}

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
fn should_classify_spatial_direction_overlap_relations() {
    // Arrange
    let sql = "SELECT '((0,0),(2,2))'::box << '((4,0),(6,2))'::box, \
                      '((0,0),(2,2))'::box >> '((4,0),(6,2))'::box, \
                      '((0,0),(2,2))'::box <<| '((0,4),(2,6))'::box, \
                      '((0,0),(2,2))'::box |>> '((0,4),(2,6))'::box, \
                      '((0,0),(2,2))'::box &< '((4,0),(6,2))'::box, \
                      '((0,0),(2,2))'::box &> '((4,0),(6,2))'::box, \
                      '((0,0),(2,2))'::box &<| '((0,4),(2,6))'::box, \
                      '((0,0),(2,2))'::box |&> '((0,4),(2,6))'::box, \
                      '((0,0),(3,3))'::box && '((2,2),(4,4))'::box, \
                      '((0,0),(2,2))'::box && '((4,4),(6,6))'::box, \
                      '<(0,0),1>'::circle << '<(4,0),1>'::circle, \
                      '<(0,0),1>'::circle >> '<(4,0),1>'::circle, \
                      '<(0,0),1>'::circle <<| '<(0,4),1>'::circle, \
                      '<(0,0),1>'::circle |>> '<(0,4),1>'::circle, \
                      '<(0,0),1>'::circle &< '<(4,0),1>'::circle, \
                      '<(0,0),1>'::circle &> '<(4,0),1>'::circle, \
                      '<(0,0),1>'::circle &<| '<(0,4),1>'::circle, \
                      '<(0,0),1>'::circle |&> '<(0,4),1>'::circle, \
                      '<(0,0),2>'::circle && '<(3,0),2>'::circle, \
                      '<(0,0),2>'::circle && '<(5,0),2>'::circle, \
                      '((0,0),(2,0),(2,2),(0,2))'::polygon << \
                          '((4,0),(6,0),(6,2),(4,2))'::polygon, \
                      '((0,0),(2,0),(2,2),(0,2))'::polygon >> \
                          '((4,0),(6,0),(6,2),(4,2))'::polygon, \
                      '((0,0),(2,0),(2,2),(0,2))'::polygon <<| \
                          '((0,4),(2,4),(2,6),(0,6))'::polygon, \
                      '((0,0),(2,0),(2,2),(0,2))'::polygon |>> \
                          '((0,4),(2,4),(2,6),(0,6))'::polygon, \
                      '((0,0),(3,0),(3,3),(0,3))'::polygon && \
                          '((2,2),(4,2),(4,4),(2,4))'::polygon, \
                      '((0,0),(3,0),(3,3),(0,3))'::polygon && \
                          '((5,5),(7,5),(7,7),(5,7))'::polygon, \
                      point '(0,0)' << point '(1,0)', \
                      point '(0,0)' >> point '(1,0)', \
                      point '(0,0)' <<| point '(0,1)', \
                      point '(0,0)' |>> point '(0,1)', \
                      '((0,0),(2,2))'::box <^ '((0,4),(2,6))'::box, \
                      '((0,0),(2,2))'::box >^ '((0,4),(2,6))'::box, \
                      point '(0,0)' <^ point '(0,1)', \
                      point '(0,0)' >^ point '(0,1)'";

    // Act
    let frames = execute_geometry_query("issue-907-directional-overlap-operators", sql);

    // Assert
    assert_eq!(wire::error_code(&frames), None);
    assert_eq!(geometry_result_types(&frames), vec![(16, 1, -1, 0); 34]);
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

#[test]
fn should_compare_line_point_operator_values() {
    // Arrange
    let sql = "SELECT '{1,0,-2}'::line = '{2,0,-4}'::line, \
                      '{1,0,-2}'::line = '{0,1,-2}'::line, \
                      point '(1,2)' <> point '(1,2)', \
                      point '(1,2)' <> point '(2,2)'";

    // Act
    let frames = execute_geometry_query("issue-907-line-point-equality-operators", sql);

    // Assert
    assert_eq!(wire::error_code(&frames), None);
    assert_eq!(geometry_result_types(&frames), vec![(16, 1, -1, 0); 4]);
    assert_eq!(
        wire::data_rows(&frames),
        vec![vec![
            Some("t".to_owned()),
            Some("f".to_owned()),
            Some("f".to_owned()),
            Some("t".to_owned()),
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

#[test]
fn should_classify_line_orientation_operators() {
    // Arrange
    let sql = "SELECT ?- '{0,1,-3}'::line, \
                      ?- '[(0,2),(4,2)]'::lseg, \
                      point '(1,2)' ?- point '(4,2)', \
                      point '(1,2)' ?- point '(4,3)', \
                      ?| '{1,0,-3}'::line, \
                      ?| '[(2,0),(2,4)]'::lseg, \
                      point '(2,1)' ?| point '(2,4)', \
                      point '(2,1)' ?| point '(3,4)', \
                      '{1,0,-2}'::line ?|| '{2,0,4}'::line, \
                      '{1,0,-2}'::line ?|| '{0,1,-3}'::line, \
                      '[(0,0),(4,4)]'::lseg ?|| '[(1,0),(3,2)]'::lseg, \
                      '{1,0,-2}'::line ?-| '{0,1,-3}'::line, \
                      '{1,0,-2}'::line ?-| '{2,0,-4}'::line, \
                      '[(0,0),(4,4)]'::lseg ?-| '[(0,4),(4,0)]'::lseg, \
                      '[(0,0),(4,4)]'::lseg ?-| '[(1,0),(3,2)]'::lseg";

    // Act
    let frames = execute_geometry_query("issue-907-orientation-operators", sql);

    // Assert
    assert_eq!(wire::error_code(&frames), None);
    assert_eq!(geometry_result_types(&frames), vec![(16, 1, -1, 0); 15]);
    assert_eq!(
        wire::data_rows(&frames),
        vec![vec![
            Some("t".to_owned()),
            Some("t".to_owned()),
            Some("t".to_owned()),
            Some("f".to_owned()),
            Some("t".to_owned()),
            Some("t".to_owned()),
            Some("t".to_owned()),
            Some("f".to_owned()),
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
