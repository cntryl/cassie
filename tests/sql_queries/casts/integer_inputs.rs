//! Existing-width integer conversion grammar and typed VALUES arithmetic.
use crate::support_sql_fixture::sql_fixture;
use cassie::types::Value;

#[test]
fn should_accept_integer_text_spellings_given_existing_widths_when_explicit_casts_execute() {
    // Arrange
    let fixture = sql_fixture("integer-cast-spellings", &[]);
    let cases = [
        ("SMALLINT", "0x_7f_ff", 21, 32_767),
        ("SMALLINT", "-0x_8000", 21, -32_768),
        ("INT", "0o_17_777", 23, 8_191),
        ("INT", "-0b_1000", 23, -8),
        ("INT", " \t+1_234\r\n", 23, 1_234),
        ("INT", "\u{b}42\u{b}", 23, 42),
        ("BIGINT", "9_007_199_254_740_993", 20, 9_007_199_254_740_993),
        ("BIGINT", "-0x8000_0000_0000_0000", 20, i64::MIN),
        ("BIGINT", "0x7fff_ffff_ffff_ffff", 20, i64::MAX),
    ];
    // Act
    let outcomes = cases.map(|(kind, input, oid, expected)| {
        let sql = format!("SELECT CAST($1 AS {kind}) AS n");
        (
            fixture
                .cassie
                .execute_sql(&fixture.session, &sql, vec![Value::String(input.into())]),
            oid,
            expected,
        )
    });
    // Assert
    for (outcome, oid, expected) in outcomes {
        let outcome = outcome.expect("checked integer text cast");
        assert_eq!(outcome.columns()[0].type_oid, oid);
        assert_eq!(outcome.rows(), &[vec![Value::Int64(expected)]]);
    }
}

#[test]
fn should_resolve_integer_widths_given_typed_values_operands_when_arithmetic_executes() {
    // Arrange
    let fixture = sql_fixture("values-integer-operators", &[]);
    let cases = [
        ("VALUES (1::SMALLINT + 2::SMALLINT)", 21, Value::Int64(3)),
        ("VALUES (3::SMALLINT * 4::SMALLINT)", 21, Value::Int64(12)),
        ("VALUES ((-5)::INT4 / 2::INT4)", 23, Value::Int64(-2)),
        (
            "VALUES (32767::SMALLINT + 1::INT4)",
            23,
            Value::Int64(32_768),
        ),
        (
            "VALUES (9007199254740993::BIGINT + 2)",
            20,
            Value::Int64(9_007_199_254_740_995),
        ),
        ("VALUES (NULL + 1)", 23, Value::Null),
    ];
    // Act
    let outcomes = cases.map(|(sql, oid, expected)| (fixture.execute(sql), oid, expected));
    // Assert
    for (outcome, oid, expected) in outcomes {
        let outcome = outcome.expect("native integer operator width");
        assert_eq!(outcome.columns()[0].type_oid, oid);
        assert_eq!(outcome.rows(), &[vec![expected]]);
    }
}

#[test]
fn should_retain_integer_source_types_given_empty_outputs_when_values_arithmetic_is_bound() {
    // Arrange
    let fixture = sql_fixture("values-integer-empty-types", &[]);
    // Act
    let empty = fixture.execute("SELECT n FROM (VALUES (1 + 2)) AS v(n) WHERE FALSE");
    let aggregate = fixture.execute(
        "SELECT SUM(n) FROM (VALUES (1::SMALLINT + 2::SMALLINT), (NULL::SMALLINT)) AS v(n)",
    );
    // Assert
    let empty = empty.expect("empty INT4 arithmetic");
    assert_eq!(empty.columns()[0].type_oid, 23);
    assert_eq!(empty.rows(), Vec::<Vec<Value>>::new());
    let aggregate = aggregate.expect("integer aggregate over arithmetic VALUES");
    assert_eq!(aggregate.columns()[0].type_oid, 20);
    assert_eq!(aggregate.rows(), &[vec![Value::Int64(3)]]);
}

#[test]
fn should_preserve_json_conversion_boundaries_given_shared_text_parsing_when_integer_casts_execute()
{
    // Arrange
    let fixture = sql_fixture("integer-json-cast-boundary", &[]);
    // Act
    let decimal = fixture.execute("SELECT CAST(CAST('123' AS JSON) AS INT)");
    let radix = fixture.execute("SELECT CAST(CAST('0x_FF' AS JSON) AS INT)");
    // Assert
    assert_eq!(
        decimal.expect("existing decimal JSON conversion").rows(),
        &[vec![Value::Int64(123)]]
    );
    assert!(
        radix.is_err(),
        "shared text grammar must not broaden JSON conversion: {radix:?}"
    );
}

#[test]
fn should_preserve_expression_depth_given_nested_integer_addition_when_values_are_bound() {
    // Arrange
    let fixture = sql_fixture("values-integer-depth", &[]);
    let expression = std::iter::repeat_n("1", 64).collect::<Vec<_>>().join(" + ");
    let sql = format!("VALUES ({expression})");
    // Act
    let outcome = fixture.execute(&sql);
    // Assert
    let outcome = outcome.expect("resolved operators do not add cast nesting");
    assert_eq!(outcome.columns()[0].type_oid, 23);
    assert_eq!(outcome.rows(), &[vec![Value::Int64(64)]]);
}

#[test]
fn should_preserve_dead_case_branches_given_integer_division_when_constant_values_are_planned() {
    // Arrange
    let fixture = sql_fixture("values-integer-dead-case", &[]);
    // Act
    let outcome = fixture.execute("VALUES (CASE WHEN FALSE THEN 1 / 0 ELSE 7 END)");
    // Assert
    let outcome = outcome.expect("dead integer division is not folded");
    assert_eq!(outcome.columns()[0].type_oid, 23);
    assert_eq!(outcome.rows(), &[vec![Value::Int64(7)]]);
}
