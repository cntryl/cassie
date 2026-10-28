//! PostgreSQL 18 empty varbit binary framing tracked by #816.

use super::*;

#[test]
fn should_round_trip_zero_length_varbit_binary_framing() {
    // Arrange
    let sql = "SELECT bit_length($1::varbit) AS bit_count, $1::varbit AS bits";
    let statement = "empty_varbit_binary";
    let portal = "empty_varbit_binary_portal";
    let empty_varbit_binary = [0, 0, 0, 0];
    let steps = vec![
        Step::ParseFlush(wire::parse_frame_with_types(statement, sql, &[1562])),
        Step::ThroughReady(vec![
            wire::describe_statement_frame(statement),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                portal,
                statement,
                &[1],
                &[Some(&empty_varbit_binary)],
                &[0, 1],
            ),
            wire::describe_portal_frame(portal),
            wire::execute_frame(portal),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("issue-816-empty-varbit-binary-round-trip", steps);

    // Assert
    assert_eq!(batches.len(), 3);
    assert_eq!(wire::error_code(&batches[0]), None);

    let description = &batches[1];
    assert_eq!(wire::error_code(description), None);
    let parameters = description
        .iter()
        .find(|(tag, _)| *tag == b't')
        .expect("varbit ParameterDescription");
    assert_eq!(wire::parse_parameter_description(&parameters.1), vec![1562]);

    let result = &batches[2];
    assert_eq!(wire::error_code(result), None);
    assert_eq!(
        row_description(result)
            .iter()
            .map(|column| (
                column.type_oid,
                column.type_size,
                column.type_mod,
                column.format_code
            ))
            .collect::<Vec<_>>(),
        vec![(23, 4, -1, 0), (1562, -1, -1, 1)]
    );
    assert_eq!(
        data_row_bytes(result),
        vec![vec![vec![b'0'], empty_varbit_binary.to_vec()]]
    );
}
