//! PostgreSQL 18 geometric value and operator behavior tracked by #907.

use cassie::types::Value;

use crate::support_sql_fixture::sql_fixture;

#[test]
fn should_calculate_geometry_distance_with_polygon_containment() {
    // Arrange
    let fixture = sql_fixture("issue-907-geometry-distance-containment", &[]);
    let sql = "SELECT point '(1,2)' <-> point '(4,6)' = 5, '((0,0),(5,5),(0,5))'::polygon @> point '(1,1)', point '(1,2)' <@ '((0,0),(5,5),(0,5))'::polygon";

    // Act
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result.expect("geometry distance and containment").rows(),
        &[vec![
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true)
        ]]
    );
}

#[test]
fn should_construct_a_point_with_the_postgresql_text_representation() {
    // Arrange
    let fixture = sql_fixture("issue-907-point-constructor", &[]);

    // Act
    let result = fixture.execute("SELECT point(1, 2)::TEXT");

    // Assert
    assert_eq!(
        result.expect("point constructor").rows(),
        &[vec![Value::String("(1,2)".to_owned())]]
    );
}

#[test]
fn should_evaluate_circle_area_containment() {
    // Arrange
    let fixture = sql_fixture("issue-907-circle-construction-and-operators", &[]);
    let sql = "SELECT circle(point '(1,2)', 3) = '<(1,2),3>'::circle, circle(point '(1,2)', 3) @> point '(1,4)', NOT (circle(point '(1,2)', 3) @> point '(1,6)'), area(circle(point '(1,2)', 3))::numeric(6,2) = 28.27";

    // Act
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result
            .expect("circle construction, area, and containment")
            .rows(),
        &[vec![
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
        ]]
    );
}

#[test]
fn should_calculate_box_area() {
    // Arrange
    let fixture = sql_fixture("issue-907-box-area", &[]);
    let sql = "SELECT area(box(point(0,0), point(4,3))) = 12";

    // Act
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(result.expect("box area").rows(), &[vec![Value::Bool(true)]]);
}

#[test]
fn should_calculate_box_center() {
    // Arrange
    let fixture = sql_fixture("issue-907-box-center", &[]);
    let sql = "SELECT center(box(point(0,0), point(4,3)))::text = '(2,1.5)'";

    // Act
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result.expect("box center").rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_evaluate_box_overlap_operator() {
    // Arrange
    let fixture = sql_fixture("issue-907-box-overlap-operator", &[]);
    let sql = "SELECT box(point(0,4),point(4,0)) && box(point(3,5),point(7,1))";

    // Act
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result.expect("box overlap operator").rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_preserve_point_addition_coordinates() {
    // Arrange
    let fixture = sql_fixture("issue-907-point-addition", &[]);

    // Act
    let result = fixture.execute("SELECT (point '(1,2)' + point '(3,4)')::text");

    // Assert
    assert_eq!(
        result.expect("point addition coordinates").rows(),
        &[vec![Value::String("(4,6)".to_owned())]]
    );
}

#[test]
fn should_preserve_point_multiplication_coordinates() {
    // Arrange
    let fixture = sql_fixture("issue-907-point-multiplication", &[]);

    // Act
    let result = fixture.execute("SELECT (point '(1,2)' * point '(3,4)')::text");

    // Assert
    assert_eq!(
        result.expect("point multiplication coordinates").rows(),
        &[vec![Value::String("(-5,10)".to_owned())]]
    );
}

#[test]
fn should_return_line_intersection_coordinates() {
    // Arrange
    let fixture = sql_fixture("issue-907-line-intersection", &[]);

    // Act
    let result = fixture.execute("SELECT ('{1,0,-2}'::line # '{0,1,-3}'::line)::text");

    // Assert
    assert_eq!(
        result.expect("line intersection coordinates").rows(),
        &[vec![Value::String("(2,3)".to_owned())]]
    );
}

#[test]
fn should_return_segment_intersection_coordinates() {
    // Arrange
    let fixture = sql_fixture("issue-907-segment-intersection", &[]);

    // Act
    let result = fixture.execute("SELECT ('[(0,0),(4,0)]'::lseg # '[(2,-2),(2,2)]'::lseg)::text");

    // Assert
    assert_eq!(
        result.expect("segment intersection coordinates").rows(),
        &[vec![Value::String("(2,0)".to_owned())]]
    );
}

#[test]
fn should_measure_point_distances_to_native_shapes() {
    // Arrange
    let fixture = sql_fixture("issue-907-point-shape-distances", &[]);
    let sql = "SELECT point '(1,2)' <-> '[(0,0),(4,0)]'::path, point '(5,2)' <-> '((0,0),(4,4))'::box, point '(5,2)' <-> '((0,0),(4,0),(4,4),(0,4))'::polygon, point '(4,0)' <-> '<(0,0),2>'::circle, point '(4,3)' <-> '{0,1,0}'::line, point '(2,3)' <-> '[(0,0),(4,0)]'::lseg";

    // Act
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result.expect("point-to-shape distances").rows(),
        &[vec![
            Value::Float64(2.0),
            Value::Float64(1.0),
            Value::Float64(1.0),
            Value::Float64(2.0),
            Value::Float64(3.0),
            Value::Float64(3.0),
        ]]
    );
}

#[test]
fn should_normalize_reversed_box_corners() {
    // Arrange
    let fixture = sql_fixture("issue-907-box-corner-normalization", &[]);

    // Act
    let result = fixture.execute("SELECT box(point(0,0), point(4,3))::text");

    // Assert
    assert_eq!(
        result.expect("normalized box corners").rows(),
        &[vec![Value::String("(4,3),(0,0)".to_owned())]]
    );
}

#[test]
fn should_classify_box_point_containment() {
    // Arrange
    let fixture = sql_fixture("issue-907-box-boundary-predicates", &[]);

    // Act
    let result = fixture.execute("SELECT box(point(0,0), point(4,3)) @> point '(4,3)', box(point(0,0), point(4,3)) @> point '(4.01,3)', point '(1,1)' <@ box(point(4,3), point(0,0))");

    // Assert
    assert_eq!(
        result.expect("box boundary predicates").rows(),
        &[vec![
            Value::Bool(true),
            Value::Bool(false),
            Value::Bool(true)
        ]]
    );
}

#[test]
fn should_distinguish_closed_from_open_path_identity() {
    // Arrange
    let fixture = sql_fixture("issue-907-path-closure-identity", &[]);

    // Act
    let result = fixture.execute(
        "SELECT '[(1,2),(3,4)]'::path::text, '((1,2),(3,4))'::path::text, '[(1,2),(3,4)]'::path = '((1,2),(3,4))'::path",
    );

    // Assert
    assert_eq!(
        result.expect("open and closed path identity").rows(),
        &[vec![
            Value::String("[(1,2),(3,4)]".to_owned()),
            Value::String("((1,2),(3,4))".to_owned()),
            Value::Bool(false),
        ]]
    );
}

#[test]
fn should_preserve_path_vertex_count_when_closure_changes() {
    // Arrange
    let fixture = sql_fixture("issue-907-path-closure-vertex-count", &[]);

    // Act
    let result = fixture.execute("SELECT # '[(1,2),(3,4)]'::path, # '((1,2),(3,4))'::path");

    // Assert
    assert_eq!(
        result.expect("path vertex count").rows(),
        &[vec![Value::Int32(2), Value::Int32(2)]]
    );
}

#[test]
fn should_report_path_circle_measurements() {
    // Arrange
    let fixture = sql_fixture("issue-907-shape-measurements", &[]);
    let sql = "SELECT npoints('((0,0),(4,0),(4,3))'::polygon), npoints('((0,0),(4,0),(4,3))'::path), isclosed('((0,0),(4,0),(4,3))'::path), diameter('<(0,0),3>'::circle), radius('<(0,0),3>'::circle)";

    // Act
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result.expect("path and circle measurements").rows(),
        &[vec![
            Value::Int32(3),
            Value::Int32(3),
            Value::Bool(true),
            Value::Float64(6.0),
            Value::Float64(3.0),
        ]]
    );
}
