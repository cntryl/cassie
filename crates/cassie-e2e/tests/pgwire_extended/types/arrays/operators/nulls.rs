#[test]
fn should_concatenate_arrays_with_postgresql_null_empty_semantics() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT array_cat(ARRAY[1,2], ARRAY[]::INT[]), array_cat(NULL::INT[], ARRAY[3]), array_cat(ARRAY[1], NULL::INT[]), array_cat(NULL::INT[], NULL::INT[])",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("type-array-cat-null-empty", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("array_cat RowDescription");
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
            Some("{1,2}".to_owned()),
            Some("{3}".to_owned()),
            Some("{1}".to_owned()),
            None,
        ]]
    );
}
