//! Plan-time error phases and recovery through real frontend frames.
use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;

pub fn failures(label: &str, cases: &[(&str, &str)]) -> Vec<fixture::Frames> {
    let mut steps = Vec::new();
    for result_format in [None, Some(0), Some(1)] {
        for (sql, _) in cases {
            steps.push(Step::ParseFlush(wire::parse_frame_with_types(
                "bad",
                sql,
                &[23, 20],
            )));
            steps.push(Step::ThroughReady(vec![
                wire::describe_statement_frame("bad"),
                wire::sync_frame(),
            ]));
            let formats = result_format.into_iter().collect::<Vec<_>>();
            steps.push(Step::BindFlush(wire::bind_frame_with_formats(
                "failed",
                "bad",
                &[],
                &[Some(b"1"), Some(b"2")],
                &formats,
            )));
            steps.push(Step::ThroughReady(vec![
                wire::describe_portal_frame("failed"),
                wire::execute_frame("failed"),
                wire::sync_frame(),
            ]));
            steps.push(Step::ThroughReady(vec![
                wire::describe_statement_frame("bad"),
                wire::sync_frame(),
            ]));
            steps.push(Step::ThroughReady(vec![
                wire::close_statement_frame("bad"),
                wire::sync_frame(),
            ]));
            steps.push(Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT 7::INT",
            )]));
        }
    }
    run_script(label, steps)
}

pub fn assert_failures(batches: &[fixture::Frames], cases: &[(&str, &str)]) {
    assert_eq!(batches.len(), cases.len() * 3 * 7);
    for (i, batch) in batches.as_chunks::<7>().0.iter().enumerate() {
        let (sql, state) = cases[i % cases.len()];
        assert_eq!(
            batch[0].iter().map(|f| f.0).collect::<Vec<_>>(),
            vec![b'1'],
            "{sql}"
        );
        fixture::assert_success(&batch[1]);
        assert!(batch[1].iter().any(|f| f.0 == b't'), "{sql}");
        assert!(batch[1].iter().any(|f| f.0 == b'T'), "{sql}");
        assert_eq!(wire::error_code(&batch[2]).as_deref(), Some(state), "{sql}");
        assert_eq!(
            batch[2].iter().map(|f| f.0).collect::<Vec<_>>(),
            vec![b'E'],
            "{sql}"
        );
        assert_eq!(
            batch[3].iter().map(|f| f.0).collect::<Vec<_>>(),
            vec![b'Z'],
            "{sql}"
        );
        fixture::assert_success(&batch[4]);
        assert!(batch[4].iter().any(|f| f.0 == b'T'), "{sql}");
        assert_eq!(
            batch[5].iter().map(|f| f.0).collect::<Vec<_>>(),
            vec![b'3', b'Z']
        );
        fixture::assert_success(&batch[6]);
        assert_eq!(wire::data_rows(&batch[6]), vec![vec![Some("7".into())]]);
    }
}

pub fn pruned(label: &str, sql: &str) -> Vec<fixture::Frames> {
    pruned_with_setup(label, sql, &[])
}

pub fn pruned_with_setup(label: &str, sql: &str, setup: &[&str]) -> Vec<fixture::Frames> {
    let mut steps = setup
        .iter()
        .map(|sql| Step::ThroughReady(vec![wire::simple_query_frame(sql)]))
        .collect::<Vec<_>>();
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
            &[],
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
    let mut batches = run_script(label, steps);
    for batch in batches.drain(..setup.len()) {
        fixture::assert_success(&batch);
    }
    batches
}

pub fn assert_pruned(batches: &[fixture::Frames], oid: i32, text: &str) {
    assert_eq!(batches.len(), 10);
    for (i, batch) in batches.as_chunks::<5>().0.iter().enumerate() {
        assert_eq!(
            batch[0][0].0,
            b'1',
            "Parse errors: {:?}",
            batch[0]
                .iter()
                .filter(|frame| frame.0 == b'E')
                .map(|frame| wire::parse_error_fields(&frame.1))
                .collect::<Vec<_>>()
        );
        fixture::assert_success(&batch[1]);
        assert_eq!(batch[2].iter().map(|f| f.0).collect::<Vec<_>>(), vec![b'2']);
        fixture::assert_success(&batch[3]);
        let description = batch[3]
            .iter()
            .find(|f| f.0 == b'T')
            .expect("portal descriptor");
        let column = &wire::parse_row_description(&description.1)[0];
        assert_eq!(column.type_oid, oid);
        assert_eq!(column.format_code, i16::try_from(i).expect("result format"));
        let value = if i == 0 {
            text.as_bytes().to_vec()
        } else if oid == 16 {
            vec![u8::from(text == "t")]
        } else {
            text.parse::<i32>()
                .expect("int4 result")
                .to_be_bytes()
                .to_vec()
        };
        assert_eq!(
            fixture::data_row_payloads(&batch[3]),
            vec![fixture::one_field_row(&value)]
        );
        fixture::assert_success(&batch[4]);
    }
}
