//! Forward `generate_subscripts` traversal with a nondefault array lower bound (#808).

use cassie::types::Value;

use crate::support_sql_fixture::sql_fixture;

#[test]
fn should_return_ascending_subscripts_given_nondefault_array_bounds_when_generate_subscripts_uses_forward_order(
) {
    // Arrange
    let fixture = sql_fixture("forward-subscripts-nondefault-array-bound", &[]);
    let bounds = fixture.execute(
        "SELECT array_lower('[0:2]={10,20,30}'::INT4[], 1), \
         array_upper('[0:2]={10,20,30}'::INT4[], 1)",
    );
    let actual = fixture.execute(
        "SELECT subscript FROM generate_subscripts( \
             '[0:2]={10,20,30}'::INT4[], 1 \
         ) AS indexes(subscript) ORDER BY subscript",
    );

    // Act
    let bounds = bounds.expect("array bounds control");
    let actual = actual.expect("forward subscripts of a nondefault-bound array");

    // Assert
    assert_eq!(bounds.rows(), &[vec![Value::Int64(0), Value::Int64(2)]]);
    assert_eq!(
        actual.columns()[0].type_oid,
        23,
        "generate_subscripts returns int4"
    );
    assert_eq!(
        actual.rows(),
        &[
            vec![Value::Int64(0)],
            vec![Value::Int64(1)],
            vec![Value::Int64(2)],
        ]
    );
}
