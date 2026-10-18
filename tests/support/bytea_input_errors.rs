//! BYTEA input errors at Parse and Bind, followed by Sync and successful reuse.
use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;

pub fn parse(label: &str, sql: &str) -> Vec<fixture::Frames> {
    run_script(
        label,
        vec![
            Step::ParseFlush(wire::parse_frame("bad", sql)),
            Step::ThroughReady(vec![wire::sync_frame()]),
            Step::ThroughReady(vec![wire::simple_query_frame("SELECT 7::INT")]),
        ],
    )
}

pub fn assert_parse_result(batches: &[fixture::Frames], state: &str, message: &str) {
    assert_error(&batches[0], state, message);
    fixture::assert_success(&batches[1]);
    fixture::assert_success(&batches[2]);
    assert_eq!(wire::data_rows(&batches[2]), vec![vec![Some("7".into())]]);
}

pub fn parameter(label: &str, input: &[u8], oid: i32) -> Vec<fixture::Frames> {
    let mut steps = Vec::new();
    for format in [0, 1] {
        steps.extend([
            Step::ParseFlush(wire::parse_frame_with_types(
                "p",
                "SELECT $1::BYTEA AS output",
                &[oid],
            )),
            Step::ThroughReady(vec![
                wire::describe_statement_frame("p"),
                wire::sync_frame(),
            ]),
            Step::BindFlush(wire::bind_frame_with_formats(
                "bad",
                "p",
                &[0],
                &[Some(input)],
                &[format],
            )),
            Step::ThroughReady(vec![wire::sync_frame()]),
            Step::ThroughReady(vec![
                wire::describe_statement_frame("p"),
                wire::sync_frame(),
            ]),
            Step::BindFlush(wire::bind_frame_with_formats(
                "good",
                "p",
                &[0],
                &[Some(b"\\x00")],
                &[format],
            )),
            Step::ThroughReady(vec![
                wire::describe_portal_frame("good"),
                wire::execute_frame("good"),
                wire::sync_frame(),
            ]),
            Step::ThroughReady(vec![wire::close_statement_frame("p"), wire::sync_frame()]),
        ]);
    }
    run_script(label, steps)
}

pub fn assert_parameter_result(batches: &[fixture::Frames], oid: i32, state: &str, message: &str) {
    assert_eq!(batches.len(), 16);
    for (format, cycle) in batches.as_chunks::<8>().0.iter().enumerate() {
        assert_eq!(cycle[0][0].0, b'1');
        fixture::assert_success(&cycle[1]);
        assert_error(&cycle[2], state, message);
        fixture::assert_success(&cycle[3]);
        fixture::assert_success(&cycle[4]);
        let descriptor = cycle[4]
            .iter()
            .find(|frame| frame.0 == b't')
            .expect("retained parameters");
        assert_eq!(wire::parse_parameter_description(&descriptor.1), vec![oid]);
        assert_eq!(cycle[5][0].0, b'2', "{:?}", cycle[5]);
        fixture::assert_success(&cycle[6]);
        let expected = if format == 0 {
            b"\0\x01\0\0\0\x04\\x00".as_slice()
        } else {
            b"\0\x01\0\0\0\x01\0".as_slice()
        };
        assert_eq!(
            fixture::data_row_payloads(&cycle[6]),
            vec![expected.to_vec()]
        );
        fixture::assert_success(&cycle[7]);
    }
}

fn assert_error(frames: &fixture::Frames, state: &str, message: &str) {
    assert_eq!(
        frames.iter().map(|frame| frame.0).collect::<Vec<_>>(),
        vec![b'E']
    );
    assert_eq!(wire::error_code(frames).as_deref(), Some(state));
    let fields = wire::parse_error_fields(&frames[0].1);
    assert!(
        fields
            .iter()
            .any(|(field, value)| *field == 'M' && value == message),
        "{fields:?}"
    );
}
