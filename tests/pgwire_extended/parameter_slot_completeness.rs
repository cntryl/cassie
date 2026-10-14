//! PostgreSQL Parse validates and retains every declared parameter slot.

use super::support_parameter_occurrence_wire::{Step, run_script};
use super::support_pgwire as wire;

#[test]
fn should_reject_the_lowest_unresolved_parameter_slot_during_parse() {
    // Arrange
    let cases = [
        ("leading_zero", "SELECT $2 IS NULL AS value", vec![0, 16], 1),
        (
            "leading_unknown",
            "SELECT $2 IS NULL AS value",
            vec![705, 16],
            1,
        ),
        (
            "middle_zero",
            "SELECT $3 IS NULL AS value",
            vec![23, 0, 16],
            2,
        ),
        (
            "reference_beyond_type_vector",
            "SELECT $2 IS NULL AS value",
            vec![16],
            2,
        ),
        (
            "trailing_zero",
            "SELECT $1 IS NULL AS value",
            vec![16, 0],
            2,
        ),
        ("unused_zero", "SELECT TRUE AS value", vec![0], 1),
    ];
    let mut steps = Vec::new();
    for (name, sql, oids, _) in &cases {
        steps.push(Step::ParseFlush(wire::parse_frame_with_types(
            name, sql, oids,
        )));
        steps.push(Step::ThroughReady(vec![wire::sync_frame()]));
    }

    // Act
    let batches = run_script("parameter-slot-unresolved", steps);

    // Assert
    assert_eq!(batches.len(), cases.len() * 2);
    for ((_, _, _, parameter), pair) in cases.iter().zip(batches.as_chunks::<2>().0) {
        assert_error(
            &pair[0],
            "42P18",
            &format!("could not determine data type of parameter ${parameter}"),
        );
        assert_eq!(
            pair[0].iter().map(|(tag, _)| *tag).collect::<Vec<_>>(),
            vec![b'E']
        );
        assert_ready(&pair[1]);
    }
}

#[test]
fn should_reject_unused_slots_for_non_select_parse_statements() {
    // Arrange
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(
            "CREATE TABLE parameter_slot_legacy (id INTEGER)",
        )]),
        Step::ParseFlush(wire::parse_frame_with_types(
            "legacy_untyped_extra",
            "INSERT INTO parameter_slot_legacy (id) VALUES ($1)",
            &[23, 0],
        )),
        Step::ThroughReady(vec![wire::sync_frame()]),
    ];

    // Act
    let batches = run_script("parameter-slot-legacy-statement", steps);

    // Assert
    assert_eq!(
        batches[0].iter().map(|(tag, _)| *tag).collect::<Vec<_>>(),
        vec![b'C', b'Z']
    );
    assert_error(
        &batches[1],
        "42P18",
        "could not determine data type of parameter $2",
    );
    assert_ready(&batches[2]);
}

#[test]
fn should_infer_call_parameters_from_the_registered_procedure_signature() {
    // Arrange
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(
            r#"CREATE PROCEDURE parameter_slot_signature (title TEXT) AS "SELECT 1""#,
        )]),
        Step::ParseFlush(wire::parse_frame_with_types(
            "typed_procedure_call",
            "CALL parameter_slot_signature($1)",
            &[0],
        )),
        Step::ThroughReady(vec![
            wire::describe_statement_frame("typed_procedure_call"),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("parameter-slot-procedure-signature", steps);

    // Assert
    assert_eq!(
        batches[0].iter().map(|(tag, _)| *tag).collect::<Vec<_>>(),
        vec![b'C', b'Z']
    );
    assert_eq!(
        batches[1].iter().map(|(tag, _)| *tag).collect::<Vec<_>>(),
        vec![b'1']
    );
    assert_eq!(
        batches[2].iter().map(|(tag, _)| *tag).collect::<Vec<_>>(),
        vec![b't', b'n', b'Z']
    );
    let (_, description) = batches[2]
        .iter()
        .find(|(tag, _)| *tag == b't')
        .expect("ParameterDescription");
    assert_eq!(wire::parse_parameter_description(description), vec![25]);
    assert_ready(&batches[2]);
}

#[test]
fn should_retain_unused_parameter_types_through_describe_bind() {
    // Arrange
    let steps = vec![
        Step::ParseFlush(wire::parse_frame_with_types(
            "typed_extra_slot",
            "SELECT $1 IS NULL AS value",
            &[16, 23],
        )),
        Step::ThroughReady(vec![
            wire::describe_statement_frame("typed_extra_slot"),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "typed_extra_good",
                "typed_extra_slot",
                &[0],
                &[Some(b"t"), Some(b"42")],
                &[0],
            ),
            wire::execute_frame("typed_extra_good"),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "typed_extra_missing",
                "typed_extra_slot",
                &[0],
                &[Some(b"t")],
                &[0],
            ),
            wire::execute_frame("typed_extra_missing"),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "typed_extra_invalid",
                "typed_extra_slot",
                &[0],
                &[Some(b"t"), Some(b"bad")],
                &[0],
            ),
            wire::execute_frame("typed_extra_invalid"),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "typed_extra_out_of_range",
                "typed_extra_slot",
                &[0],
                &[Some(b"t"), Some(b"2147483648")],
                &[0],
            ),
            wire::execute_frame("typed_extra_out_of_range"),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("parameter-slot-typed-extra", steps);

    // Assert
    assert_eq!(
        batches[0].iter().map(|(tag, _)| *tag).collect::<Vec<_>>(),
        vec![b'1']
    );
    assert_eq!(
        batches[1].iter().map(|(tag, _)| *tag).collect::<Vec<_>>(),
        vec![b't', b'T', b'Z']
    );
    let (_, parameter_description) = batches[1]
        .iter()
        .find(|(tag, _)| *tag == b't')
        .expect("ParameterDescription");
    assert_eq!(
        wire::parse_parameter_description(parameter_description),
        vec![16, 23]
    );
    assert_eq!(
        batches[2].iter().map(|(tag, _)| *tag).collect::<Vec<_>>(),
        vec![b'2', b'D', b'C', b'Z']
    );
    assert_ready(&batches[2]);
    assert_error(
        &batches[3],
        "08P01",
        "bind message supplies 1 parameters, but prepared statement \"typed_extra_slot\" requires 2",
    );
    assert_ready(&batches[3]);
    assert_error(
        &batches[4],
        "22P02",
        "invalid input syntax for type integer: \"bad\"",
    );
    assert_ready(&batches[4]);
    assert_error(
        &batches[5],
        "22003",
        "value \"2147483648\" is out of range for type integer",
    );
    assert_ready(&batches[5]);
}

#[test]
fn should_preserve_parameter_slot_oids_across_inference() {
    // Arrange
    let cases = [
        ("zero_comparison", "SELECT $1 = 1 AS value", 0_i32, 23_i32),
        (
            "unknown_comparison",
            "SELECT $1 = 1 AS value",
            705_i32,
            23_i32,
        ),
        (
            "zero_cast",
            "SELECT $1::INT4 IS NULL AS value",
            0_i32,
            23_i32,
        ),
        (
            "unknown_cast",
            "SELECT $1::INT4 IS NULL AS value",
            705_i32,
            23_i32,
        ),
    ];
    let mut steps = Vec::new();
    for (name, sql, oid, _) in cases {
        steps.push(Step::ParseFlush(wire::parse_frame_with_types(
            name,
            sql,
            &[oid],
        )));
        steps.push(Step::ThroughReady(vec![
            wire::describe_statement_frame(name),
            wire::sync_frame(),
        ]));
    }

    // Act
    let batches = run_script("parameter-slot-inference", steps);

    // Assert
    assert_eq!(batches.len(), cases.len() * 2);
    for (index, (_, _, _, expected_oid)) in cases.iter().enumerate() {
        let parsed = &batches[index * 2];
        assert_eq!(
            parsed.iter().map(|(tag, _)| *tag).collect::<Vec<_>>(),
            vec![b'1']
        );
        assert_parameter_description(&batches[index * 2 + 1], &[*expected_oid]);
    }
}

#[test]
fn should_preserve_high_bit_parameter_slot_oids() {
    // Arrange
    let high_bit_cases = [0x8000_0000_u32, u32::MAX];
    let mut steps = Vec::new();
    for (index, oid) in high_bit_cases.iter().copied().enumerate() {
        let statement = format!("unused_high_bit_{index}");
        let signed_wire_bits = i32::from_be_bytes(oid.to_be_bytes());
        steps.push(Step::ParseFlush(wire::parse_frame_with_types(
            &statement,
            "SELECT TRUE AS value",
            &[signed_wire_bits],
        )));
        steps.push(Step::ThroughReady(vec![
            wire::describe_statement_frame(&statement),
            wire::sync_frame(),
        ]));
        let portal = format!("unused_high_bit_portal_{index}");
        steps.push(Step::ThroughReady(vec![
            wire::bind_frame_with_formats(&portal, &statement, &[0], &[None], &[0]),
            wire::execute_frame(&portal),
            wire::sync_frame(),
        ]));
    }

    // Act
    let batches = run_script("parameter-slot-high-bit", steps);

    // Assert
    assert_eq!(batches.len(), high_bit_cases.len() * 3);
    for (index, oid) in high_bit_cases.iter().copied().enumerate() {
        let parse = &batches[index * 3];
        assert_eq!(
            parse.iter().map(|(tag, _)| *tag).collect::<Vec<_>>(),
            vec![b'1']
        );
        let described = &batches[index * 3 + 1];
        assert_eq!(
            described.iter().map(|(tag, _)| *tag).collect::<Vec<_>>(),
            vec![b't', b'T', b'Z']
        );
        let (_, payload) = described
            .iter()
            .find(|(tag, _)| *tag == b't')
            .expect("ParameterDescription");
        assert_eq!(
            wire::parse_parameter_description(payload),
            vec![i32::from_be_bytes(oid.to_be_bytes())]
        );
        let bind = &batches[index * 3 + 2];
        assert_eq!(
            bind.iter().map(|(tag, _)| *tag).collect::<Vec<_>>(),
            vec![b'E', b'Z']
        );
        assert_error(
            bind,
            "XX000",
            &format!("cache lookup failed for type {oid}"),
        );
        assert_ready(bind);
    }
}

fn assert_parameter_description(frames: &[(u8, Vec<u8>)], expected_oids: &[i32]) {
    assert_eq!(
        frames.iter().map(|(tag, _)| *tag).collect::<Vec<_>>(),
        vec![b't', b'T', b'Z']
    );
    let (_, payload) = frames
        .iter()
        .find(|(tag, _)| *tag == b't')
        .expect("ParameterDescription");
    assert_eq!(wire::parse_parameter_description(payload), expected_oids);
    assert_ready(frames);
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
