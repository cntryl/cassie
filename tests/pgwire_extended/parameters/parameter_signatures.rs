//! Input signatures adjudicated against the immutable PostgreSQL 18.6 baseline.

use super::support_parameter_signature_wire::{assert_case, Case};
use super::support_pgwire as wire;
use super::support_type_metadata_contract as fixture;

#[test]
fn should_resolve_unknown_text_inputs_of_existing_scalar_functions() {
    // Arrange
    let cases = [
        Case {
            sql: "SELECT lower($1) AS value",
            parameter_oid: 25,
            column: (25, -1, -1),
            input: b"MiXeD",
            text: b"mixed",
            binary: b"mixed",
        },
        Case {
            sql: "SELECT lower(COALESCE($1, NULL)) AS value",
            parameter_oid: 25,
            column: (25, -1, -1),
            input: b"MiXeD",
            text: b"mixed",
            binary: b"mixed",
        },
        Case {
            sql: "SELECT COALESCE(lower($1), CAST(NULL AS TEXT)) AS value",
            parameter_oid: 25,
            column: (25, -1, -1),
            input: b"MiXeD",
            text: b"mixed",
            binary: b"mixed",
        },
    ];
    let mut requests = Vec::new();
    let mut cycles = Vec::new();
    for (index, case) in cases.iter().enumerate() {
        for oid in [0, 705, 25] {
            for output_format in [0, 1] {
                for null in [false, true] {
                    requests.push((index, output_format, null));
                    cycles.push(fixture::execute_cycle(
                        case.sql,
                        Some((oid, 0, (!null).then_some(case.input))),
                        output_format,
                        true,
                    ));
                }
            }
        }
    }

    // Act
    let batches = fixture::run_cycles("parameter-text-signatures", &[], cycles);

    // Assert
    assert_eq!(batches.len(), requests.len());
    for ((index, output_format, null), frames) in requests.into_iter().zip(batches) {
        assert_case(&frames, &cases[index], output_format, null);
    }
}

#[test]
fn should_resolve_unknown_coalesce_inputs_from_the_selected_common_type() {
    // Arrange
    let cases = [
        Case {
            sql: "SELECT COALESCE($1, NULL) AS value",
            parameter_oid: 25,
            column: (25, -1, -1),
            input: b"MiXeD",
            text: b"MiXeD",
            binary: b"MiXeD",
        },
        Case {
            sql: "SELECT COALESCE($1, CAST(NULL AS BOOLEAN)) AS value",
            parameter_oid: 16,
            column: (16, 1, -1),
            input: b"true",
            text: b"t",
            binary: &[1],
        },
        Case {
            sql: "SELECT COALESCE(CAST(NULL AS INT), $1) AS value",
            parameter_oid: 23,
            column: (23, 4, -1),
            input: b"7",
            text: b"7",
            binary: &[0, 0, 0, 7],
        },
        Case {
            sql: "SELECT COALESCE(CAST(NULL AS BIGINT), $1) AS value",
            parameter_oid: 20,
            column: (20, 8, -1),
            input: b"7",
            text: b"7",
            binary: &[0, 0, 0, 0, 0, 0, 0, 7],
        },
    ];
    let mut requests = Vec::new();
    let mut cycles = Vec::new();
    for (index, case) in cases.iter().enumerate() {
        for oid in [0, 705, case.parameter_oid] {
            for output_format in [0, 1] {
                for null in [false, true] {
                    requests.push((index, output_format, null));
                    cycles.push(fixture::execute_cycle(
                        case.sql,
                        Some((oid, 0, (!null).then_some(case.input))),
                        output_format,
                        true,
                    ));
                }
            }
        }
    }

    // Act
    let batches = fixture::run_cycles("parameter-coalesce-signatures", &[], cycles);

    // Assert
    assert_eq!(batches.len(), requests.len());
    for ((index, output_format, null), frames) in requests.into_iter().zip(batches) {
        assert_case(&frames, &cases[index], output_format, null);
    }
}

#[test]
fn should_resolve_sql_function_inputs_from_the_active_schema() {
    // Arrange
    let setup = [
        ("CREATE SCHEMA signature_text", Vec::new()),
        ("CREATE SCHEMA signature_bool", Vec::new()),
        (
            "CREATE FUNCTION signature_text.signature_pick(x TEXT) RETURNS TEXT AS \"lower(x)\"",
            Vec::new(),
        ),
        (
            "CREATE FUNCTION signature_bool.signature_pick(x BOOLEAN) RETURNS TEXT AS \"CAST(NOT x AS TEXT)\"",
            Vec::new(),
        ),
    ];
    let cases = [
        (
            "signature_text",
            Case {
                sql: "SELECT signature_pick($1) AS value",
                parameter_oid: 25,
                column: (25, -1, -1),
                input: b"TRUE",
                text: b"true",
                binary: b"true",
            },
        ),
        (
            "signature_bool",
            Case {
                sql: "SELECT signature_pick($1) AS value",
                parameter_oid: 16,
                column: (25, -1, -1),
                input: b"TRUE",
                text: b"false",
                binary: b"false",
            },
        ),
    ];
    let mut requests = Vec::new();
    let mut cycles = Vec::new();
    for (index, (schema, case)) in cases.iter().enumerate() {
        for oid in [0, 705, case.parameter_oid] {
            for output_format in [0, 1] {
                for null in [false, true] {
                    cycles.push(vec![wire::simple_query_frame(&format!(
                        "SET search_path TO {schema}"
                    ))]);
                    requests.push((index, output_format, null));
                    cycles.push(fixture::execute_cycle(
                        case.sql,
                        Some((oid, 0, (!null).then_some(case.input))),
                        output_format,
                        true,
                    ));
                }
            }
        }
    }

    // Act
    let batches = fixture::run_cycles("parameter-scoped-function-signatures", &setup, cycles);

    // Assert
    assert_eq!(batches.len(), requests.len() * 2);
    for ((index, output_format, null), pair) in requests.into_iter().zip(batches.as_chunks::<2>().0)
    {
        fixture::assert_success(&pair[0]);
        assert_eq!(
            pair[0].iter().map(|(tag, _)| *tag).collect::<Vec<_>>(),
            b"CZ"
        );
        assert_case(&pair[1], &cases[index].1, output_format, null);
    }
}

#[test]
fn should_preserve_boolean_sinks_of_coalesce_results() {
    // Arrange
    let case = Case {
        sql: "SELECT COALESCE($1, TRUE) AS value WHERE COALESCE($1, TRUE)",
        parameter_oid: 16,
        column: (16, 1, -1),
        input: b"true",
        text: b"t",
        binary: &[1],
    };
    let mut requests = Vec::new();
    let mut cycles = Vec::new();
    for oid in [0, 705, 16] {
        for output_format in [0, 1] {
            for null in [false, true] {
                requests.push(output_format);
                cycles.push(fixture::execute_cycle(
                    case.sql,
                    Some((oid, 0, (!null).then_some(case.input))),
                    output_format,
                    true,
                ));
            }
        }
    }

    // Act
    let batches = fixture::run_cycles("parameter-coalesce-boolean-sinks", &[], cycles);

    // Assert
    assert_eq!(batches.len(), requests.len());
    for (output_format, frames) in requests.into_iter().zip(batches) {
        assert_case(&frames, &case, output_format, false);
    }
}
