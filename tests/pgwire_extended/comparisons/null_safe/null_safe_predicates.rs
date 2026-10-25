//! Literal BOOLEAN expectations over the selected existing ARRAY input packets.

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;
use crate::support_type_codec_matrix::ARRAY_CASES;
use crate::support_type_metadata_contract as fixture;

#[test]
fn should_preserve_null_safe_array_boolean_wire_identity() {
    // Arrange
    let mut cycles = Vec::new();
    let mut expected = Vec::new();
    for case in &ARRAY_CASES {
        let inputs = [
            (0, Some(case.input_text)),
            (1, Some(case.input_binary)),
            (0, None),
            (0, Some(&b"{}"[..])),
            (1, case.empty_binary),
        ];
        for (input_format, body) in inputs {
            for result_format in [0, 1] {
                for (sql, value, label) in [
                    ("SELECT $1 IS NOT DISTINCT FROM $1 AS same", true, "same"),
                    (
                        "SELECT $1 IS DISTINCT FROM NULL AS different",
                        body.is_some(),
                        "different",
                    ),
                ] {
                    cycles.push(fixture::execute_cycle(
                        sql,
                        Some((case.oid, input_format, body)),
                        result_format,
                        true,
                    ));
                    expected.push((case.name, case.oid, result_format, value, label));
                }
            }
        }
    }
    // Act
    let results = fixture::run_cycles("null-safe-array-wire", &[], cycles);
    // Assert
    assert_eq!(results.len(), 280);
    for (frames, (name, oid, format, value, label)) in results.iter().zip(expected) {
        fixture::assert_success(frames);
        fixture::assert_single_column_descriptors(frames, (16, 1, -1), format, true);
        let parameter_descriptions = frames
            .iter()
            .filter(|(tag, _)| *tag == b't')
            .map(|(_, payload)| wire::parse_parameter_description(payload))
            .collect::<Vec<_>>();
        assert_eq!(parameter_descriptions, vec![vec![oid]], "{name}");
        for (_, payload) in frames.iter().filter(|(tag, _)| *tag == b'T') {
            assert_eq!(wire::parse_row_description(payload)[0].name, label);
        }
        let body: &[u8] = if format == 0 {
            if value {
                b"t"
            } else {
                b"f"
            }
        } else if value {
            &[1]
        } else {
            &[0]
        };
        assert_eq!(
            fixture::data_row_payloads(frames),
            vec![fixture::one_field_row(body)],
            "{name}/{format}"
        );
    }
}

#[test]
fn should_infer_int4_parameter_type_for_null_safe_comparison_over_pgwire() {
    // Arrange
    let query = "SELECT $1 IS NOT DISTINCT FROM value AS same \
                 FROM issue_798_null_safe_values WHERE id = 1";
    let batches = run_script(
        "issue-798-int4-null-safe-parameter-inference",
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame(
                "CREATE TABLE issue_798_null_safe_values (id INT4, value INT4)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "INSERT INTO issue_798_null_safe_values VALUES (1, 7), (2, NULL)",
            )]),
            Step::ParseFlush(wire::parse_frame_with_types("null_safe", query, &[0])),
            Step::ThroughReady(vec![
                wire::describe_statement_frame("null_safe"),
                wire::sync_frame(),
            ]),
        ],
    );

    // Act
    let parsed = &batches[2];
    let described = &batches[3];

    // Assert
    for (name, frames) in [
        ("CREATE TABLE", &batches[0]),
        ("INSERT", &batches[1]),
        ("Parse", parsed),
        ("Describe", described),
    ] {
        let errors = frames
            .iter()
            .filter(|frame| frame.0 == b'E')
            .map(|frame| wire::parse_error_fields(&frame.1))
            .collect::<Vec<_>>();
        assert!(
            errors.is_empty(),
            "{name} returned pgwire errors: {errors:?}"
        );
    }
    let parameter_description = described
        .iter()
        .find(|frame| frame.0 == b't')
        .expect("INT4 ParameterDescription");
    assert_eq!(
        wire::parse_parameter_description(&parameter_description.1),
        vec![23]
    );
    let result_description = described
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("Boolean RowDescription");
    assert_eq!(
        wire::parse_row_description(&result_description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![16]
    );
}
