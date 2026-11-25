use super::*;

#[test]
fn should_encode_computed_bit_string_operations_in_binary_results() {
    // Arrange
    let statement = "computed_bit_string_binary_operations";
    let portal = "computed_bit_string_binary_operations_portal";
    let sql = "SELECT B'10101' & B'00111' AS bit_and, \
                      B'10101' | B'00111' AS bit_or, \
                      B'10101' # B'00111' AS bit_xor, \
                      B'1001' || B'011' AS concatenated, \
                      ~B'101' AS complemented, \
                      get_bit(B'101011', 5) AS selected_bit, \
                      bit_count(B'101011') AS set_bits";
    let batches = run_script(
        "issue-816-computed-bit-string-binary-results",
        vec![
            Step::ParseFlush(wire::parse_frame(statement, sql)),
            Step::ThroughReady(vec![
                wire::bind_frame_with_formats(portal, statement, &[], &[], &[1]),
                wire::describe_portal_frame(portal),
                wire::execute_frame(portal),
                wire::sync_frame(),
            ]),
        ],
    );

    // Act
    let result = &batches[1];
    let fields = row_description(result);

    // Assert
    assert_eq!(wire::error_code(result), None);
    assert_eq!(
        fields
            .iter()
            .map(|field| (field.type_oid, field.format_code))
            .collect::<Vec<_>>(),
        vec![
            (1560, 1),
            (1560, 1),
            (1560, 1),
            (1562, 1),
            (1560, 1),
            (23, 1),
            (20, 1),
        ]
    );
    assert_eq!(
        data_row_bytes(result),
        vec![vec![
            vec![0, 0, 0, 5, 0b0010_1000],
            vec![0, 0, 0, 5, 0b1011_1000],
            vec![0, 0, 0, 5, 0b1001_0000],
            vec![0, 0, 0, 7, 0b1001_0110],
            vec![0, 0, 0, 3, 0b0100_0000],
            vec![0, 0, 0, 1],
            vec![0, 0, 0, 0, 0, 0, 0, 4],
        ]]
    );
}
