//! Shared wire observations for the `json[b]_array_elements` / `json[b]_each` FROM sources.

use super::{fixture, wire};
use cassie::types::Value;

pub(super) type Rows = Vec<Vec<Option<String>>>;

/// Runs one extended-query cycle over a fresh in-memory fixture.
pub(super) fn query(
    label: &str,
    setup: &[(&str, Vec<Value>)],
    sql: &str,
    described: bool,
) -> fixture::Frames {
    let cycle = fixture::execute_cycle(sql, None, 0, described);
    fixture::run_cycles(label, setup, vec![cycle]).remove(0)
}

pub(super) fn row(values: &[Option<&str>]) -> Vec<Option<String>> {
    values
        .iter()
        .map(|value| value.map(ToOwned::to_owned))
        .collect()
}

pub(super) fn text_rows(values: &[&str]) -> Rows {
    values.iter().map(|value| row(&[Some(*value)])).collect()
}

fn last_description(frames: &fixture::Frames) -> Vec<wire::RowDescription> {
    let description = frames
        .iter()
        .rfind(|(tag, _)| *tag == b'T')
        .expect("RowDescription");
    wire::parse_row_description(&description.1)
}

pub(super) fn column_oids(frames: &fixture::Frames) -> Vec<i32> {
    last_description(frames)
        .iter()
        .map(|column| column.type_oid)
        .collect()
}

pub(super) fn column_names(frames: &fixture::Frames) -> Vec<String> {
    last_description(frames)
        .into_iter()
        .map(|column| column.name)
        .collect()
}

pub(super) fn error_message(frames: &fixture::Frames) -> Option<String> {
    frames
        .iter()
        .find(|(tag, _)| *tag == b'E')
        .and_then(|(_, payload)| {
            wire::parse_error_fields(payload)
                .into_iter()
                .find(|(kind, _)| *kind == 'M')
                .map(|(_, message)| message)
        })
}

pub(super) fn docs_setup() -> Vec<(&'static str, Vec<Value>)> {
    vec![
        (
            "CREATE TABLE json_elements_docs (id INT, doc JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO json_elements_docs VALUES (1, '[1, 2]'), (2, '[]'), (3, NULL)",
            Vec::new(),
        ),
    ]
}
