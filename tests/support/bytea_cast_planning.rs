//! Native-backed Bind branch selection fixture.
use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;

pub struct Case {
    pub label: &'static str,
    pub sql: &'static str,
    pub oid: i32,
    pub good: [&'static [u8]; 2],
    pub bad: [Option<&'static [u8]>; 2],
    pub output: [&'static [u8]; 2],
}

pub fn run(case: &Case) -> Vec<fixture::Frames> {
    let mut steps = Vec::new();
    for input_format in [0, 1] {
        for output_format in [0, 1] {
            let index = usize::try_from(input_format).expect("input format");
            steps.extend([
                Step::ParseFlush(wire::parse_frame_with_types("p", case.sql, &[case.oid])),
                Step::ThroughReady(vec![
                    wire::describe_statement_frame("p"),
                    wire::sync_frame(),
                ]),
                Step::BindFlush(wire::bind_frame_with_formats(
                    "good",
                    "p",
                    &[input_format],
                    &[Some(case.good[index])],
                    &[output_format],
                )),
                Step::ThroughReady(vec![
                    wire::describe_portal_frame("good"),
                    wire::execute_frame("good"),
                    wire::sync_frame(),
                ]),
                Step::BindFlush(wire::bind_frame_with_formats(
                    "bad",
                    "p",
                    &[input_format],
                    &[case.bad[index]],
                    &[output_format],
                )),
                Step::ThroughReady(vec![wire::sync_frame()]),
                Step::ThroughReady(vec![
                    wire::describe_statement_frame("p"),
                    wire::sync_frame(),
                ]),
                Step::BindFlush(wire::bind_frame_with_formats(
                    "recovery",
                    "p",
                    &[input_format],
                    &[Some(case.good[index])],
                    &[output_format],
                )),
                Step::ThroughReady(vec![
                    wire::describe_portal_frame("recovery"),
                    wire::execute_frame("recovery"),
                    wire::sync_frame(),
                ]),
                Step::ThroughReady(vec![wire::close_statement_frame("p"), wire::sync_frame()]),
            ]);
        }
    }
    run_script(case.label, steps)
}

pub fn assert_result(case: &Case, batches: &[fixture::Frames]) {
    assert_eq!(batches.len(), 40);
    for (index, cycle) in batches.as_chunks::<10>().0.iter().enumerate() {
        assert_eq!(
            cycle[0].iter().map(|frame| frame.0).collect::<Vec<_>>(),
            vec![b'1']
        );
        fixture::assert_success(&cycle[1]);
        assert_eq!(
            cycle[2].iter().map(|frame| frame.0).collect::<Vec<_>>(),
            vec![b'2']
        );
        fixture::assert_success(&cycle[3]);
        assert_eq!(
            fixture::data_row_payloads(&cycle[3]),
            vec![case.output[index % 2].to_vec()]
        );
        assert_eq!(wire::error_code(&cycle[4]).as_deref(), Some("22003"));
        assert_eq!(
            cycle[4].iter().map(|frame| frame.0).collect::<Vec<_>>(),
            vec![b'E']
        );
        fixture::assert_success(&cycle[5]);
        fixture::assert_success(&cycle[6]);
        assert_eq!(
            wire::parse_parameter_description(
                &cycle[6]
                    .iter()
                    .find(|frame| frame.0 == b't')
                    .expect("parameters")
                    .1
            ),
            vec![case.oid]
        );
        assert_eq!(
            cycle[7].iter().map(|frame| frame.0).collect::<Vec<_>>(),
            vec![b'2']
        );
        fixture::assert_success(&cycle[8]);
        assert_eq!(
            fixture::data_row_payloads(&cycle[8]),
            vec![case.output[index % 2].to_vec()]
        );
        fixture::assert_success(&cycle[9]);
    }
}
