#[test]
fn should_apply_array_functions_to_null_elements() {
    // Arrange
    let input = b"{7,NULL,9}";
    let sql = "SELECT array_append($1::INT[], 11)::TEXT, array_remove($1::INT[], NULL)::TEXT, array_replace($1::INT[], 7, 8)::TEXT, array_positions($1::INT[], NULL)::TEXT";
    let cycle = fixture::execute_cycle(sql, Some((1007, 0, Some(input))), 0, true);

    // Act
    let batches = fixture::run_cycles("type-array-null-elements", &[], vec![cycle]);

    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![
            Some("{7,NULL,9,11}".to_owned()),
            Some("{7,9}".to_owned()),
            Some("{8,NULL,9}".to_owned()),
            Some("{2}".to_owned()),
        ]]
    );
}

#[test]
fn should_join_array_values_using_explicit_null_replacement() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT array_to_string($1::INT[], ',', 'NULL')",
        Some((1007, 0, Some(&b"{1,NULL,3}"[..]))),
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("type-array-to-string-null-replacement", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (25, -1, -1), 0, true);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("1,NULL,3".to_owned())]]
    );
}

#[test]
fn should_prepend_array_elements_to_empty_null_inputs() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT array_prepend(0, ARRAY[1,2]), array_prepend(NULL::INT, ARRAY[1,2]), array_prepend(5, NULL::INT[]), array_prepend(NULL::INT, NULL::INT[])",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("type-array-prepend-null-identity", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("array_prepend RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![1007; 4]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![
            Some("{0,1,2}".to_owned()),
            Some("{NULL,1,2}".to_owned()),
            Some("{5}".to_owned()),
            Some("{NULL}".to_owned()),
        ]]
    );
}
