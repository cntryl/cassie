//! Quoted unknown input is checked before pruning, unlike typed conversions.
use crate::support_primitive_literal_inputs::INVALID;
use crate::support_sql_fixture::sql_fixture;
use cassie::types::Value;

#[test]
fn should_validate_quoted_inputs_given_dead_case_arms_when_primitive_casts_are_bound() {
    // Arrange
    let fixture = sql_fixture("primitive-input-dead-case", &[]);
    // Act
    let outcomes = INVALID
        .iter()
        .map(|case| {
            fixture.execute(&format!(
                "SELECT CASE WHEN FALSE THEN '{}'::{} ELSE NULL::{} END",
                case.input, case.sql_type, case.sql_type
            ))
        })
        .collect::<Vec<_>>();
    // Assert
    for (outcome, case) in outcomes.into_iter().zip(INVALID) {
        assert!(
            outcome.is_err(),
            "{} {:?} expected {}: {outcome:?}",
            case.sql_type,
            case.input,
            case.state
        );
    }
}

#[test]
fn should_validate_quoted_inputs_given_unused_values_fields_when_source_constants_are_pruned() {
    // Arrange
    let fixture = sql_fixture("primitive-input-unused-values", &[]);
    // Act
    let outcomes = INVALID
        .iter()
        .map(|case| {
            fixture.execute(&format!(
                "SELECT 1 FROM (VALUES ('{}'::{})) v(n)",
                case.input, case.sql_type
            ))
        })
        .collect::<Vec<_>>();
    // Assert
    for (outcome, case) in outcomes.into_iter().zip(INVALID) {
        assert!(
            outcome.is_err(),
            "{} {:?} expected {}: {outcome:?}",
            case.sql_type,
            case.input,
            case.state
        );
    }
}

#[test]
fn should_prune_typed_text_conversions_given_dead_case_arms_when_input_is_already_text() {
    // Arrange
    let fixture = sql_fixture("primitive-input-typed-text-case", &[]);
    // Act
    let outcomes = INVALID
        .iter()
        .map(|case| {
            fixture.execute(&format!(
                "SELECT CASE WHEN FALSE THEN CAST('{}' AS TEXT)::{} ELSE NULL::{} END",
                case.input, case.sql_type, case.sql_type
            ))
        })
        .collect::<Vec<_>>();
    // Assert
    for outcome in outcomes {
        assert_eq!(
            outcome.expect("dead typed conversion").rows(),
            &[vec![Value::Null]]
        );
    }
}

#[test]
fn should_prune_typed_text_conversions_given_unused_values_fields_when_input_is_already_text() {
    // Arrange
    let fixture = sql_fixture("primitive-input-typed-text-values", &[]);
    // Act
    let outcomes = INVALID
        .iter()
        .map(|case| {
            fixture.execute(&format!(
                "SELECT 1 FROM (VALUES (CAST('{}' AS TEXT)::{})) v(n)",
                case.input, case.sql_type
            ))
        })
        .collect::<Vec<_>>();
    // Assert
    for outcome in outcomes {
        assert_eq!(
            outcome.expect("unused typed conversion").rows(),
            &[vec![Value::Int64(1)]]
        );
    }
}

#[test]
fn should_preserve_boolean_prefixes_given_ascii_whitespace_when_quoted_casts_execute() {
    // Arrange
    let fixture = sql_fixture("primitive-boolean-prefixes", &[]);
    let cases = [
        ("t", true),
        ("TR", true),
        ("yes", true),
        ("ON", true),
        ("1", true),
        ("f", false),
        ("fa", false),
        ("n", false),
        ("of", false),
        ("0", false),
        ("\u{b}\ttrue\n", true),
    ];
    // Act
    let outcomes = cases.map(|(input, _)| fixture.execute(&format!("SELECT '{input}'::BOOLEAN")));
    // Assert
    for (outcome, (_, expected)) in outcomes.into_iter().zip(cases) {
        assert_eq!(
            outcome.expect("native Boolean spelling").rows(),
            &[vec![Value::Bool(expected)]]
        );
    }
}

#[test]
fn should_preserve_integer_extrema_given_quoted_input_when_primitive_widths_are_bound() {
    // Arrange
    let fixture = sql_fixture("primitive-quoted-integer-extrema", &[]);
    // Act
    let outcome = fixture.execute("SELECT '-32768'::SMALLINT, '32767'::SMALLINT, '-2147483648'::INT, '2147483647'::INT, '-9223372036854775808'::BIGINT, '9223372036854775807'::BIGINT");
    // Assert
    assert_eq!(
        outcome.expect("quoted integer endpoints").rows(),
        &[vec![
            Value::Int64(-32768),
            Value::Int64(32767),
            Value::Int64(-2_147_483_648),
            Value::Int64(2_147_483_647),
            Value::Int64(i64::MIN),
            Value::Int64(i64::MAX)
        ]]
    );
}

#[test]
fn should_validate_quoted_inputs_given_nonnull_coalesce_inputs_when_later_arguments_are_pruned() {
    // Arrange
    let fixture = sql_fixture("primitive-input-coalesce", &[]);
    // Act
    let outcomes = INVALID
        .iter()
        .map(|case| {
            fixture.execute(&format!(
                "SELECT COALESCE({}::{}, '{}'::{})",
                if case.sql_type == "BOOLEAN" {
                    "TRUE"
                } else {
                    "0"
                },
                case.sql_type,
                case.input,
                case.sql_type
            ))
        })
        .collect::<Vec<_>>();
    // Assert
    for outcome in outcomes {
        assert!(outcome.is_err(), "{outcome:?}");
    }
}

#[test]
fn should_validate_quoted_inputs_given_false_filters_when_result_rows_are_discarded() {
    // Arrange
    let fixture = sql_fixture("primitive-input-empty-result", &[]);
    // Act
    let outcomes = INVALID
        .iter()
        .map(|case| {
            fixture.execute(&format!(
                "SELECT '{}'::{} WHERE FALSE",
                case.input, case.sql_type
            ))
        })
        .collect::<Vec<_>>();
    // Assert
    for outcome in outcomes {
        assert!(outcome.is_err(), "{outcome:?}");
    }
}

#[test]
fn should_validate_quoted_inputs_given_exists_targets_when_output_expressions_are_pruned() {
    // Arrange
    let fixture = sql_fixture("primitive-input-exists-target", &[]);
    // Act
    let outcome = fixture.execute("SELECT EXISTS(SELECT 'bad'::INT)");
    // Assert
    assert!(outcome.is_err(), "{outcome:?}");
}

#[test]
fn should_validate_quoted_inputs_given_unused_ctes_when_relations_are_bound() {
    // Arrange
    let fixture = sql_fixture("primitive-input-unused-cte", &[]);
    // Act
    let outcome = fixture.execute("WITH unused AS (SELECT 'bad'::INT) SELECT 1");
    // Assert
    assert!(outcome.is_err(), "{outcome:?}");
}

#[test]
fn should_fold_typed_text_input_errors_given_false_filters_when_immutable_conversions_are_live() {
    // Arrange
    let fixture = sql_fixture("primitive-input-typed-text-zero-rows", &[]);
    // Act
    let outcomes = INVALID
        .iter()
        .map(|case| {
            fixture.execute(&format!(
                "SELECT CAST('{}' AS TEXT)::{} WHERE FALSE",
                case.input, case.sql_type
            ))
        })
        .collect::<Vec<_>>();
    // Assert
    for (outcome, case) in outcomes.into_iter().zip(INVALID) {
        assert!(
            outcome.is_err(),
            "{} {:?} expected {}: {outcome:?}",
            case.sql_type,
            case.input,
            case.state
        );
    }
}

#[test]
fn should_preserve_typed_boolean_values_given_null_rows_when_source_columns_are_projected() {
    // Arrange
    let fixture = sql_fixture("primitive-input-valid-boolean-values", &[]);
    // Act
    let outcome = fixture
        .execute("SELECT n FROM (VALUES('of'::BOOLEAN),(NULL::BOOLEAN),('tr'::BOOLEAN)) v(n)");
    // Assert
    assert_eq!(
        outcome.expect("typed Boolean VALUES").rows(),
        &[
            vec![Value::Bool(false)],
            vec![Value::Null],
            vec![Value::Bool(true)]
        ]
    );
}

#[test]
fn should_match_native_boolean_input_given_prefixes_and_control_characters_when_quoted_casts_execute(
) {
    // Arrange
    let fixture = sql_fixture("primitive-native-boolean-matrix", &[]);
    let cases = crate::support_primitive_literal_inputs::BOOLEANS;
    // Act
    let outcomes = cases
        .iter()
        .map(|(input, _)| fixture.execute(&format!("SELECT '{input}'::BOOLEAN")))
        .collect::<Vec<_>>();
    // Assert
    for (outcome, (input, expected)) in outcomes.into_iter().zip(cases) {
        if let Some(value) = expected {
            assert_eq!(
                outcome.expect("native Boolean spelling").rows(),
                &[vec![Value::Bool(*value)]],
                "{input:?}"
            );
        } else {
            assert!(outcome.is_err(), "{input:?}: {outcome:?}");
        }
    }
}
