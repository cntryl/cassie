#[test]
fn should_preserve_nondefault_array_parameters_across_index_restart() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_807_array_bound_keys (id INT, items INT[])",
            Vec::new(),
        ),
        (
            "CREATE INDEX issue_807_array_bound_keys_items_idx ON issue_807_array_bound_keys USING btree (items)",
            Vec::new(),
        ),
    ];
    let nondefault_bounds = b"[0:2]={7,NULL,9}";
    let insert = fixture::execute_cycle(
        "INSERT INTO issue_807_array_bound_keys VALUES (1, $1)",
        Some((1007, 0, Some(nondefault_bounds))),
        0,
        false,
    );
    let query = fixture::execute_cycle(
        "SELECT items::TEXT FROM issue_807_array_bound_keys WHERE items = $1 ORDER BY id",
        Some((1007, 0, Some(nondefault_bounds))),
        0,
        true,
    );

    // Act
    let stages = fixture::run_cycles_across_restart(
        "type-array-bounds-index-restart",
        &setup,
        vec![vec![insert], vec![query]],
    );

    // Assert
    fixture::assert_success(&stages[0][0]);
    assert!(stages[0][0].iter().any(|(tag, _)| *tag == b'C'));
    let frames = &stages[1][0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (25, -1, -1), 0, true);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("[0:2]={7,NULL,9}".to_owned())]],
        "the nondefault lower bound survives the btree index and restart"
    );
}

#[test]
fn should_round_trip_nondefault_array_bounds_across_index_restart() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_807_array_binary_input (id INT, items INT[])",
            Vec::new(),
        ),
        (
            "CREATE INDEX issue_807_array_binary_input_items_idx ON issue_807_array_binary_input USING btree (items)",
            Vec::new(),
        ),
    ];
    let input = b"\0\0\0\x01\0\0\0\x01\0\0\0\x17\0\0\0\x03\0\0\0\0\0\0\0\x04\0\0\0\x07\xff\xff\xff\xff\0\0\0\x04\0\0\0\x09";
    let insert = fixture::execute_cycle(
        "INSERT INTO issue_807_array_binary_input VALUES (1, $1)",
        Some((1007, 1, Some(input))),
        0,
        false,
    );
    let lookup = fixture::execute_cycle(
        "SELECT items FROM issue_807_array_binary_input WHERE items = $1",
        Some((1007, 0, Some(b"[0:2]={7,NULL,9}"))),
        0,
        true,
    );

    // Act
    let stages = fixture::run_cycles_across_restart(
        "type-array-binary-lower-bound",
        &setup,
        vec![vec![insert], vec![lookup]],
    );

    // Assert
    let frames = &stages[0][0];
    fixture::assert_success(frames);
    assert_eq!(
        frames
            .iter()
            .find(|(tag, _)| *tag == b'C')
            .map(|(_, body)| body.as_slice()),
        Some(&b"INSERT 0 1\0"[..])
    );
    let frames = &stages[1][0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (1007, -1, -1), 0, true);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("[0:2]={7,NULL,9}".to_owned())]]
    );
}
