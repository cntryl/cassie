//! Native typed VALUES row metadata and exact payload checks.
use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;

pub fn assert_result(
    batches: &[fixture::Frames],
    expected: (&str, i32, i16, i32),
    rows: [&[&[u8]]; 2],
) {
    assert!(matches!(batches.len(), 10 | 20));
    for (cycle, batch) in batches.as_chunks::<5>().0.iter().enumerate() {
        let format = cycle % 2;
        assert_eq!(batch[0][0].0, b'1', "{:?}", batch[0]);
        fixture::assert_success(&batch[1]);
        assert_eq!(batch[2][0].0, b'2', "{:?}", batch[2]);
        fixture::assert_success(&batch[3]);
        for (phase, output_format) in [(1, 0), (3, i16::try_from(format).expect("format"))] {
            let description = batch[phase]
                .iter()
                .find(|frame| frame.0 == b'T')
                .expect("description");
            let columns = wire::parse_row_description(&description.1);
            assert_eq!(columns.len(), 1);
            let column = &columns[0];
            assert_eq!(
                (
                    column.name.as_str(),
                    column.type_oid,
                    column.type_size,
                    column.type_mod
                ),
                expected
            );
            assert_eq!(column.format_code, output_format);
        }
        let actual = fixture::data_row_payloads(&batch[3]);
        assert_eq!(actual.len(), rows[format].len());
        for (actual, expected) in actual.iter().zip(rows[format]) {
            assert_eq!(actual, expected);
        }
        fixture::assert_success(&batch[4]);
    }
}

pub fn parameters(
    label: &str,
    sql: &str,
    oid: i32,
    text: &[u8],
    binary: &[u8],
) -> Vec<fixture::Frames> {
    use crate::support_parameter_occurrence_wire::{run_script, Step};
    let mut steps = Vec::new();
    for (parameter_format, input) in [(0, text), (1, binary)] {
        for output_format in [0, 1] {
            steps.push(Step::ParseFlush(wire::parse_frame_with_types(
                "p",
                sql,
                &[oid],
            )));
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
        assert_eq!(batch[0][0].0, b'1', "{:?}", batch[0]);
        fixture::assert_success(&batch[1]);
        let descriptor = batch[1]
            .iter()
            .find(|frame| frame.0 == b't')
            .expect("parameters");
        assert_eq!(wire::parse_parameter_description(&descriptor.1), vec![oid]);
    }
}
