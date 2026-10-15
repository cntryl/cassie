//! PostgreSQL built-in vector wire identities shared by #817 and #904.

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

use super::{data_row_bytes, row_description};

#[test]
fn should_send_builtin_vector_binary_frames() {
    // Arrange
    let sql = "SELECT ''::int2vector AS empty_int2, '1 2 3'::int2vector AS int2_values, \
                      ''::oidvector AS empty_oid, '4 5'::oidvector AS oid_values";
    let statement = "builtin_vector_binary_output";
    let portal = "builtin_vector_binary_output_portal";
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(sql)]),
        Step::ParseFlush(wire::parse_frame(statement, sql)),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(portal, statement, &[], &[], &[1]),
            wire::describe_portal_frame(portal),
            wire::execute_frame(portal),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("issue-817-builtin-vector-binary-send", steps);

    // Assert
    let text_result = &batches[0];
    assert_eq!(wire::error_code(text_result), None);
    assert_eq!(
        row_description(text_result)
            .iter()
            .map(|column| (column.type_oid, column.type_size, column.type_mod))
            .collect::<Vec<_>>(),
        vec![(22, -1, -1), (22, -1, -1), (30, -1, -1), (30, -1, -1)]
    );
    assert_eq!(
        wire::data_rows(text_result),
        vec![vec![
            Some(String::new()),
            Some("1 2 3".to_owned()),
            Some(String::new()),
            Some("4 5".to_owned()),
        ]]
    );

    let binary_result = &batches[2];
    assert_eq!(wire::error_code(binary_result), None);
    assert_eq!(
        row_description(binary_result)
            .iter()
            .map(|column| (
                column.type_oid,
                column.type_size,
                column.type_mod,
                column.format_code
            ))
            .collect::<Vec<_>>(),
        vec![
            (22, -1, -1, 1),
            (22, -1, -1, 1),
            (30, -1, -1, 1),
            (30, -1, -1, 1)
        ]
    );
    assert_eq!(
        data_row_bytes(binary_result),
        vec![
            vec![
                0, 0, 0, 1, // one dimension
                0, 0, 0, 0, // no NULL elements
                0, 0, 0, 21, // int2 element OID
                0, 0, 0, 0, // empty length
                0, 0, 0, 0, // zero-based lower bound
            ],
            vec![
                0, 0, 0, 1, // one dimension
                0, 0, 0, 0, // no NULL elements
                0, 0, 0, 21, // int2 element OID
                0, 0, 0, 3, // three elements
                0, 0, 0, 0, // zero-based lower bound
                0, 0, 0, 2, 0, 1, // int2 value 1
                0, 0, 0, 2, 0, 2, // int2 value 2
                0, 0, 0, 2, 0, 3, // int2 value 3
            ],
            vec![
                0, 0, 0, 1, // one dimension
                0, 0, 0, 0, // no NULL elements
                0, 0, 0, 26, // oid element OID
                0, 0, 0, 0, // empty length
                0, 0, 0, 0, // zero-based lower bound
            ],
            vec![
                0, 0, 0, 1, // one dimension
                0, 0, 0, 0, // no NULL elements
                0, 0, 0, 26, // oid element OID
                0, 0, 0, 2, // two elements
                0, 0, 0, 0, // zero-based lower bound
                0, 0, 0, 4, 0, 0, 0, 1, // OID value 1
                0, 0, 0, 4, 0, 0, 0, 2, // OID value 2
            ],
        ]
    );
}

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
    assert_eq!(data_row_bytes(binary_output), vec![wire_value.to_vec()]);

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
