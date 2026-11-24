#[test]
fn should_parse_delimited_text_array_null_tokens() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT string_to_array('a,NULL,c', ',', 'NULL')",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("type-string-to-array-null-token", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (1009, -1, -1), 0, true);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("{a,NULL,c}".to_owned())]]
    );
}
