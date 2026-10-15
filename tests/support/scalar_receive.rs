//! Independent wire assertions for current fixed-scalar receive errors.
use crate::support_pgwire as wire;
use crate::support_type_codec_matrix::CodecCase;
use crate::support_type_metadata_contract as fixture;

#[derive(Clone, Copy)]
pub enum Outcome {
    Error(&'static str),
    Value,
    Null,
}

pub struct Expected {
    pub case: &'static CodecCase,
    pub format: i16,
    pub outcome: Outcome,
}

pub fn invalid_temporal_cases() -> [(i32, Vec<u8>); 4] {
    [
        (1082, (-2_147_483_647_i32).to_be_bytes().to_vec()),
        (1083, (-1_i64).to_be_bytes().to_vec()),
        (1083, 86_400_000_001_i64.to_be_bytes().to_vec()),
        (
            1114,
            (-9_223_372_036_854_775_807_i64).to_be_bytes().to_vec(),
        ),
    ]
}

pub fn one_element_array(oid: i32, body: &[u8]) -> Vec<u8> {
    let mut result = Vec::new();
    for header in [
        1_i32,
        0,
        oid,
        1,
        1,
        i32::try_from(body.len()).expect("fixture length"),
    ] {
        result.extend_from_slice(&header.to_be_bytes());
    }
    result.extend_from_slice(body);
    result
}

pub fn named_cycle(
    statement: &str,
    initialize: Option<(&str, i32)>,
    body: Option<&[u8]>,
    format: i16,
) -> fixture::Cycle {
    let mut cycle = initialize.map_or_else(Vec::new, |(sql, oid)| {
        vec![wire::parse_frame_with_types(statement, sql, &[oid])]
    });
    cycle.extend([
        wire::describe_statement_frame(statement),
        wire::bind_frame_with_formats("", statement, &[1], &[body], &[format]),
        wire::describe_portal_frame(""),
        wire::execute_frame(""),
        wire::sync_frame(),
    ]);
    cycle
}

pub fn assert_error(frames: &fixture::Frames, state: &str, ready: u8) {
    assert_eq!(wire::error_code(frames).as_deref(), Some(state));
    assert_eq!(frames.iter().filter(|(tag, _)| *tag == b'E').count(), 1);
    assert!(!frames
        .iter()
        .any(|(tag, _)| matches!(*tag, b'2' | b'D' | b'C')));
    assert_eq!(
        frames.last().map(|frame| frame.1.as_slice()),
        Some(&[ready][..])
    );
}

pub fn assert_command(frames: &fixture::Frames, command: &[u8], ready: u8) {
    assert_eq!(wire::error_code(frames), None);
    let completion = frames
        .iter()
        .find(|(tag, _)| *tag == b'C')
        .expect("command");
    assert_eq!(completion.1, command);
    assert_eq!(
        frames.last().map(|frame| frame.1.as_slice()),
        Some(&[ready][..])
    );
}

pub fn assert_value(
    frames: &fixture::Frames,
    case: &CodecCase,
    format: i16,
    ready: u8,
    command: &[u8],
) {
    assert_command(frames, command, ready);
    fixture::assert_single_column_descriptors(
        frames,
        (case.oid, case.typlen, case.typmod),
        format,
        true,
    );
    let field = if format == 0 {
        case.output_text
    } else {
        case.output_binary
    };
    assert_eq!(
        fixture::data_row_payloads(frames),
        vec![fixture::one_field_row(field)]
    );
}

pub fn assert_batches(batches: &[fixture::Frames], expected: &[Expected]) {
    assert_eq!(batches.len(), expected.len());
    for (frames, expected) in batches.iter().zip(expected) {
        let case = expected.case;
        let parameters = frames
            .iter()
            .find(|(tag, _)| *tag == b't')
            .expect("parameter metadata");
        assert_eq!(
            wire::parse_parameter_description(&parameters.1),
            vec![case.oid]
        );
        match expected.outcome {
            Outcome::Error(state) => {
                assert_error(frames, state, b'I');
                let columns = frames
                    .iter()
                    .filter(|(tag, _)| *tag == b'T')
                    .collect::<Vec<_>>();
                assert_eq!(columns.len(), 1);
                let columns = wire::parse_row_description(&columns[0].1);
                assert_eq!(columns.len(), 1);
                assert_eq!(
                    (
                        columns[0].type_oid,
                        columns[0].type_size,
                        columns[0].type_mod
                    ),
                    (case.oid, case.typlen, case.typmod)
                );
                assert_eq!(columns[0].format_code, 0);
            }
            Outcome::Value => assert_value(frames, case, expected.format, b'I', b"SELECT 1\0"),
            Outcome::Null => {
                assert_command(frames, b"SELECT 1\0", b'I');
                fixture::assert_single_column_descriptors(
                    frames,
                    (case.oid, case.typlen, case.typmod),
                    expected.format,
                    true,
                );
                assert_eq!(
                    fixture::data_row_payloads(frames),
                    vec![vec![0, 1, 255, 255, 255, 255]]
                );
            }
        }
    }
}
