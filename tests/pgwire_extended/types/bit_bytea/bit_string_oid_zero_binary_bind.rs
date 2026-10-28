use super::*;

#[test]
fn should_infer_bit_string_oids_for_oid_zero_binary_parameters() {
    // Arrange
    let statement = "bit_string_oid_zero_binary_parameters";
    let portal = "bit_string_oid_zero_binary_parameters_portal";
    let sql = "SELECT $1::bit(5) AS fixed_bits, $2::varbit AS variable_bits";
    let fixed_bits = [0, 0, 0, 5, 0b1010_1000];
    let variable_bits = [0, 0, 0, 7, 0b0110_1100];
    let batches = run_script(
        "issue-816-bit-string-oid-zero-binary-parameters",
        vec![
            Step::ParseFlush(wire::parse_frame_with_types(statement, sql, &[0, 0])),
            Step::ThroughReady(vec![
                wire::describe_statement_frame(statement),
                wire::sync_frame(),
            ]),
            Step::ThroughReady(vec![
                wire::bind_frame_with_formats(
                    portal,
                    statement,
                    &[1, 1],
                    &[Some(&fixed_bits), Some(&variable_bits)],
                    &[1],
                ),
                wire::describe_portal_frame(portal),
                wire::execute_frame(portal),
                wire::sync_frame(),
            ]),
        ],
    );

    // Act
    let parsed = &batches[0];
    let described = &batches[1];
    let result = &batches[2];

    // Assert
    assert_eq!(wire::error_code(parsed), None);
    assert_eq!(wire::error_code(described), None);
    let parameters = described
        .iter()
        .find(|(tag, _)| *tag == b't')
        .expect("ParameterDescription");
    assert_eq!(
        wire::parse_parameter_description(&parameters.1),
        vec![1560, 1562]
    );
    assert_eq!(
        row_description(described)
            .iter()
            .map(|field| (field.type_oid, field.type_mod))
            .collect::<Vec<_>>(),
        vec![(1560, 5), (1562, -1)]
    );
    assert_eq!(wire::error_code(result), None);
    assert_eq!(
        row_description(result)
            .iter()
            .map(|field| (field.type_oid, field.format_code))
            .collect::<Vec<_>>(),
        vec![(1560, 1), (1562, 1)]
    );
    assert_eq!(
        data_row_bytes(result),
        vec![vec![fixed_bits.to_vec(), variable_bits.to_vec()]]
    );
}
