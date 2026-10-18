#[test]
fn should_infer_integer_array_constructor_type_for_null_elements() {
    // Arrange
    let cycle = fixture::execute_cycle("SELECT ARRAY[NULL, 1] AS values_array", None, 0, true);

    // Act
    let batches = fixture::run_cycles("type-array-constructor-null-element", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (1007, -1, -1), 0, true);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("{NULL,1}".to_owned())]]
    );
}

#[test]
fn should_insert_standard_oid_array_parameter_through_pgwire() {
    // Arrange
    let setup = [(
        "CREATE TABLE issue_807_array_standard_keys (id INT, items INT[])",
        Vec::new(),
    )];
    let input = b"{7,NULL,9}";
    let insert = fixture::execute_cycle(
        "INSERT INTO issue_807_array_standard_keys VALUES (1, $1)",
        Some((1007, 0, Some(input))),
        0,
        false,
    );

    // Act
    let batches = fixture::run_cycles("type-array-standard-oid-dml", &setup, vec![insert]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_eq!(
        frames
            .iter()
            .find(|(tag, _)| *tag == b'C')
            .map(|(_, body)| body.as_slice()),
        Some(&b"INSERT 0 1\0"[..])
    );
}
