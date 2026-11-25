#[test]
fn should_report_two_dimensions_for_nested_arrays() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT ARRAY_NDIMS($1::INT[]) AS dimensions",
        Some((1_007, 0, Some(b"{{1,2},{3,4}}"))),
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-807-array-ndims-nested", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (23, 4, -1), 0, true);
    assert_eq!(wire::data_rows(frames), vec![vec![Some("2".to_owned())]]);
}

#[test]
fn should_report_one_dimension_for_nondefault_array_bounds() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT ARRAY_NDIMS($1::INT[]) AS dimensions",
        Some((1_007, 0, Some(b"[0:2]={7,NULL,9}"))),
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-807-array-ndims-nondefault-bounds", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (23, 4, -1), 0, true);
    assert_eq!(wire::data_rows(frames), vec![vec![Some("1".to_owned())]]);
}

#[test]
fn should_concatenate_arrays_while_preserving_postgresql_bounds() {
    // Arrange
    let cycle = vec![
        wire::parse_frame_with_types(
            "",
            "SELECT ARRAY_CAT($1::INT[], $2::INT[])::TEXT",
            &[1_007, 1_007],
        ),
        wire::bind_frame_with_formats(
            "",
            "",
            &[0, 0],
            &[Some(b"[0:1]={1,2}"), Some(b"[4:5]={3,4}")],
            &[],
        ),
        wire::execute_frame(""),
        wire::sync_frame(),
    ];

    // Act
    let batches = fixture::run_cycles("issue-807-array-cat-bounds", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("[0:3]={1,2,3,4}".to_owned())]]
    );
}
