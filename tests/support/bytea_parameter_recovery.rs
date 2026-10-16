//! Native parameter failure/recovery fixture for BYTEA integer casts.
use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;

pub struct Case {
    pub label: &'static str,
    pub sql: &'static str,
    pub bad_binary: &'static [u8],
    pub bad_text: &'static [u8],
    pub expected: [&'static [u8]; 2],
    pub oid: i32,
}

pub fn run(case: &Case) -> Vec<fixture::Frames> {
    let good_binary = &[0];
    let good_text = b"\\x00";
    let mut steps = Vec::new();
    for input_format in [0, 1] {
        for output_format in [0, 1] {
            let bad: &[u8] = if input_format == 0 {
                case.bad_text
            } else {
                case.bad_binary
            };
            let good: &[u8] = if input_format == 0 {
                good_text
            } else {
                good_binary
            };
            steps.extend([
                Step::ParseFlush(wire::parse_frame_with_types("p", case.sql, &[17])),
                Step::ThroughReady(vec![
                    wire::describe_statement_frame("p"),
                    wire::sync_frame(),
                ]),
                Step::BindFlush(wire::bind_frame_with_formats(
                    "bad",
                    "p",
                    &[input_format],
                    &[Some(bad)],
                    &[output_format],
                )),
                Step::ThroughReady(vec![wire::sync_frame()]),
                Step::ThroughReady(vec![
                    wire::describe_statement_frame("p"),
                    wire::sync_frame(),
                ]),
                Step::BindFlush(wire::bind_frame_with_formats(
                    "good",
                    "p",
                    &[input_format],
                    &[Some(good)],
                    &[output_format],
                )),
                Step::ThroughReady(vec![
                    wire::describe_portal_frame("good"),
                    wire::execute_frame("good"),
                    wire::sync_frame(),
                ]),
                Step::ThroughReady(vec![wire::close_statement_frame("p"), wire::sync_frame()]),
            ]);
        }
    }
    run_script(case.label, steps)
}

pub fn assert_result(case: &Case, batches: &[fixture::Frames]) {
    assert_eq!(batches.len(), 32);
    for (index, cycle) in batches.as_chunks::<8>().0.iter().enumerate() {
        assert_eq!(
            cycle[0].iter().map(|frame| frame.0).collect::<Vec<_>>(),
            vec![b'1']
        );
        fixture::assert_success(&cycle[1]);
        assert_eq!(wire::error_code(&cycle[2]).as_deref(), Some("22003"));
        assert_eq!(
            cycle[2].iter().map(|frame| frame.0).collect::<Vec<_>>(),
            vec![b'E']
        );
        fixture::assert_success(&cycle[3]);
        fixture::assert_success(&cycle[4]);
        assert_eq!(
            wire::parse_parameter_description(
                &cycle[4]
                    .iter()
                    .find(|frame| frame.0 == b't')
                    .expect("parameters")
                    .1
            ),
            vec![17]
        );
        assert_eq!(
            cycle[5].iter().map(|frame| frame.0).collect::<Vec<_>>(),
            vec![b'2']
        );
        fixture::assert_success(&cycle[6]);
        let fields = wire::parse_row_description(
            &cycle[6]
                .iter()
                .find(|frame| frame.0 == b'T')
                .expect("descriptor")
                .1,
        );
        assert_eq!(
            (fields[0].type_oid, fields[0].format_code),
            (case.oid, i16::try_from(index % 2).expect("format"))
        );
        assert_eq!(
            fixture::data_row_payloads(&cycle[6]),
            vec![case.expected[index % 2].to_vec()]
        );
        fixture::assert_success(&cycle[7]);
    }
}
