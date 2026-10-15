#[test]
fn should_follow_array_nulls_setting_when_reading_array_text() {
    // Arrange
    let cycles = [
        "SELECT '{NULL}'::TEXT[]",
        "SET array_nulls = off; SELECT '{NULL}'::TEXT[]",
        "SET array_nulls = on; SELECT '{NULL}'::TEXT[]",
    ]
    .into_iter()
    .map(|sql| vec![wire::simple_query_frame(sql)])
    .collect();

    // Act
    let batches = fixture::run_cycles("issue-807-array-nulls-setting", &[], cycles);

    // Assert
    for frames in &batches {
        fixture::assert_success(frames);
        let description = frames
            .iter()
            .find(|(tag, _)| *tag == b'T')
            .expect("array_nulls result RowDescription");
        assert_eq!(
            wire::parse_row_description(&description.1)[0].type_oid,
            1_009
        );
    }
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![Some("{NULL}".to_owned())]]
    );
    assert_eq!(
        wire::data_rows(&batches[1]),
        vec![vec![Some("{\"NULL\"}".to_owned())]]
    );
    assert_eq!(
        wire::data_rows(&batches[2]),
        vec![vec![Some("{NULL}".to_owned())]]
    );
}

#[test]
fn should_apply_any_all_empty_array_truth_tables() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT 4 = ANY ($1), 4 = ALL ($1)",
        Some((1007, 0, Some(b"{}"))),
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("type-array-any-all-empty", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("quantified comparison RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![16, 16]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("f".to_owned()), Some("t".to_owned())]]
    );
}

#[test]
fn should_preserve_null_shape_results_for_empty_or_null_arrays() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT array_lower('{}'::INT[], 1), array_upper('{}'::INT[], 1), array_ndims('{}'::INT[]), array_lower(NULL::INT[], 1), array_upper(NULL::INT[], 1), array_ndims(NULL::INT[])",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("type-array-empty-null-shape-functions", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("empty and null array shape RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| (column.type_oid, column.type_size))
            .collect::<Vec<_>>(),
        vec![(23, 4); 6]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![None, None, None, None, None, None]]
    );
}
