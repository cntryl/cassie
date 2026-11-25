use cassie::types::Value;

use crate::support_sql_fixture::sql_fixture;

#[test]
fn should_raise_integer_out_of_range_when_abs_function_is_applied_to_int4_minimum() {
    // Arrange
    let fixture = sql_fixture("issue-1476-abs-int4-minimum", &[]);
    fixture
        .execute("CREATE TABLE abs_int4_minimum (v integer)")
        .unwrap();
    fixture
        .execute("INSERT INTO abs_int4_minimum VALUES (-2147483648)")
        .unwrap();

    // Act
    let result = fixture.execute("SELECT abs(v) FROM abs_int4_minimum");
    let explicit_argument_type = fixture.execute("SELECT abs(v::integer) FROM abs_int4_minimum");

    // Assert
    assert!(
        matches!(result, Err(cassie::app::CassieError::Execution(message)) if message == "integer out of range")
    );
    assert!(
        matches!(explicit_argument_type, Err(cassie::app::CassieError::Execution(message)) if message == "integer out of range")
    );
}

#[test]
fn should_reject_int4_abs_overflow_before_an_outer_bigint_cast() {
    // Arrange
    let fixture = sql_fixture("issue-1476-abs-outer-bigint-cast", &[]);
    fixture
        .execute("CREATE TABLE abs_outer_cast (v integer)")
        .unwrap();
    fixture
        .execute("INSERT INTO abs_outer_cast VALUES (-2147483648)")
        .unwrap();

    // Act
    let overflow = fixture.execute("SELECT abs(v::integer)::bigint FROM abs_outer_cast");
    let widened_value = fixture.execute("SELECT abs(7::integer)::bigint");

    // Assert
    assert!(
        matches!(overflow, Err(cassie::app::CassieError::Execution(message)) if message == "integer out of range")
    );
    let widened_value = widened_value.expect("outer bigint cast retains its declared result type");
    assert_eq!(widened_value.rows(), &[vec![Value::Int64(7)]]);
    assert_eq!(widened_value.columns()[0].type_oid, 20);
}

#[test]
fn should_raise_smallint_out_of_range_when_abs_function_is_applied_to_int2_minimum() {
    // Arrange
    let fixture = sql_fixture("issue-1476-abs-int2-minimum", &[]);
    fixture
        .execute("CREATE TABLE abs_int2_minimum (v smallint)")
        .unwrap();
    fixture
        .execute("INSERT INTO abs_int2_minimum VALUES (-32768)")
        .unwrap();

    // Act
    let result = fixture.execute("SELECT abs(v) FROM abs_int2_minimum");

    // Assert
    assert!(
        matches!(result, Err(cassie::app::CassieError::Execution(message)) if message == "smallint out of range")
    );
}

#[test]
fn should_preserve_abs_result_width_for_int4_boundary_value() {
    // Arrange
    let fixture = sql_fixture("issue-1476-abs-int4-boundary", &[]);

    // Act
    let result = fixture.execute("SELECT abs(-2147483647::integer)");

    // Assert
    let result = result.expect("non-minimum int4 absolute value");
    assert_eq!(result.rows(), &[vec![Value::Int64(2_147_483_647)]]);
    assert_eq!(result.columns()[0].type_oid, 23);
}

#[test]
fn should_preserve_abs_output_alias_for_int4_value() {
    // Arrange
    let fixture = sql_fixture("issue-1476-abs-int4-alias", &[]);

    // Act
    let result = fixture.execute("SELECT abs(-2147483647::integer) AS magnitude");

    // Assert
    let result = result.expect("non-minimum int4 absolute value with alias");
    assert_eq!(result.columns()[0].name, "magnitude");
}

#[test]
fn should_preserve_null_result_for_int4_abs() {
    // Arrange
    let fixture = sql_fixture("issue-1476-abs-int4-null", &[]);

    // Act
    let result = fixture.execute("SELECT abs(NULL::integer)");

    // Assert
    let result = result.expect("NULL remains NULL through integer ABS");
    assert_eq!(result.rows(), &[vec![Value::Null]]);
}

#[test]
fn should_preserve_declared_int4_width_for_null_abs_result() {
    // Arrange
    let fixture = sql_fixture("issue-1476-abs-int4-null-width", &[]);

    // Act
    let result = fixture.execute("SELECT abs(NULL::integer)");

    // Assert
    let result = result.expect("NULL remains NULL through integer ABS");
    assert_eq!(result.columns()[0].type_oid, 23);
}

#[test]
fn should_raise_bigint_out_of_range_for_abs_minimum() {
    // Arrange
    let fixture = sql_fixture("issue-1476-abs-bigint-minimum", &[]);

    // Act
    let result = fixture.execute("SELECT abs('-9223372036854775808'::bigint)");

    // Assert
    assert!(
        matches!(result, Err(cassie::app::CassieError::Execution(message)) if message == "bigint out of range")
    );
}

#[test]
fn should_raise_integer_out_of_range_for_abs_int4_minimum() {
    // Arrange
    let fixture = sql_fixture("issue-1476-abs-int4-minimum", &[]);

    // Act
    let result = fixture.execute("SELECT abs((-2147483648)::integer)");

    // Assert
    assert!(
        matches!(result, Err(cassie::app::CassieError::Execution(message)) if message == "integer out of range")
    );
}

#[test]
fn should_preserve_int4_width_guard_for_at_prefix() {
    // Arrange
    let fixture = sql_fixture("issue-1476-abs-at-prefix-guard", &[]);

    // Act
    let result = fixture.execute("SELECT @ (-2147483648::integer)");

    // Assert
    assert!(
        matches!(result, Err(cassie::app::CassieError::Execution(message)) if message == "integer out of range")
    );
}

#[test]
fn should_preserve_int4_width_guard_for_unary_negation() {
    // Arrange
    let fixture = sql_fixture("issue-1476-abs-unary-negation-guard", &[]);

    // Act
    let result = fixture.execute("SELECT -(-2147483648::integer)");

    // Assert
    assert!(
        matches!(result, Err(cassie::app::CassieError::Execution(message)) if message == "integer out of range")
    );
}

#[test]
fn should_not_evaluate_abs_for_empty_int4_input() {
    // Arrange
    let fixture = sql_fixture("issue-1476-abs-empty-input", &[]);
    fixture
        .execute("CREATE TABLE abs_empty_minimum (v integer)")
        .unwrap();

    // Act
    let result = fixture.execute("SELECT abs(v) FROM abs_empty_minimum WHERE false");

    // Assert
    assert_eq!(
        result.expect("empty input does not evaluate ABS").rows(),
        &[] as &[Vec<Value>]
    );
}

#[test]
fn should_preserve_abs_width_for_nonminimum_smallint_value() {
    // Arrange
    let fixture = sql_fixture("issue-1476-abs-int2-boundary", &[]);

    // Act
    let result = fixture.execute("SELECT abs(-32767::smallint)");

    // Assert
    let result = result.expect("non-minimum smallint absolute value");
    assert_eq!(result.rows(), &[vec![Value::Int64(32767)]]);
    assert_eq!(result.columns()[0].type_oid, 21);
}

#[test]
fn should_not_apply_the_builtin_abs_width_check_to_a_qualified_user_routine() {
    // Arrange
    let fixture = sql_fixture("issue-1476-abs-declared-routine", &[]);
    fixture.execute("CREATE SCHEMA abs_shadow").unwrap();
    fixture
        .execute(
            r#"CREATE FUNCTION abs_shadow.abs(x INT) RETURNS BIGINT IMMUTABLE AS "2147483648::bigint""#,
        )
        .unwrap();
    fixture
        .execute("SET search_path TO abs_shadow, pg_catalog, public")
        .unwrap();

    // Act
    let builtin_result = fixture.execute("SELECT abs((-2147483648)::integer)");
    let result = fixture.execute("SELECT abs_shadow.abs(-1::integer)");

    // Assert
    assert!(
        matches!(builtin_result, Err(cassie::app::CassieError::Execution(message)) if message == "integer out of range")
    );
    let result = result.expect("qualified user routine bypasses builtin ABS lowering");
    assert_eq!(result.rows(), &[vec![Value::Int64(2_147_483_648)]]);
    assert_eq!(result.columns()[0].type_oid, 20);
}
