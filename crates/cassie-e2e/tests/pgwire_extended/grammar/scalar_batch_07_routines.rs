//! Native exact-routine precedence around type-function coercion.
use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;

#[test]
fn should_preserve_exact_routines_given_type_function_names_when_argument_identity_is_known() {
    // Arrange
    let queries = [
        ("SELECT int4('7') AS output", 7),
        ("SELECT int4('7'::TEXT) AS output", 99),
        ("SELECT int4(NULL::TEXT) AS output", 99),
        ("SELECT pg_catalog.int4('7'::TEXT) AS output", 7),
        ("SELECT int4('7'::VARCHAR) AS output", 7),
    ];
    let cycles = queries
        .iter()
        .flat_map(|(sql, _)| [0, 1].map(|format| fixture::execute_cycle(sql, None, format, true)))
        .collect();
    // Act
    let batches = fixture::run_cycles(
        "type-function-routines",
        &[("CREATE FUNCTION int4(x TEXT) RETURNS INT AS \"99\"", vec![])],
        cycles,
    );
    // Assert
    for (index, frames) in batches.iter().enumerate() {
        fixture::assert_success(frames);
        fixture::assert_single_column_descriptors(
            frames,
            (23, 4, -1),
            i16::try_from(index % 2).expect("format"),
            true,
        );
        let value: i32 = queries[index / 2].1;
        let payload = if index % 2 == 0 {
            value.to_string().into_bytes()
        } else {
            value.to_be_bytes().to_vec()
        };
        assert_eq!(
            fixture::data_row_payloads(frames),
            vec![fixture::one_field_row(&payload)]
        );
    }
}

#[test]
fn should_preserve_routine_parameter_inference_given_text_overload_when_type_function_is_prepared()
{
    // Arrange
    let mut cycles = Vec::new();
    for oid in [0, 25, 23] {
        for input_format in [0, 1] {
            for output_format in [0, 1] {
                let binary = 7_i32.to_be_bytes();
                let input: &[u8] = if oid == 23 && input_format == 1 {
                    &binary
                } else {
                    b"7"
                };
                cycles.push(fixture::execute_cycle(
                    "SELECT int4($1) AS output",
                    Some((oid, input_format, Some(input))),
                    output_format,
                    true,
                ));
            }
        }
    }
    // Act
    let batches = fixture::run_cycles(
        "type-function-routine-parameters",
        &[("CREATE FUNCTION int4(x TEXT) RETURNS INT AS \"99\"", vec![])],
        cycles,
    );
    // Assert
    for (index, frames) in batches.iter().enumerate() {
        fixture::assert_success(frames);
        let oid = if index < 8 { 25 } else { 23 };
        let parameters = &frames
            .iter()
            .find(|frame| frame.0 == b't')
            .expect("parameters")
            .1;
        assert_eq!(wire::parse_parameter_description(parameters), vec![oid]);
        let value: i32 = if oid == 25 { 99 } else { 7 };
        let payload = if index % 2 == 0 {
            value.to_string().into_bytes()
        } else {
            value.to_be_bytes().to_vec()
        };
        assert_eq!(
            fixture::data_row_payloads(frames),
            vec![fixture::one_field_row(&payload)]
        );
    }
}

#[test]
fn should_stage_unselected_type_function_inputs_given_pending_types_when_prepared() {
    // Arrange
    // `text(1.5)` and `float8(0.1+0.2)` take native numeric inputs and are
    // covered by should_convert_numeric_literal_type_function_inputs.
    let queries = [
        "SELECT date($1) AS output",
        "SELECT pg_catalog.timestamp($1) AS output",
        "SELECT text('{1}'::INTEGER[]) AS output",
    ];
    let cycles = queries
        .iter()
        .flat_map(|sql| {
            [0, 1].map(|format| {
                fixture::execute_cycle(
                    sql,
                    sql.contains("$1").then_some((0, 0, None)),
                    format,
                    true,
                )
            })
        })
        .collect();
    // Act
    let batches = fixture::run_cycles("type-function-staging", &[], cycles);
    // Assert
    for frames in &batches {
        fixture::assert_unsupported_before_metadata(frames);
    }
}

#[test]
fn should_convert_numeric_literal_type_function_inputs() {
    // Arrange
    let cases = [
        ("SELECT text(1.5) AS output", (25, -1, -1), b"1.5".to_vec()),
        (
            "SELECT float8(0.1+0.2) AS output",
            (701, 8, -1),
            0.3_f64.to_be_bytes().to_vec(),
        ),
    ];
    let cycles = cases
        .iter()
        .map(|(sql, _, _)| fixture::execute_cycle(sql, None, 1, true))
        .collect();
    // Act
    let batches = fixture::run_cycles("type-function-numeric-inputs", &[], cycles);
    // Assert
    // PostgreSQL 18.6: text(numeric) is '1.5'; float8(0.1 + 0.2) is 0.3.
    for ((sql, descriptor, payload), frames) in cases.into_iter().zip(batches) {
        fixture::assert_success(&frames);
        fixture::assert_single_column_descriptors(&frames, descriptor, 1, true);
        assert_eq!(
            fixture::data_row_payloads(&frames),
            vec![fixture::one_field_row(&payload)],
            "{sql}"
        );
    }
}

#[test]
fn should_preserve_unknown_text_coercion_given_text_routine_when_declared_types_differ() {
    // Arrange
    let mut cycles = Vec::new();
    for oid in [0, 25, 23] {
        for input_format in [0, 1] {
            for output_format in [0, 1] {
                let binary = 7_i32.to_be_bytes();
                let input: &[u8] = if oid == 23 && input_format == 1 {
                    &binary
                } else {
                    b"7"
                };
                cycles.push(fixture::execute_cycle(
                    "SELECT text($1) AS output",
                    Some((oid, input_format, Some(input))),
                    output_format,
                    true,
                ));
            }
        }
    }
    // Act
    let batches = fixture::run_cycles(
        "type-function-text-origins",
        &[(
            "CREATE FUNCTION text(x TEXT) RETURNS TEXT AS \"'custom'\"",
            vec![],
        )],
        cycles,
    );
    // Assert
    for (index, frames) in batches.iter().enumerate() {
        fixture::assert_success(frames);
        let oid = if index < 8 { 25 } else { 23 };
        let parameters = &frames
            .iter()
            .find(|frame| frame.0 == b't')
            .expect("parameters")
            .1;
        assert_eq!(wire::parse_parameter_description(parameters), vec![oid]);
        let value: &[u8] = if (4..8).contains(&index) {
            b"custom"
        } else {
            b"7"
        };
        assert_eq!(
            fixture::data_row_payloads(frames),
            vec![fixture::one_field_row(value)]
        );
    }
}

#[test]
fn should_preserve_nested_text_coercion_given_ctes_and_exists_when_text_routine_is_present() {
    // Arrange
    let cases = [
        (
            "WITH c AS (SELECT text($1) AS v) SELECT v AS output FROM c",
            0,
            25,
            "7",
        ),
        (
            "WITH c AS (SELECT text($1) AS v) SELECT v AS output FROM c",
            25,
            25,
            "custom",
        ),
        ("SELECT text(text($1)) AS output", 0, 25, "custom"),
        (
            "SELECT EXISTS(SELECT 1 WHERE text($1)='7') AS output",
            0,
            16,
            "t",
        ),
    ];
    let cycles = cases
        .iter()
        .flat_map(|(sql, oid, _, _)| {
            [0, 1].map(|format| {
                fixture::execute_cycle(sql, Some((*oid, 0, Some(b"7"))), format, true)
            })
        })
        .collect();
    // Act
    let batches = fixture::run_cycles(
        "type-function-text-nesting",
        &[(
            "CREATE FUNCTION text(x TEXT) RETURNS TEXT AS \"'custom'\"",
            vec![],
        )],
        cycles,
    );
    // Assert
    for (index, frames) in batches.iter().enumerate() {
        fixture::assert_success(frames);
        let (_, _, oid, value) = cases[index / 2];
        fixture::assert_single_column_descriptors(
            frames,
            (oid, if oid == 16 { 1 } else { -1 }, -1),
            i16::try_from(index % 2).expect("format"),
            true,
        );
        let payload: &[u8] = if oid == 16 && index % 2 == 1 {
            &[1]
        } else {
            value.as_bytes()
        };
        assert_eq!(
            fixture::data_row_payloads(frames),
            vec![fixture::one_field_row(payload)]
        );
    }
}

#[test]
fn should_preserve_dml_text_coercion_given_text_routine_when_returning_rows() {
    // Arrange
    let mut cycles = Vec::new();
    for oid in [0, 25] {
        for input_format in [0, 1] {
            for output_format in [0, 1] {
                cycles.push(fixture::execute_cycle(
                    "INSERT INTO text_coercion_rows(v) VALUES(text($1)) RETURNING v AS output",
                    Some((oid, input_format, Some(b"7"))),
                    output_format,
                    true,
                ));
            }
        }
    }
    // Act
    let batches = fixture::run_cycles(
        "type-function-text-dml",
        &[
            ("CREATE TABLE text_coercion_rows(v TEXT)", vec![]),
            (
                "CREATE FUNCTION text(x TEXT) RETURNS TEXT AS \"'custom'\"",
                vec![],
            ),
        ],
        cycles,
    );
    // Assert
    for (index, frames) in batches.iter().enumerate() {
        fixture::assert_success(frames);
        let value: &[u8] = if index < 4 { b"7" } else { b"custom" };
        assert_eq!(
            fixture::data_row_payloads(frames),
            vec![fixture::one_field_row(value)]
        );
    }
}

#[test]
fn should_materialize_scalar_values_given_casts_and_predicates_when_rows_are_inserted() {
    // Arrange
    let sql = "INSERT INTO value_expression_rows(v) VALUES(text(1+2)), (CASE WHEN TRUE THEN text(4) ELSE text(5) END), (text((2 BETWEEN SYMMETRIC 3 AND 1) IS TRUE)) RETURNING v AS output";
    let cycles = [0, 1]
        .map(|format| fixture::execute_cycle(sql, None, format, true))
        .to_vec();
    // Act
    let batches = fixture::run_cycles(
        "type-function-computed-values",
        &[("CREATE TABLE value_expression_rows(v TEXT)", vec![])],
        cycles,
    );
    // Assert
    for frames in &batches {
        fixture::assert_success(frames);
        assert_eq!(
            fixture::data_row_payloads(frames),
            [b"3".as_slice(), b"4", b"true"]
                .map(fixture::one_field_row)
                .to_vec()
        );
    }
}

#[test]
fn should_preserve_atomic_values_given_a_later_scalar_error_when_insertion_is_attempted() {
    // Arrange
    let mut cycles = Vec::new();
    for format in [0, 1] {
        cycles.push(fixture::execute_cycle(
            "INSERT INTO value_expression_rows(v) VALUES(text(1)),(text(1/0)) RETURNING v",
            None,
            format,
            true,
        ));
        cycles.push(fixture::execute_cycle(
            "SELECT count(*) AS output FROM value_expression_rows",
            None,
            format,
            true,
        ));
    }
    // Act
    let batches = fixture::run_cycles(
        "type-function-values-atomicity",
        &[("CREATE TABLE value_expression_rows(v TEXT)", vec![])],
        cycles,
    );
    // Assert
    for (index, frames) in batches.iter().enumerate() {
        if index % 2 == 0 {
            assert_eq!(wire::error_code(frames).as_deref(), Some("22012"));
        } else {
            fixture::assert_success(frames);
            let zero = 0_i64.to_be_bytes();
            let payload: &[u8] = if index < 2 { b"0" } else { &zero };
            assert_eq!(
                fixture::data_row_payloads(frames),
                vec![fixture::one_field_row(payload)]
            );
        }
    }
}

#[test]
fn should_reject_aggregate_values_given_scalar_context_when_rows_are_inserted() {
    // Arrange
    let cycles = [0, 1]
        .map(|format| {
            fixture::execute_cycle(
                "INSERT INTO value_expression_rows(v) VALUES(text(count(*)))",
                None,
                format,
                true,
            )
        })
        .to_vec();
    // Act
    let batches = fixture::run_cycles(
        "type-function-values-aggregate-context",
        &[("CREATE TABLE value_expression_rows(v TEXT)", vec![])],
        cycles,
    );
    // Assert
    for frames in &batches {
        assert_eq!(wire::error_code(frames).as_deref(), Some("42803"));
        assert!(!frames.iter().any(|frame| frame.0 == b'D'));
    }
}
