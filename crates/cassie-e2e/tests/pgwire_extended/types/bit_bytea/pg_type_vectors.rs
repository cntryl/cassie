//! PostgreSQL built-in vector wire identities shared by #817 and #904.

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

use super::{data_row_bytes, row_description};

#[test]
fn should_round_trip_non_byte_aligned_varbit_binary() {
    // Arrange
    let sql = "SELECT B'00101'::varbit AS bits";
    let statement = "varbit_binary_round_trip";
    let portal = "varbit_binary_round_trip_portal";
    let wire_value = [0, 0, 0, 5, 0b0010_1000];
    let steps = vec![
        Step::ParseFlush(wire::parse_frame(statement, sql)),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(portal, statement, &[], &[], &[1]),
            wire::describe_portal_frame(portal),
            wire::execute_frame(portal),
            wire::sync_frame(),
        ]),
        Step::ParseFlush(wire::parse_frame_with_types(
            "varbit_binary_parameter",
            "SELECT $1::varbit AS bits",
            &[1562],
        )),
        Step::ThroughReady(vec![
            wire::describe_statement_frame("varbit_binary_parameter"),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "varbit_binary_parameter_portal",
                "varbit_binary_parameter",
                &[1],
                &[Some(&wire_value)],
                &[],
            ),
            wire::describe_portal_frame("varbit_binary_parameter_portal"),
            wire::execute_frame("varbit_binary_parameter_portal"),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("issue-816-varbit-binary-round-trip", steps);

    // Assert
    let binary_output = &batches[1];
    assert_eq!(wire::error_code(binary_output), None);
    assert_eq!(
        row_description(binary_output)
            .iter()
            .map(|column| (
                column.type_oid,
                column.type_size,
                column.type_mod,
                column.format_code
            ))
            .collect::<Vec<_>>(),
        vec![(1562, -1, -1, 1)]
    );
    assert_eq!(
        data_row_bytes(binary_output),
        vec![vec![wire_value.to_vec()]]
    );

    let statement_description = &batches[3];
    assert_eq!(wire::error_code(statement_description), None);
    assert_eq!(
        wire::parse_parameter_description(
            &statement_description
                .iter()
                .find(|(tag, _)| *tag == b't')
                .expect("varbit ParameterDescription")
                .1,
        ),
        vec![1562]
    );
    assert_eq!(
        row_description(statement_description)
            .iter()
            .map(|column| (column.type_oid, column.type_mod))
            .collect::<Vec<_>>(),
        vec![(1562, -1)]
    );

    let binary_input = &batches[4];
    assert_eq!(wire::error_code(binary_input), None);
    assert_eq!(
        wire::data_rows(binary_input),
        vec![vec![Some("00101".to_owned())]]
    );
}
