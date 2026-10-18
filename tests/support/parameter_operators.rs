//! Finite parameter operator probes with PostgreSQL 18.6 wire witnesses.

use crate::support_pgwire as wire;
use crate::support_type_metadata_contract::{self as metadata, Frames};

pub struct Case {
    pub sql: &'static str,
    pub supplied: &'static [i32],
    pub resolved: &'static [i32],
    pub text_inputs: &'static [&'static [u8]],
    pub binary_inputs: &'static [&'static [u8]],
    pub column: (i32, i16, i32),
    pub text_output: &'static [u8],
    pub binary_output: &'static [u8],
}

pub fn run(case: &Case) -> Vec<Frames> {
    let setup = [
        ("CREATE TABLE operator_inputs (id INT, b BYTEA)", vec![]),
        (
            "INSERT INTO operator_inputs VALUES (5, '\\x01'::BYTEA)",
            vec![],
        ),
    ];
    let mut cycles = vec![vec![
        wire::parse_frame_with_types("operator", case.sql, case.supplied),
        wire::describe_statement_frame("operator"),
        wire::sync_frame(),
    ]];
    for (input_format, inputs) in [(0, case.text_inputs), (1, case.binary_inputs)] {
        for output_format in [0, 1] {
            let inputs = inputs.iter().map(|input| Some(*input)).collect::<Vec<_>>();
            cycles.push(vec![
                wire::bind_frame_with_formats(
                    "",
                    "operator",
                    &[input_format],
                    &inputs,
                    &[output_format],
                ),
                wire::describe_portal_frame(""),
                wire::execute_frame(""),
                wire::sync_frame(),
            ]);
        }
    }
    metadata::run_cycles("parameter-operator-resolution", &setup, cycles)
}

pub fn assert_case(case: &Case, batches: &[Frames]) {
    metadata::assert_success(&batches[0]);
    let parameters = batches[0]
        .iter()
        .find(|(tag, _)| *tag == b't')
        .expect("ParameterDescription");
    assert_eq!(
        wire::parse_parameter_description(&parameters.1),
        case.resolved
    );
    metadata::assert_single_column_descriptors(&batches[0], case.column, 0, false);
    for (index, frames) in batches[1..].iter().enumerate() {
        let output_format = i16::try_from(index % 2).expect("result format");
        metadata::assert_success(frames);
        metadata::assert_single_column_descriptors(frames, case.column, output_format, false);
        let output = if output_format == 0 {
            case.text_output
        } else {
            case.binary_output
        };
        assert_eq!(
            metadata::data_row_payloads(frames),
            [metadata::one_field_row(output)],
            "{} input format {} output format {output_format}",
            case.sql,
            index / 2,
        );
    }
}

pub fn run_rejected(sql: &str, oids: &[i32], values: &[&str]) -> Frames {
    metadata::run_cycles(
        "rejected-parameter-operator",
        &[],
        vec![vec![
            wire::parse_frame_with_types("operator", sql, oids),
            wire::describe_statement_frame("operator"),
            wire::bind_frame("", "operator", values),
            wire::execute_frame(""),
            wire::sync_frame(),
        ]],
    )
    .pop()
    .expect("rejected parameter operator cycle")
}

pub fn assert_rejected(frames: &Frames) {
    assert!(wire::error_code(frames).is_some());
    assert!(frames.iter().all(|(tag, _)| !matches!(tag, b'D' | b'C')));
    assert_eq!(
        frames.last().map(|frame| frame.1.as_slice()),
        Some(&b"I"[..])
    );
}
