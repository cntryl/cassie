
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
