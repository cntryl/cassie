//! Exact transient integer magnitudes; NUMERIC storage remains separately gated.
use crate::support_sql_fixture::sql_fixture;
use cassie::types::Value;

#[test]
fn should_preserve_decimal_digits_given_large_integer_tokens_when_text_casts_execute() {
    // Arrange
    let fixture = sql_fixture("exact-integer-decimal-text", &[]);
    let values = [
        "9223372036854775808",
        "18446744073709551615",
        "9999999999999999999999999999999999999999",
        "-18446744073709551615",
    ];
    // Act
    let outcomes = values.map(|value| fixture.execute(&format!("SELECT ({value})::TEXT")));
    // Assert
    for (outcome, value) in outcomes.into_iter().zip(values) {
        assert_eq!(
            outcome.expect("exact decimal text").rows(),
            &[vec![Value::String(value.into())]]
        );
    }
}

#[test]
fn should_normalize_radix_magnitudes_given_large_integer_tokens_when_text_casts_execute() {
    // Arrange
    let fixture = sql_fixture("exact-integer-radix-text", &[]);
    let cases = [
        ("0xffffffffffffffff", "18446744073709551615"),
        ("0o1777777777777777777777", "18446744073709551615"),
        (
            "0b1000000000000000000000000000000000000000000000000000000000000000",
            "9223372036854775808",
        ),
        ("9_223_372_036_854_775_808", "9223372036854775808"),
    ];
    // Act
    let outcomes = cases.map(|(token, _)| fixture.execute(&format!("SELECT {token}::TEXT")));
    // Assert
    for (outcome, (_, expected)) in outcomes.into_iter().zip(cases) {
        assert_eq!(
            outcome.expect("exact radix text").rows(),
            &[vec![Value::String(expected.into())]]
        );
    }
}

#[test]
fn should_round_float8_once_given_exact_large_integer_tokens_when_explicit_conversion_executes() {
    // Arrange
    let fixture = sql_fixture("exact-integer-float8", &[]);
    // Act
    let outcome = fixture
        .execute("SELECT 18446744073709551615::FLOAT8::TEXT, 9223372036854775808::FLOAT8::TEXT");
    // Assert
    assert_eq!(
        outcome.expect("explicit float conversion").rows(),
        &[vec![
            Value::String("1.8446744073709552e+19".into()),
            Value::String("9.223372036854776e+18".into())
        ]]
    );
}

#[test]
fn should_prune_integer_range_errors_given_dead_case_arms_when_numeric_tokens_are_exact() {
    // Arrange
    let fixture = sql_fixture("exact-integer-case-phase", &[]);
    let tokens = [
        "9223372036854775808",
        "18446744073709551615",
        "0xffffffffffffffff",
        "9999999999999999999999999999999999999999",
    ];
    // Act
    let outcomes = tokens.map(|token| {
        fixture.execute(&format!(
            "SELECT CASE WHEN FALSE THEN {token}::BIGINT ELSE 7 END"
        ))
    });
    // Assert
    for outcome in outcomes {
        assert_eq!(
            outcome.expect("dead numeric conversion").rows(),
            &[vec![Value::Int64(7)]]
        );
    }
}

#[test]
fn should_prune_integer_range_errors_given_deciding_boolean_operands_when_numeric_casts_are_dead() {
    // Arrange
    let fixture = sql_fixture("exact-integer-boolean-phase", &[]);
    // Act
    let outcome = fixture.execute(
        "SELECT FALSE AND (9223372036854775808::BIGINT=0), TRUE OR (18446744073709551615::INT=0)",
    );
    // Assert
    assert_eq!(
        outcome.expect("dead Boolean operands").rows(),
        &[vec![Value::Bool(false), Value::Bool(true)]]
    );
}

#[test]
fn should_prune_integer_range_errors_given_nonnull_coalesce_input_when_numeric_casts_are_dead() {
    // Arrange
    let fixture = sql_fixture("exact-integer-coalesce-phase", &[]);
    // Act
    let outcome = fixture.execute("SELECT COALESCE(1::BIGINT,9223372036854775808::BIGINT)");
    // Assert
    assert_eq!(
        outcome.expect("dead COALESCE conversion").rows(),
        &[vec![Value::Int64(1)]]
    );
}

#[test]
fn should_discard_numeric_conversions_given_unused_values_fields_when_source_constants_inline() {
    // Arrange
    let fixture = sql_fixture("exact-integer-unused-source", &[]);
    // Act
    let outcome = fixture.execute("SELECT 1 FROM (VALUES(9223372036854775808::BIGINT)) s(n)");
    // Assert
    assert_eq!(
        outcome.expect("unused numeric source").rows(),
        &[vec![Value::Int64(1)]]
    );
}

#[test]
fn should_preserve_exact_text_given_values_aliases_when_source_constants_inline() {
    // Arrange
    let fixture = sql_fixture("exact-integer-text-source", &[]);
    // Act
    let outcome = fixture
        .execute("SELECT n FROM (VALUES(9999999999999999999999999999999999999999::TEXT)) s(n)");
    // Assert
    assert_eq!(
        outcome.expect("exact source text").rows(),
        &[vec![Value::String(
            "9999999999999999999999999999999999999999".into()
        )]]
    );
}

#[test]
fn should_preserve_bigint_minimum_given_cast_function_syntax_when_sign_belongs_to_operand() {
    // Arrange
    let fixture = sql_fixture("exact-integer-minimum", &[]);
    // Act
    let outcome = fixture.execute("SELECT CAST(-9223372036854775808 AS BIGINT)");
    // Assert
    assert_eq!(
        outcome.expect("signed minimum").rows(),
        &[vec![Value::Int64(i64::MIN)]]
    );
}

#[test]
fn should_apply_varchar_width_given_exact_integer_digits_when_explicit_text_conversion_executes() {
    // Arrange
    let fixture = sql_fixture("exact-integer-varchar", &[]);
    // Act
    let outcome = fixture.execute("SELECT 9223372036854775808::VARCHAR(5)");
    // Assert
    assert_eq!(
        outcome.expect("bounded VARCHAR").rows(),
        &[vec![Value::String("92233".into())]]
    );
}

#[test]
fn should_canonicalize_signs_given_leading_zero_integer_tokens_when_exact_text_casts_execute() {
    // Arrange
    let fixture = sql_fixture("exact-integer-canonical-signs", &[]);
    let cases = [
        ("+0009223372036854775808", "9223372036854775808"),
        ("-00018446744073709551615", "-18446744073709551615"),
        ("+0xffffffffffffffff", "18446744073709551615"),
        ("-0xffffffffffffffff", "-18446744073709551615"),
    ];
    // Act
    let outcomes = cases.map(|(token, _)| fixture.execute(&format!("SELECT ({token})::TEXT")));
    // Assert
    for (outcome, (_, expected)) in outcomes.into_iter().zip(cases) {
        assert_eq!(
            outcome.expect("canonical exact digits").rows(),
            &[vec![Value::String(expected.into())]]
        );
    }
}

// PostgreSQL 18.6 materializes integer tokens beyond int8 as exact numeric
// values (oracle-corrections.md).
#[test]
fn should_materialize_exact_numeric_values_given_integer_tokens_beyond_int8() {
    // Arrange
    let fixture = sql_fixture("exact-integer-numeric-boundary", &[]);
    let cases = [
        ("SELECT 9223372036854775809", "9223372036854775809"),
        (
            "SELECT n FROM (VALUES(9223372036854775809)) v(n)",
            "9223372036854775809",
        ),
        ("SELECT 9223372036854775809::NUMERIC", "9223372036854775809"),
        (
            "SELECT CASE WHEN FALSE THEN 9223372036854775809::NUMERIC ELSE 0 END",
            "0",
        ),
    ];
    // Act
    let outcomes = cases.map(|(sql, _)| fixture.execute(sql));
    let narrowed = fixture.execute("SELECT 9223372036854775809::DECIMAL::FLOAT8");
    // Assert
    for (outcome, (sql, expected)) in outcomes.into_iter().zip(cases) {
        assert_eq!(
            outcome.expect(sql).rows(),
            &[vec![Value::String(expected.to_owned())]],
            "{sql}"
        );
    }
    assert_eq!(
        narrowed.expect("numeric to float8").rows(),
        &[vec![Value::Float64(9.223_372_036_854_776e18)]]
    );
}

#[test]
fn should_release_numeric_clone_reservations_given_repeated_values_fields_when_memory_admission_rejects_expansion(
) {
    // Arrange
    let mut config = cassie::config::CassieRuntimeConfig::from_env().expect("config");
    config.limits.query_memory_budget_bytes = 512 * 1024;
    let fixture = crate::support_sql_fixture::sql_fixture_with_config(
        "exact-numeric-clone-admission",
        &[],
        config,
    );
    let fields = vec!["n"; 512].join(",");
    let digits = "9".repeat(2048);
    let sql = format!("SELECT {fields} FROM (VALUES ({digits}::TEXT)) v(n) WHERE FALSE");
    let before = fixture.cassie.metrics()["query"]["current_accounted_memory_bytes"].clone();
    // Act
    let outcome = fixture.execute(&sql);
    let after = fixture.cassie.metrics()["query"]["current_accounted_memory_bytes"].clone();
    let recovery = fixture.execute("SELECT 9223372036854775808::TEXT");
    // Assert
    assert!(
        matches!(outcome, Err(cassie::CassieError::ResourceLimit(_))),
        "{outcome:?}"
    );
    assert_eq!(before, after);
    assert_eq!(
        recovery
            .expect("released numeric clone reservations")
            .rows(),
        &[vec![Value::String("9223372036854775808".into())]]
    );
}

#[test]
fn should_store_existing_primitive_values_given_exact_numeric_casts_when_insert_select_executes() {
    // Arrange
    let fixture = sql_fixture(
        "exact-numeric-primitive-storage",
        &["CREATE TABLE exact_cast_sink(digits TEXT, f FLOAT8)"],
    );
    // Act
    let inserted = fixture.execute("INSERT INTO exact_cast_sink SELECT 18446744073709551615::TEXT, 18446744073709551615::FLOAT8");
    let selected = fixture.execute("SELECT digits, f::TEXT FROM exact_cast_sink");
    // Assert
    assert!(inserted.is_ok(), "{inserted:?}");
    assert_eq!(
        selected.expect("existing primitive storage").rows(),
        &[vec![
            Value::String("18446744073709551615".into()),
            Value::String("1.8446744073709552e+19".into())
        ]]
    );
}
