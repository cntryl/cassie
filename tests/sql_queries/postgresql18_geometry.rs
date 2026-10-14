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
