//! PostgreSQL 18.6 occurrence-order observations: Parse admission and retained metadata.

use super::support_parameter_occurrence_wire::{run_script, Step};
use super::support_pgwire as wire;
use super::support_type_metadata_contract as fixture;

include!("../type_resolution/parameter_occurrences/errors.rs");

struct PairCase {
    sql: &'static str,
    input: &'static [u8],
    parameter_oid: i32,
    columns: [(i32, i16); 2],
    text: [&'static [u8]; 2],
    binary: [&'static [u8]; 2],
}

include!("../grammar/parameter_occurrences/semantics.rs");

include!("parameter_occurrences/parameters.rs");

include!("../protocol/parameter_occurrences/errors.rs");

include!("../type_resolution/parameter_occurrences/parameters.rs");

include!("../expressions/conditional/parameter_occurrences/errors.rs");

include!("../grammar/parameter_occurrences/errors.rs");

include!("../grammar/parameter_occurrences/parameters.rs");

include!("parameter_occurrences/errors.rs");

include!("../type_resolution/parameter_occurrences/resources.rs");

include!("../ordering/parameter_occurrences/errors.rs");

fn assert_rebinding_error(frames: &fixture::Frames, state: &str) {
    assert_eq!(wire::error_code(frames).as_deref(), Some(state));
    assert_eq!(frames.iter().filter(|(tag, _)| *tag == b'E').count(), 1);
    assert!(!frames
        .iter()
        .any(|(tag, _)| matches!(*tag, b'2' | b'D' | b'C')));
    assert_descriptors(frames, &[(25, -1), (16, 1)], &["body", "flag"], 0, false);
    assert_ready(frames);
}

fn tags(frames: &fixture::Frames) -> Vec<u8> {
    frames.iter().map(|(tag, _)| *tag).collect()
}

fn assert_ready(frames: &fixture::Frames) {
    assert_eq!(
        frames.last().map(|(tag, body)| (*tag, body.as_slice())),
        Some((b'Z', &b"I"[..]))
    );
}

fn assert_error(frames: &fixture::Frames, state: &str, message: &str, detail: Option<&str>) {
    assert_eq!(frames.iter().filter(|(tag, _)| *tag == b'E').count(), 1);
    let (_, body) = frames
        .iter()
        .find(|(tag, _)| *tag == b'E')
        .expect("ErrorResponse");
    let fields = wire::parse_error_fields(body);
    let field = |code| {
        fields
            .iter()
            .find(|(kind, _)| *kind == code)
            .map(|(_, text)| text.as_str())
    };
    assert_eq!(field('S'), Some("ERROR"));
    assert_eq!(field('C'), Some(state));
    assert_eq!(field('M'), Some(message));
    assert_eq!(field('D'), detail);
    assert_eq!(field('H'), None);
}

fn assert_parameter_oids(frames: &fixture::Frames, expected: &[i32]) {
    let descriptions = frames
        .iter()
        .filter(|(tag, _)| *tag == b't')
        .collect::<Vec<_>>();
    assert_eq!(descriptions.len(), 1);
    assert_eq!(
        wire::parse_parameter_description(&descriptions[0].1),
        expected
    );
}

fn assert_descriptors(
    frames: &fixture::Frames,
    expected: &[(i32, i16)],
    names: &[&str],
    format: i16,
    has_portal: bool,
) {
    let descriptions = frames
        .iter()
        .filter(|(tag, _)| *tag == b'T')
        .map(|(_, body)| wire::parse_row_description(body))
        .collect::<Vec<_>>();
    assert_eq!(descriptions.len(), if has_portal { 2 } else { 1 });
    assert_eq!(expected.len(), names.len());
    for (index, columns) in descriptions.into_iter().enumerate() {
        assert_eq!(columns.len(), expected.len());
        for ((column, &(oid, size)), name) in columns.into_iter().zip(expected).zip(names) {
            assert_eq!(column.name, *name);
            assert_eq!((column.table_oid, column.attr_num), (0, 0));
            assert_eq!(
                (column.type_oid, column.type_size, column.type_mod),
                (oid, size, -1)
            );
            assert_eq!(column.format_code, if index == 0 { 0 } else { format });
        }
    }
}

fn row_payload(fields: &[&[u8]]) -> Vec<u8> {
    let mut payload = i16::try_from(fields.len())
        .expect("fixture field count")
        .to_be_bytes()
        .to_vec();
    for field in fields {
        payload.extend_from_slice(
            &i32::try_from(field.len())
                .expect("fixture field width")
                .to_be_bytes(),
        );
        payload.extend_from_slice(field);
    }
    payload
}

fn assert_command(frames: &fixture::Frames, expected: &[u8]) {
    let completions = frames
        .iter()
        .filter(|(tag, _)| *tag == b'C')
        .collect::<Vec<_>>();
    assert_eq!(completions.len(), 1);
    assert_eq!(completions[0].1, expected);
}
