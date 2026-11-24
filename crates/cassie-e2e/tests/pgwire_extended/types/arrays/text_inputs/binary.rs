#[test]
fn should_infer_standard_oids_for_array_binary_binds() {
    // Arrange
    let int4_input = [
        0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 23, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 4, 0, 0, 0, 42,
    ];
    let int8_input = [
        0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 20, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0,
        0, 42,
    ];
    let text_input = [
        0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 25, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 3, b'A', b'd', b'a',
    ];
    let cases = [
        (
            "SELECT CAST($1 AS INT4[]) AS value",
            1_007,
            int4_input.as_slice(),
        ),
        (
            "SELECT CAST($1 AS INT8[]) AS value",
            1_016,
            int8_input.as_slice(),
        ),
        (
            "SELECT CAST($1 AS TEXT[]) AS value",
            1_009,
            text_input.as_slice(),
        ),
    ];
    let cycles = cases
        .iter()
        .map(|(sql, _, input)| fixture::execute_cycle(sql, Some((0, 1, Some(input))), 1, true))
        .collect();

    // Act
    let batches = fixture::run_cycles("type-array-oids-binary-bind", &[], cycles);

    // Assert
    assert_eq!(batches.len(), cases.len());
    for ((_, expected_oid, input), frames) in cases.iter().zip(&batches) {
        let parameters = frames
            .iter()
            .find(|(tag, _)| *tag == b't')
            .expect("array ParameterDescription");
        assert_eq!(
            wire::parse_parameter_description(&parameters.1),
            vec![*expected_oid]
        );
        fixture::assert_success(frames);
        fixture::assert_single_column_descriptors(frames, (*expected_oid, -1, -1), 1, true);
        assert_eq!(
            fixture::data_row_payloads(frames),
            vec![fixture::one_field_row(input)]
        );
    }
}

#[test]
fn should_send_integer_arrays_with_postgresql_oid_binary_format() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_807_array_binary_output (items INT[])",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_807_array_binary_output VALUES ($1)",
            vec![Value::Json(serde_json::json!([7, null, 9]))],
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT items FROM issue_807_array_binary_output",
        None,
        1,
        true,
    );

    // Act
    let batches = fixture::run_cycles("type-array-binary-output", &setup, vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (1007, -1, -1), 1, true);
    assert_eq!(
        fixture::data_row_payloads(frames),
        vec![fixture::one_field_row(
            b"\0\0\0\x01\0\0\0\x01\0\0\0\x17\0\0\0\x03\0\0\0\x01\0\0\0\x04\0\0\0\x07\xff\xff\xff\xff\0\0\0\x04\0\0\0\x09"
        )]
    );
}
