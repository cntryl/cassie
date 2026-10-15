//! Wire assertions for explicitly selected catalog/admin policies.

use crate::support_pgwire as wire;
use crate::support_wire_queries as fixture;

pub fn run(label: &str, statements: &[&str]) -> Vec<fixture::Frames> {
    fixture::run_cycles(
        label,
        &[],
        statements
            .iter()
            .map(|sql| vec![wire::simple_query_frame(sql)])
            .collect(),
    )
}

pub fn assert_nulls(frames: &fixture::Frames, oids: &[i32]) {
    fixture::assert_success(frames);
    let (_, body) = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("typed NULL RowDescription");
    assert_eq!(
        wire::parse_row_description(body)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        oids
    );
    assert_eq!(wire::data_rows(frames), vec![vec![None; oids.len()]]);
}

pub fn assert_registered(frames: &fixture::Frames) {
    fixture::assert_success(frames);
    assert_eq!(wire::data_rows(frames), vec![vec![Some("t".to_owned())]]);
}

pub fn assert_error(frames: &fixture::Frames, sqlstate: &str) {
    assert_eq!(wire::error_code(frames).as_deref(), Some(sqlstate));
    assert_eq!(wire::data_rows(frames), Vec::<Vec<Option<String>>>::new());
    assert_eq!(frames.iter().filter(|(tag, _)| *tag == b'C').count(), 0);
    assert_eq!(
        frames.last().map(|(_, body)| body.as_slice()),
        Some(&b"I"[..])
    );
}
