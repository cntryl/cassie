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
