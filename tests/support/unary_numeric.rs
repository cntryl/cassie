//! Exact unary result descriptors and payloads from the native protocol oracle.
use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;

pub fn assert_result(batches: &[fixture::Frames], oid: i32, payloads: [&[u8]; 2]) {
    assert_eq!(batches.len(), 10);
    for (format, batch) in batches.as_chunks::<5>().0.iter().enumerate() {
        assert_eq!(
            batch[0].iter().map(|frame| frame.0).collect::<Vec<_>>(),
            vec![b'1']
        );
        fixture::assert_success(&batch[1]);
        assert_eq!(
            batch[2].iter().map(|frame| frame.0).collect::<Vec<_>>(),
            vec![b'2']
        );
        fixture::assert_success(&batch[3]);
        let description = batch[3]
            .iter()
            .find(|frame| frame.0 == b'T')
            .expect("descriptor");
        let columns = wire::parse_row_description(&description.1);
        assert_eq!(columns.len(), 1);
        assert_eq!(columns[0].type_oid, oid);
        assert_eq!(
            columns[0].format_code,
            i16::try_from(format).expect("format")
        );
        assert_eq!(
            fixture::data_row_payloads(&batch[3]),
            vec![payloads[format].to_vec()]
        );
        fixture::assert_success(&batch[4]);
    }
}

pub fn parse(label: &str, sql: &str) -> Vec<fixture::Frames> {
    use crate::support_parameter_occurrence_wire::{run_script, Step};
    run_script(
        label,
        vec![
            Step::ParseFlush(wire::parse_frame("p", sql)),
            Step::ThroughReady(vec![wire::sync_frame()]),
        ],
    )
}
pub fn assert_parse_failure(batches: &[fixture::Frames], state: &str) {
    assert_eq!(
        batches[0].iter().map(|frame| frame.0).collect::<Vec<_>>(),
        vec![b'E']
    );
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some(state));
    assert_eq!(batches[1][0].0, b'Z');
}

pub fn parameters(label: &str, sql: &str, input: &[u8]) -> Vec<fixture::Frames> {
    use crate::support_parameter_occurrence_wire::{run_script, Step};
    let mut steps = Vec::new();
    for format in [0, 1] {
        steps.push(Step::ParseFlush(wire::parse_frame("p", sql)));
        steps.push(Step::ThroughReady(vec![
            wire::describe_statement_frame("p"),
            wire::sync_frame(),
        ]));
        steps.push(Step::BindFlush(wire::bind_frame_with_formats(
            "out",
            "p",
            &[],
            &[Some(input)],
            &[format],
        )));
        steps.push(Step::ThroughReady(vec![
            wire::describe_portal_frame("out"),
            wire::execute_frame("out"),
            wire::sync_frame(),
        ]));
        steps.push(Step::ThroughReady(vec![
            wire::close_statement_frame("p"),
            wire::sync_frame(),
        ]));
    }
    run_script(label, steps)
}
pub fn assert_parameter(batches: &[fixture::Frames], oid: i32) {
    for batch in batches.as_chunks::<5>().0 {
        let description = batch[1]
            .iter()
            .find(|frame| frame.0 == b't')
            .expect("parameter description");
        assert_eq!(wire::parse_parameter_description(&description.1), vec![oid]);
    }
}
