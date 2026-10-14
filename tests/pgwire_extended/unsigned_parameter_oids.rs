//! PostgreSQL OID wire identities use all 32 network bits.

use super::support_parameter_occurrence_wire::{Step, run_script};
use super::support_pgwire as wire;

#[test]
fn should_echo_unknown_unsigned_high_bit_oids_from_parse() {
    // Arrange
    let cases = [0x8000_0000_u32, u32::MAX];
    let mut steps = Vec::new();
    for (index, oid) in cases.into_iter().enumerate() {
        let name = format!("unknown_unsigned_oid_{index}");
        let signed_wire_bits = i32::from_be_bytes(oid.to_be_bytes());
        steps.push(Step::ParseFlush(wire::parse_frame_with_types(
            &name,
            "SELECT $1 IS NULL AS value",
            &[signed_wire_bits],
        )));
        steps.push(Step::ThroughReady(vec![
            wire::describe_statement_frame(&name),
            wire::sync_frame(),
        ]));
    }

    // Act
    let batches = run_script("unknown-unsigned-parameter-oids", steps);

    // Assert
    assert_eq!(batches.len(), cases.len() * 2);
    for (oid, pair) in cases.into_iter().zip(batches.as_chunks::<2>().0) {
        assert_eq!(
            pair[0].iter().map(|(tag, _)| *tag).collect::<Vec<_>>(),
            vec![b'1']
        );
        assert_eq!(
            pair[1].iter().map(|(tag, _)| *tag).collect::<Vec<_>>(),
            vec![b't', b'T', b'Z']
        );
        let (_, parameter_description) = pair[1]
            .iter()
            .find(|(tag, _)| *tag == b't')
            .expect("ParameterDescription");
        let decoded = wire::parse_parameter_description(parameter_description)[0];
        assert_eq!(decoded.to_be_bytes(), oid.to_be_bytes());
        let (_, row_description) = pair[1]
            .iter()
            .find(|(tag, _)| *tag == b'T')
            .expect("RowDescription");
        assert_eq!(wire::parse_row_description(row_description)[0].type_oid, 16);
        assert_eq!(
            pair[1].last().map(|(tag, body)| (*tag, body.as_slice())),
            Some((b'Z', &b"I"[..]))
        );
    }
}

#[test]
fn should_match_postgres_unknown_high_bit_oid_error_at_result_description() {
    // Arrange
    let cases = [0x8000_0000_u32, u32::MAX];
    let mut steps = Vec::new();
    for (index, oid) in cases.into_iter().enumerate() {
        let name = format!("unknown_result_oid_{index}");
        let signed_wire_bits = i32::from_be_bytes(oid.to_be_bytes());
        steps.push(Step::ParseFlush(wire::parse_frame_with_types(
            &name,
            "SELECT $1 AS value",
            &[signed_wire_bits],
        )));
        steps.push(Step::ThroughReady(vec![
            wire::describe_statement_frame(&name),
            wire::sync_frame(),
        ]));
        steps.push(Step::ParseFlush(wire::parse_frame_with_types(
            &name,
            "SELECT TRUE AS value",
            &[],
        )));
        steps.push(Step::ThroughReady(vec![
            wire::describe_statement_frame(&name),
            wire::sync_frame(),
        ]));
    }

    // Act
    let batches = run_script("unknown-unsigned-result-oids", steps);

    // Assert
    assert_eq!(batches.len(), cases.len() * 4);
    for (oid, group) in cases.into_iter().zip(batches.as_chunks::<4>().0) {
        let pair = &group[..2];
        assert_error(
            &pair[0],
            "XX000",
            &format!("cache lookup failed for type {oid}"),
        );
        assert_eq!(
            pair[0].iter().map(|(tag, _)| *tag).collect::<Vec<_>>(),
            vec![b'E']
        );
        assert_eq!(
            pair[1].iter().map(|(tag, _)| *tag).collect::<Vec<_>>(),
            vec![b'Z']
        );
        assert_ready(&pair[1]);
        assert_eq!(
            group[2].iter().map(|(tag, _)| *tag).collect::<Vec<_>>(),
            vec![b'1']
        );
        assert_eq!(
            group[3].iter().map(|(tag, _)| *tag).collect::<Vec<_>>(),
            vec![b't', b'T', b'Z']
        );
        assert_ready(&group[3]);
    }
}

#[test]
fn should_match_postgres_unknown_high_bit_oid_error_when_binding() {
    // Arrange
    let cases = [0x8000_0000_u32, u32::MAX];
    let mut steps = Vec::new();
    for (index, oid) in cases.into_iter().enumerate() {
        for (label, format, value) in [("text", 0_i16, None), ("binary", 1_i16, None)] {
            let name = format!("unknown_bind_oid_{index}_{label}");
            let portal = format!("unknown_bind_portal_{index}_{label}");
            let signed_wire_bits = i32::from_be_bytes(oid.to_be_bytes());
            steps.push(Step::ParseFlush(wire::parse_frame_with_types(
                &name,
                "SELECT $1 IS NULL AS value",
                &[signed_wire_bits],
            )));
            steps.push(Step::ThroughReady(vec![
                wire::describe_statement_frame(&name),
                wire::bind_frame_with_formats(&portal, &name, &[format], &[value], &[0]),
                wire::execute_frame(&portal),
                wire::sync_frame(),
            ]));
        }
    }

    // Act
    let batches = run_script("unknown-unsigned-bind-oids", steps);

    // Assert
    assert_eq!(batches.len(), cases.len() * 4);
    for (oid, pairs) in cases.into_iter().zip(batches.as_chunks::<4>().0) {
        for pair in pairs.as_chunks::<2>().0 {
            assert_eq!(
                pair[0].iter().map(|(tag, _)| *tag).collect::<Vec<_>>(),
                vec![b'1']
            );
            assert_eq!(
                pair[1].iter().map(|(tag, _)| *tag).collect::<Vec<_>>(),
                vec![b't', b'T', b'E', b'Z']
            );
            assert_error(
                &pair[1],
                "XX000",
                &format!("cache lookup failed for type {oid}"),
            );
            assert_ready(&pair[1]);
        }
    }
}

fn assert_error(frames: &[(u8, Vec<u8>)], expected_state: &str, expected_message: &str) {
    let fields = frames
        .iter()
        .find(|(tag, _)| *tag == b'E')
        .map(|(_, payload)| wire::parse_error_fields(payload))
        .expect("ErrorResponse");
    let field = |code| {
        fields
            .iter()
            .find(|(kind, _)| *kind == code)
            .map(|(_, value)| value.as_str())
    };
    assert_eq!(field('C'), Some(expected_state));
    assert_eq!(field('M'), Some(expected_message));
}

fn assert_ready(frames: &[(u8, Vec<u8>)]) {
    assert_eq!(
        frames.last().map(|(tag, body)| (*tag, body.as_slice())),
        Some((b'Z', &b"I"[..]))
    );
}
