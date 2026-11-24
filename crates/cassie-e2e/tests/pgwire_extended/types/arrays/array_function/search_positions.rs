#[test]
fn should_return_all_repeated_custom_bound_subscripts() {
    // Arrange
    let input = b"[0:4]={7,NULL,7,NULL,7}";
    let cycle = fixture::execute_cycle(
        "SELECT array_position($1::INT[], 7), array_positions($1::INT[], 7)",
        Some((1_007, 0, Some(input))),
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-807-array-search-positions", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("array position RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| (column.type_oid, column.type_size, column.type_mod))
            .collect::<Vec<_>>(),
        vec![(23, 4, -1), (1_007, -1, -1)]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("0".to_owned()), Some("{0,2,4}".to_owned()),]]
    );
}
