//! Binary parameters and RETURNING descriptors through a data-modifying CTE (#834).

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

type Frames = Vec<(u8, Vec<u8>)>;

fn expected_binary_row(id: i32, value: i64) -> Vec<u8> {
    let mut row = Vec::new();
    row.extend_from_slice(&2_i16.to_be_bytes());
    row.extend_from_slice(&4_i32.to_be_bytes());
    row.extend_from_slice(&id.to_be_bytes());
    row.extend_from_slice(&8_i32.to_be_bytes());
    row.extend_from_slice(&value.to_be_bytes());
    row
}

fn assert_success(frames: &[(u8, Vec<u8>)], context: &str) {
    assert_eq!(
        wire::error_code(frames),
        None,
        "{context} ErrorResponse: {:?}",
        frames
            .iter()
            .filter(|frame| frame.0 == b'E')
            .map(|frame| wire::parse_error_fields(&frame.1))
            .collect::<Vec<_>>()
    );
}

fn assert_modifying_cte_binary_contract(batches: &[Frames], id: i32, value: i64) {
    let setup = &batches[0];
    let control_parse = &batches[1];
    let control_description = &batches[2];
    let control_execution = &batches[3];
    let target_parse = &batches[4];
    let target_description = &batches[5];
    let target_execution = &batches[6];
    let persisted = &batches[7];

    for (context, frames) in [
        ("table setup", setup),
        ("binary parameter control Parse", control_parse),
        ("binary parameter control Describe", control_description),
        ("binary parameter control Execute", control_execution),
        ("modifying CTE Parse", target_parse),
        ("modifying CTE Describe", target_description),
        ("modifying CTE Execute", target_execution),
        ("persisted RETURNING row", persisted),
    ] {
        assert_success(frames, context);
    }
    assert_eq!(
        control_parse
            .iter()
            .map(|frame| frame.0)
            .collect::<Vec<_>>(),
        [b'1']
    );
    assert_eq!(
        wire::parse_parameter_description(
            &control_description
                .iter()
                .find(|frame| frame.0 == b't')
                .expect("control ParameterDescription")
                .1
        ),
        [23, 20]
    );
    assert_eq!(
        wire::parse_parameter_description(
            &target_description
                .iter()
                .find(|frame| frame.0 == b't')
                .expect("modifying CTE ParameterDescription")
                .1
        ),
        [23, 20]
    );
    for (context, frames) in [
        ("control", control_description),
        ("modifying CTE", target_description),
    ] {
        let description = frames
            .iter()
            .find(|frame| frame.0 == b'T')
            .expect("statement RowDescription");
        assert_eq!(
            wire::parse_row_description(&description.1)
                .iter()
                .map(|column| column.type_oid)
                .collect::<Vec<_>>(),
            [23, 20],
            "{context} result OIDs"
        );
    }
    let binary_description = target_execution
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("binary modifying CTE portal RowDescription");
    let columns = wire::parse_row_description(&binary_description.1);
    assert_eq!(
        columns
            .iter()
            .map(|column| (column.type_oid, column.format_code))
            .collect::<Vec<_>>(),
        [(23, 1), (20, 1)]
    );
    assert_eq!(
        target_execution
            .iter()
            .find(|frame| frame.0 == b'D')
            .expect("binary modifying CTE DataRow")
            .1,
        expected_binary_row(id, value)
    );
    assert_eq!(
        wire::data_rows(persisted),
        vec![vec![Some(id.to_string()), Some(value.to_string())]]
    );
}

#[test]
fn should_preserve_returning_identity_given_binary_parameters_when_a_modifying_cte_executes() {
    // Arrange
    let id = 7_i32;
    let value = 9_007_199_254_740_993_i64;
    let id_bytes = id.to_be_bytes();
    let value_bytes = value.to_be_bytes();
    let control_statement = "binary_parameter_control";
    let target_statement = "modifying_cte_insert";
    // Act
    let batches = run_script(
        "modifying-cte-binary-returning",
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame(
                "CREATE TABLE modifying_cte_binary_target (id INT4 PRIMARY KEY, value INT8)",
            )]),
            Step::ParseFlush(wire::parse_frame_with_types(
                control_statement,
                "SELECT $1::INT4 AS id, $2::INT8 AS value",
                &[0, 0],
            )),
            Step::ThroughReady(vec![
                wire::describe_statement_frame(control_statement),
                wire::sync_frame(),
            ]),
            Step::ThroughReady(vec![
                wire::bind_frame_with_formats(
                    "binary_control_portal",
                    control_statement,
                    &[1],
                    &[Some(&id_bytes), Some(&value_bytes)],
                    &[1],
                ),
                wire::describe_portal_frame("binary_control_portal"),
                wire::execute_frame("binary_control_portal"),
                wire::sync_frame(),
            ]),
            Step::ParseFlush(wire::parse_frame_with_types(
                target_statement,
                "WITH inserted AS ( \
                    INSERT INTO modifying_cte_binary_target (id, value) \
                    VALUES ($1::INT4, $2::INT8) RETURNING id, value \
                 ) SELECT id, value FROM inserted",
                &[0, 0],
            )),
            Step::ThroughReady(vec![
                wire::describe_statement_frame(target_statement),
                wire::sync_frame(),
            ]),
            Step::ThroughReady(vec![
                wire::bind_frame_with_formats(
                    "modifying_cte_portal",
                    target_statement,
                    &[1],
                    &[Some(&id_bytes), Some(&value_bytes)],
                    &[1],
                ),
                wire::describe_portal_frame("modifying_cte_portal"),
                wire::execute_frame("modifying_cte_portal"),
                wire::sync_frame(),
            ]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT id, value FROM modifying_cte_binary_target ORDER BY id",
            )]),
        ],
    );

    // Assert
    assert_modifying_cte_binary_contract(&batches, id, value);
}
