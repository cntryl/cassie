#[test]
fn should_report_array_dimensions_null_search_positions() {
    // Arrange
    let input = b"{7,NULL,9}";
    let sql = "SELECT array_dims($1::INT[]) AS dimensions, array_lower($1::INT[], 1) AS lower_bound, array_upper($1::INT[], 1) AS upper_bound, array_length($1::INT[], 1) AS length, cardinality($1::INT[]) AS cardinality, array_position($1::INT[], NULL) AS null_position";
    let cycle = fixture::execute_cycle(sql, Some((1007, 0, Some(input))), 0, true);

    // Act
    let batches = fixture::run_cycles("type-array-dimensions-and-functions", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("array dimensions RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| (column.type_oid, column.type_size))
            .collect::<Vec<_>>(),
        vec![(25, -1), (23, 4), (23, 4), (23, 4), (23, 4), (23, 4)]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![
            Some("[1:3]".to_owned()),
            Some("1".to_owned()),
            Some("3".to_owned()),
            Some("3".to_owned()),
            Some("3".to_owned()),
            Some("2".to_owned()),
        ]]
    );
}

#[test]
fn should_compare_array_elements_then_dimensions_nulls_bounds() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT ARRAY[1,2] < ARRAY[1,2,0], ARRAY[1,3] < ARRAY[2,1], ARRAY[[1,2],[3,4]] = ARRAY[1,2,3,4], ARRAY[[1,2],[3,4]] > ARRAY[1,2,3,4], ARRAY[1,NULL]::INT[] = ARRAY[1,NULL]::INT[], ARRAY[1,2] < ARRAY[1,NULL]::INT[]",
        None,
        0,
        true,
    );
    let lower_bound_cycle = fixture::execute_cycle(
        "SELECT $1::INT[] < ARRAY[1,2], $1::INT[] = ARRAY[1,2]",
        Some((1007, 0, Some(b"[0:1]={1,2}"))),
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles(
        "type-array-comparison-order",
        &[],
        vec![cycle, lower_bound_cycle],
    );

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("array comparison RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![16; 6]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![
            Some("t".to_owned()),
            Some("t".to_owned()),
            Some("f".to_owned()),
            Some("t".to_owned()),
            Some("t".to_owned()),
            Some("t".to_owned()),
        ]]
    );

    let lower_bound_frames = &batches[1];
    fixture::assert_success(lower_bound_frames);
    assert_eq!(
        wire::parse_row_description(
            &lower_bound_frames
                .iter()
                .find(|(tag, _)| *tag == b'T')
                .expect("lower-bound comparison RowDescription")
                .1
        )
        .iter()
        .map(|column| (column.type_oid, column.type_size))
        .collect::<Vec<_>>(),
        vec![(16, 1), (16, 1)]
    );
    assert_eq!(
        wire::data_rows(lower_bound_frames),
        vec![vec![Some("t".to_owned()), Some("f".to_owned())]]
    );
}

#[test]
fn should_trim_array_tail_across_empty_full_multidimensional_cases() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT trim_array(ARRAY[1,2,3,4], 2), trim_array(ARRAY[1,2,3], 0), trim_array(ARRAY[]::INT[], 0), trim_array(ARRAY[1,2,3], 3), trim_array(ARRAY[[1,2],[3,4]], 1), trim_array(NULL::INT[], 1), trim_array(ARRAY[1,2], NULL::INT)",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("type-array-trim-array-contract", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("trim_array RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![1007; 7]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![
            Some("{1,2}".to_owned()),
            Some("{1,2,3}".to_owned()),
            Some("{}".to_owned()),
            Some("{}".to_owned()),
            Some("{{1,2}}".to_owned()),
            None,
            None,
        ]]
    );
}
