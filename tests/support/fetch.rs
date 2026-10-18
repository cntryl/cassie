//! Exact native pagination rows and inferred parameter identities.
use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;

pub fn assert_result(batches: &[fixture::Frames], expected: [&[&[u8]]; 2]) {
    assert!(matches!(batches.len(), 10 | 20));
    for (index, batch) in batches.as_chunks::<5>().0.iter().enumerate() {
        assert_eq!(batch[0][0].0, b'1', "{:?}", batch[0]);
        fixture::assert_success(&batch[1]);
        assert_eq!(batch[2][0].0, b'2');
        fixture::assert_success(&batch[3]);
        let descriptor = batch[3]
            .iter()
            .find(|frame| frame.0 == b'T')
            .expect("descriptor");
        let fields = wire::parse_row_description(&descriptor.1);
        assert_eq!(fields.len(), 1);
        assert_eq!((fields[0].type_oid, fields[0].type_mod), (23, -1));
        assert_eq!(
            fields[0].format_code,
            i16::try_from(index % 2).expect("format")
        );
        let rows = fixture::data_row_payloads(&batch[3]);
        assert_eq!(rows.len(), expected[index % 2].len());
        for (actual, expected) in rows.iter().zip(expected[index % 2]) {
            assert_eq!(actual, expected);
        }
        fixture::assert_success(&batch[4]);
    }
}

pub fn parameters(label: &str, sql: &str, text: &[u8], binary: &[u8]) -> Vec<fixture::Frames> {
    let mut steps = Vec::new();
    for (parameter_format, input) in [(0, text), (1, binary)] {
        for output_format in [0, 1] {
            steps.push(Step::ParseFlush(wire::parse_frame("p", sql)));
            steps.push(Step::ThroughReady(vec![
                wire::describe_statement_frame("p"),
                wire::sync_frame(),
            ]));
            steps.push(Step::BindFlush(wire::bind_frame_with_formats(
                "out",
                "p",
                &[parameter_format],
                &[Some(input)],
                &[output_format],
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
    }
    run_script(label, steps)
}

pub fn assert_parameter(batches: &[fixture::Frames], oid: i32) {
    assert_eq!(batches.len(), 20);
    for batch in batches.as_chunks::<5>().0 {
        let descriptor = batch[1]
            .iter()
            .find(|frame| frame.0 == b't')
            .expect("parameters");
        assert_eq!(wire::parse_parameter_description(&descriptor.1), vec![oid]);
    }
}
