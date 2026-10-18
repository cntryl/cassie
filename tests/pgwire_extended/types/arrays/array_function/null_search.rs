#[test]
fn should_find_null_element_subscripts_for_custom_bounds() {
    // Arrange
    let input = b"[0:4]={7,NULL,7,NULL,7}";
    let cycle = fixture::execute_cycle(
        "SELECT array_position($1::INT[], NULL), array_positions($1::INT[], NULL)",
        Some((1_007, 0, Some(input))),
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-807-array-null-search-positions", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("array null position RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| (column.type_oid, column.type_size, column.type_mod))
            .collect::<Vec<_>>(),
        vec![(23, 4, -1), (1_007, -1, -1)]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("1".to_owned()), Some("{1,3}".to_owned())]]
    );
}

#[test]
fn should_return_null_for_absent_array_position() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT array_position(ARRAY[7,NULL,9]::INT[], 99)",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-807-array-position-absent", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (23, 4, -1), 0, true);
    assert_eq!(wire::data_rows(frames), vec![vec![None]]);
}
