//! Existing FLOAT8 and TIMESTAMP text identity under #796/#904.
use crate::support_scalar_presentation::{FLOATS, TIMESTAMPS};
use crate::support_sql_fixture::sql_fixture;
use cassie::types::Value;

#[test]
fn should_format_float8_text_given_native_bit_witnesses_when_explicit_casts_execute() {
    // Arrange
    let fixture = sql_fixture("native-float8-text-casts", &[]);
    // Act
    let outcomes = FLOATS
        .iter()
        .map(|case| {
            let sql = format!("SELECT '{}'::FLOAT8::TEXT", case.text);
            (case, fixture.execute(&sql))
        })
        .collect::<Vec<_>>();
    // Assert
    for (case, outcome) in outcomes {
        assert_eq!(
            outcome.expect("FLOAT8 text cast").rows(),
            &[vec![Value::String(case.text.into())]],
            "bits {:016x}",
            case.bits
        );
    }
}

#[test]
fn should_preserve_float8_bits_given_native_text_casts_when_finite_parameters_roundtrip() {
    // Arrange
    let fixture = sql_fixture("native-float8-text-roundtrip", &[]);
    // Act
    let outcomes = FLOATS
        .iter()
        .filter(|case| f64::from_bits(case.bits).is_finite())
        .map(|case| {
            (
                case,
                fixture.cassie.execute_sql(
                    &fixture.session,
                    "SELECT CAST(CAST($1 AS TEXT) AS FLOAT8)",
                    vec![Value::Float64(f64::from_bits(case.bits))],
                ),
            )
        })
        .collect::<Vec<_>>();
    // Assert
    for (case, outcome) in outcomes {
        let result = outcome.expect("finite text roundtrip");
        let Value::Float64(actual) = result.rows()[0][0] else {
            panic!("FLOAT8 output: {result:?}")
        };
        assert_eq!(actual.to_bits(), case.bits, "{}", case.text);
    }
}

#[test]
fn should_preserve_float8_notation_given_conditional_outputs_when_nested_casts_execute() {
    // Arrange
    let fixture = sql_fixture("native-float8-conditional-text", &[]);
    let sql = "SELECT (CASE WHEN TRUE THEN '1e20'::FLOAT8 ELSE '1'::FLOAT8 END)::TEXT, COALESCE(NULL::FLOAT8, '1e-5'::FLOAT8)::TEXT, GREATEST('1e15'::FLOAT8, '1'::FLOAT8)::TEXT";
    // Act
    let outcome = fixture.execute(sql);
    // Assert
    assert_eq!(
        outcome.expect("conditional FLOAT8 text").rows(),
        &[vec![
            Value::String("1e+20".into()),
            Value::String("1e-05".into()),
            Value::String("1e+15".into())
        ]]
    );
}

#[test]
fn should_format_timestamp_text_given_native_precision_cases_when_constant_casts_execute() {
    // Arrange
    let fixture = sql_fixture("native-timestamp-constant-text", &[]);
    // Act
    let outcomes = TIMESTAMPS
        .iter()
        .map(|case| {
            (
                case,
                fixture.execute(&format!("SELECT TIMESTAMP '{}'::TEXT", case.input)),
            )
        })
        .collect::<Vec<_>>();
    // Assert
    for (case, outcome) in outcomes {
        let outcome = outcome.expect("TIMESTAMP text cast");
        assert_eq!(outcome.columns()[0].type_oid, 25);
        assert_eq!(
            outcome.rows(),
            &[vec![Value::String(case.text.into())]],
            "{} (epoch micros {})",
            case.input,
            case.micros
        );
    }
}

#[test]
fn should_format_timestamp_text_given_typed_table_fields_when_conditionals_are_projected() {
    // Arrange
    let fixture = sql_fixture(
        "native-timestamp-table-text",
        &["CREATE TABLE timestamp_text(k INT, v TIMESTAMP)"],
    );
    for (index, case) in TIMESTAMPS.iter().enumerate() {
        fixture
            .execute(&format!(
                "INSERT INTO timestamp_text VALUES({}, '{}')",
                index + 1,
                case.input
            ))
            .expect("seed timestamp");
    }
    // Act
    let outcome = fixture.execute("SELECT v::TEXT, COALESCE(v, NULL)::TEXT, (CASE WHEN k > 0 THEN v ELSE NULL END)::TEXT FROM timestamp_text ORDER BY k");
    // Assert
    assert_eq!(
        outcome.expect("typed conditional text outputs").rows(),
        TIMESTAMPS
            .iter()
            .map(|case| vec![Value::String(case.text.into()); 3])
            .collect::<Vec<_>>()
    );
}

#[test]
fn should_preserve_timestamp_source_identity_given_aggregates_when_cast_projection_is_rewritten() {
    // Arrange
    let fixture = sql_fixture("native-timestamp-aggregate-text", &["CREATE TABLE timestamp_aggregate(v TIMESTAMP)", "INSERT INTO timestamp_aggregate VALUES('1999-12-31 23:59:59.999999'), ('2000-01-01 00:00:00')"]);
    // Act
    let outcome = fixture.execute("SELECT MIN(v)::TEXT, MAX(v)::TEXT FROM timestamp_aggregate");
    // Assert
    assert_eq!(
        outcome.expect("aggregate TIMESTAMP text").rows(),
        &[vec![
            Value::String("1999-12-31 23:59:59.999999".into()),
            Value::String("2000-01-01 00:00:00".into())
        ]]
    );
}

#[test]
fn should_preserve_timestamp_source_identity_given_values_fields_when_constants_are_substituted() {
    // Arrange
    let fixture = sql_fixture("native-timestamp-values-text", &[]);
    // Act
    let outcome = fixture.execute(
        "SELECT v::TEXT FROM (VALUES (TIMESTAMP '2024-02-29 12:34:56.120000')) AS source(v)",
    );
    // Assert
    assert_eq!(
        outcome.expect("propagated TIMESTAMP text").rows(),
        &[vec![Value::String("2024-02-29 12:34:56.12".into())]]
    );
}

#[test]
fn should_preserve_text_identity_given_timestamp_shaped_strings_when_text_is_cast() {
    // Arrange
    let fixture = sql_fixture(
        "timestamp-shaped-text",
        &[
            "CREATE TABLE shaped_text(v TEXT)",
            "INSERT INTO shaped_text VALUES('2024-02-29T12:34:56.120000Z')",
        ],
    );
    // Act
    let outcome = fixture.execute("SELECT v::TEXT, COALESCE(v,NULL)::TEXT FROM shaped_text");
    // Assert
    assert_eq!(
        outcome.expect("genuine TEXT stays TEXT").rows(),
        &[vec![Value::String("2024-02-29T12:34:56.120000Z".into()); 2]]
    );
}

#[test]
fn should_format_float8_text_given_bounded_varchar_casts_when_native_output_fits_the_width() {
    // Arrange
    let fixture = sql_fixture("native-float8-varchar-text", &[]);
    // Act
    let outcomes = FLOATS
        .iter()
        .map(|case| {
            (
                case,
                fixture.execute(&format!(
                    "SELECT '{}'::FLOAT8::VARCHAR(25)::TEXT",
                    case.text
                )),
            )
        })
        .collect::<Vec<_>>();
    // Assert
    for (case, outcome) in outcomes {
        assert_eq!(
            outcome
                .expect("native FLOAT8 spelling fits VARCHAR(25)")
                .rows(),
            &[vec![Value::String(case.text.into())]]
        );
    }
}

#[test]
fn should_format_timestamp_text_given_varchar_casts_when_declared_source_identity_is_available() {
    // Arrange
    let fixture = sql_fixture("native-timestamp-varchar-text", &[]);
    // Act
    let outcomes = TIMESTAMPS
        .iter()
        .map(|case| {
            (
                case,
                fixture.execute(&format!(
                    "SELECT TIMESTAMP '{}'::VARCHAR(40)::TEXT",
                    case.input
                )),
            )
        })
        .collect::<Vec<_>>();
    // Assert
    for (case, outcome) in outcomes {
        assert_eq!(
            outcome.expect("TIMESTAMP to VARCHAR").rows(),
            &[vec![Value::String(case.text.into())]],
            "{} (epoch micros {})",
            case.input,
            case.micros
        );
    }
}

#[test]
fn should_format_time_text_given_native_fraction_cases_when_casts_execute() {
    // Arrange
    let fixture = sql_fixture("native-time-text-casts", &[]);
    // Act
    let outcomes = crate::support_scalar_presentation::TIMES
        .iter()
        .map(|case| {
            (
                case,
                fixture.execute(&format!(
                    "SELECT TIME '{}'::TEXT, TIME '{}'::VARCHAR(30)::TEXT",
                    case.input, case.input
                )),
            )
        })
        .collect::<Vec<_>>();
    // Assert
    for (case, outcome) in outcomes {
        assert_eq!(
            outcome.expect("TIME text casts").rows(),
            &[vec![Value::String(case.text.into()); 2]],
            "{} (midnight micros {})",
            case.input,
            case.micros
        );
    }
}

#[test]
fn should_prune_numeric_casts_given_large_integer_magnitudes_when_they_are_distinct_from_quoted_input(
) {
    // Arrange
    let fixture = sql_fixture("native-numeric-literal-phase", &[]);
    // Act
    let outcome =
        fixture.execute("SELECT CASE WHEN FALSE THEN 9223372036854775808::BIGINT ELSE 0 END");
    // Assert
    assert_eq!(
        outcome
            .expect("numeric conversion stays in its native phase")
            .rows(),
        &[vec![Value::Int64(0)]]
    );
}

#[test]
fn should_preserve_temporal_cast_sources_given_exists_conditions_when_expressions_are_resolved() {
    // Arrange
    let fixture = sql_fixture("native-temporal-exists-casts", &[
        "CREATE TABLE temporal_outer(k INT, v TIMESTAMP, t TIME)",
        "CREATE TABLE temporal_inner(k INT)",
        "INSERT INTO temporal_outer VALUES(1, '2024-02-29 12:34:56.120000', '12:34:56.120000'),(2, '2000-01-01 00:00:00', '00:00:00')",
        "INSERT INTO temporal_inner VALUES(1)",
    ]);
    // Act
    let outcome = fixture.execute("SELECT (CASE WHEN EXISTS(SELECT 1 FROM temporal_inner AS i WHERE i.k=o.k) THEN o.v ELSE NULL END)::TEXT, (CASE WHEN EXISTS(SELECT 1 FROM temporal_inner AS i WHERE i.k=o.k) THEN o.t ELSE NULL END)::TEXT FROM temporal_outer AS o ORDER BY o.k");
    // Assert
    assert_eq!(
        outcome.expect("resolved temporal cast sources").rows(),
        &[
            vec![
                Value::String("2024-02-29 12:34:56.12".into()),
                Value::String("12:34:56.12".into())
            ],
            vec![Value::Null; 2]
        ]
    );
}

#[test]
fn should_preserve_temporal_cast_keys_given_grouped_outputs_when_conditional_values_are_reused() {
    // Arrange
    let fixture = sql_fixture("native-temporal-grouped-casts", &[
        "CREATE TABLE temporal_group(k INT, v TIMESTAMP)",
        "INSERT INTO temporal_group VALUES(1, '2024-02-29 12:34:56.120000'), (2, '2024-02-29 12:34:56.120000')",
    ]);
    // Act
    let outcome = fixture.execute("SELECT COALESCE(v,NULL)::TEXT AS label, COUNT(*) FROM temporal_group GROUP BY COALESCE(v,NULL)::TEXT ORDER BY label");
    // Assert
    assert_eq!(
        outcome.expect("grouped temporal cast identity").rows(),
        &[vec![
            Value::String("2024-02-29 12:34:56.12".into()),
            Value::Int64(2)
        ]]
    );
}

#[test]
fn should_rebind_scalar_cast_sources_given_persisted_views_when_the_database_restarts() {
    // Arrange
    let expected = [vec![
        Value::String("2024-02-29 12:34:56.12".into()),
        Value::String("1e+20".into()),
    ]];
    // Act
    let outcomes = crate::support_scalar_presentation_restart::restart_outputs();
    // Assert
    for result in [outcomes.0, outcomes.1] {
        assert_eq!(result.rows(), &expected);
        assert_eq!(
            result
                .columns()
                .iter()
                .map(|column| column.type_oid)
                .collect::<Vec<_>>(),
            vec![25, 25]
        );
    }
}

#[test]
fn should_preserve_float8_aggregate_sources_given_declared_inputs_when_results_are_cast_to_text() {
    // Arrange
    let fixture = sql_fixture(
        "native-float8-aggregate-text",
        &["CREATE TABLE floating_aggregate(v FLOAT8)"],
    );
    fixture
        .cassie
        .execute_sql(
            &fixture.session,
            "INSERT INTO floating_aggregate VALUES($1), ($2)",
            vec![Value::Float64(1e20), Value::Float64(1e20)],
        )
        .expect("seed declared FLOAT8 inputs");
    // Act
    let outcome = fixture.execute(
        "SELECT SUM(v)::TEXT, AVG(v)::TEXT, MIN(v)::TEXT, MAX(v)::TEXT FROM floating_aggregate",
    );
    // Assert
    assert_eq!(
        outcome.expect("declared FLOAT8 aggregate outputs").rows(),
        &[vec![
            Value::String("2e+20".into()),
            Value::String("1e+20".into()),
            Value::String("1e+20".into()),
            Value::String("1e+20".into())
        ]]
    );
}

#[test]
fn should_preserve_declared_float8_identity_given_integer_assignment_when_text_outputs_are_selected(
) {
    // Arrange
    let fixture = sql_fixture(
        "native-float8-integer-carrier",
        &[
            "CREATE TABLE integer_float(v FLOAT8)",
            "INSERT INTO integer_float VALUES(9007199254740993)",
        ],
    );
    // Act
    let outcome = fixture.execute("SELECT v::TEXT FROM integer_float");
    // Assert
    assert_eq!(
        outcome.expect("declared FLOAT8 conversion").rows(),
        &[vec![Value::String("9.007199254740992e+15".into())]]
    );
}

#[test]
fn should_distinguish_numeric_text_from_float8_given_explicit_casts_when_source_identities_are_selected(
) {
    // Arrange
    let fixture = sql_fixture("native-numeric-cast-identity", &[]);
    // Act
    let outcome =
        fixture.execute("SELECT CAST('1e20' AS NUMERIC)::TEXT, CAST('1e20' AS FLOAT8)::TEXT");
    // Assert
    assert_eq!(
        outcome
            .expect("distinct NUMERIC and FLOAT8 identities")
            .rows(),
        &[vec![
            Value::String("100000000000000000000".into()),
            Value::String("1e+20".into())
        ]]
    );
}
