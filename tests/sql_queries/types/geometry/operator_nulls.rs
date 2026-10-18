//! NULL propagation and result identity for PostgreSQL geometric operators (#907).

use cassie::types::Value;

use crate::support_sql_fixture::sql_fixture;

#[test]
fn should_return_null_given_null_geometry_operands_when_geometric_operators_execute() {
    // Arrange
    let fixture = sql_fixture("geometry-operator-null-propagation", &[]);
    let control = fixture.execute(
        "SELECT point(1,2) + point(3,4) AS point_sum, \
         point(1,2) <-> point(4,6) AS distance, \
         box(point(0,2), point(2,0)) @> point(1,1) AS contains",
    );
    let null_operators = fixture.execute(
        "SELECT NULL::POINT + point(1,2) AS point_sum, \
         NULL::POINT <-> point(4,6) AS distance, \
         NULL::BOX @> point(1,1) AS contains",
    );

    // Act
    let control = control.expect("non-null geometric operator control");
    let null_operators = null_operators.expect("geometric operators with NULL operands");

    // Assert
    assert_eq!(
        control
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        [600, 701, 16]
    );
    assert_eq!(
        control.rows(),
        &[vec![
            Value::String("(4,6)".to_owned()),
            Value::Float64(5.0),
            Value::Bool(true),
        ]]
    );
    assert_eq!(
        null_operators
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        [600, 701, 16]
    );
    assert_eq!(
        null_operators.rows(),
        &[vec![Value::Null, Value::Null, Value::Null]]
    );
}
