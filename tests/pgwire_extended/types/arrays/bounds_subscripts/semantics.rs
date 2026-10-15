#[test]
fn should_round_trip_multidimensional_integer_array_shape_over_pgwire() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT $1::INT4[] AS value",
        Some((1007, 0, Some(&b"{{1,NULL},{3,4}}"[..]))),
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("type-array-multidimensional-input", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("multidimensional array RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| (column.type_oid, column.type_size))
            .collect::<Vec<_>>(),
        vec![(1007, -1)]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("{{1,NULL},{3,4}}".to_owned())]]
    );
}

#[test]
fn should_preserve_array_dimensions_in_dimension_functions() {
    // Arrange
    let input = b"[0:2]={7,NULL,9}";
    let sql = "SELECT array_dims($1::INT[]) AS dimensions, array_lower($1::INT[], 1) AS lower_bound, array_upper($1::INT[], 1) AS upper_bound, array_length($1::INT[], 1) AS length, cardinality($1::INT[]) AS cardinality, array_position($1::INT[], NULL) AS null_position";
    let cycle = fixture::execute_cycle(sql, Some((1007, 0, Some(input))), 0, true);

    // Act
    let batches = fixture::run_cycles("type-array-bounds-and-functions", &[], vec![cycle]);

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
            Some("[0:2]".to_owned()),
            Some("0".to_owned()),
            Some("2".to_owned()),
            Some("3".to_owned()),
            Some("3".to_owned()),
            Some("1".to_owned()),
        ]]
    );
}

#[test]
fn should_apply_array_functions_to_nondefault_bounds() {
    // Arrange
    let input = b"[0:2]={7,NULL,9}";
    let sql = "SELECT array_append($1::INT[], 11)::TEXT, array_remove($1::INT[], NULL)::TEXT, array_replace($1::INT[], 7, 8)::TEXT, array_positions($1::INT[], NULL)::TEXT";
    let cycle = fixture::execute_cycle(sql, Some((1007, 0, Some(input))), 0, true);

    // Act
    let batches = fixture::run_cycles("type-array-null-functions", &[], vec![cycle]);

    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![
            Some("[0:3]={7,NULL,9,11}".to_owned()),
            Some("[0:1]={7,9}".to_owned()),
            Some("[0:2]={8,NULL,9}".to_owned()),
            Some("{1}".to_owned()),
        ]]
    );
}

#[test]
fn should_insert_subscripted_elements_at_the_requested_lower_bounds() {
    // Arrange
    let setup = [(
        "CREATE TABLE issue_807_insert_array_subscripts (id INT PRIMARY KEY, items INT[])",
        Vec::new(),
    )];
    let insert_two = fixture::execute_cycle(
        "INSERT INTO issue_807_insert_array_subscripts (id, items[2]) VALUES (1, 7) RETURNING id, items::TEXT",
        None,
        0,
        true,
    );
    let insert_zero = fixture::execute_cycle(
        "INSERT INTO issue_807_insert_array_subscripts (id, items[0]) VALUES (2, 9) RETURNING id, items::TEXT",
        None,
        0,
        true,
    );
    let readback = fixture::execute_cycle(
        "SELECT id, items::TEXT FROM issue_807_insert_array_subscripts ORDER BY id",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles(
        "issue-807-insert-array-subscript-lower-bounds",
        &setup,
        vec![insert_two, insert_zero, readback],
    );

    // Assert
    for frames in &batches {
        fixture::assert_success(frames);
        let description = frames
            .iter()
            .find(|(tag, _)| *tag == b'T')
            .expect("subscripted INSERT RowDescription");
        assert_eq!(
            wire::parse_row_description(&description.1)
                .iter()
                .map(|column| column.type_oid)
                .collect::<Vec<_>>(),
            vec![23, 25]
        );
    }
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![Some("1".to_owned()), Some("[2:2]={7}".to_owned())]]
    );
    assert_eq!(
        wire::data_rows(&batches[1]),
        vec![vec![Some("2".to_owned()), Some("[0:0]={9}".to_owned())]]
    );
    assert_eq!(
        wire::data_rows(&batches[2]),
        vec![
            vec![Some("1".to_owned()), Some("[2:2]={7}".to_owned())],
            vec![Some("2".to_owned()), Some("[0:0]={9}".to_owned())],
        ]
    );
}

#[test]
fn should_round_trip_multidimensional_text_array_bounds() {
    // Arrange
    let input = b"[0:1][4:5]={{1,2},{3,4}}";
    let cycle = fixture::execute_cycle(
        "SELECT $1::INT4[] AS value",
        Some((1007, 0, Some(input))),
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("type-array-text-multidimensional-bounds", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (1007, -1, -1), 0, true);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("[0:1][4:5]={{1,2},{3,4}}".to_owned())]]
    );
}

#[test]
fn should_fill_multidimensional_arrays_from_dimension_array() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT array_fill(7, $1::INT[])::TEXT",
        Some((1007, 0, Some(&b"{2,3}"[..]))),
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("type-array-fill-dimensions", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (25, -1, -1), 0, true);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("{{7,7,7},{7,7,7}}".to_owned())]]
    );
}

#[test]
fn should_subscript_an_array_at_its_second_element() {
    // Arrange
    let sql = "SELECT (ARRAY[2,4,6])[2]";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("type-array-subscript", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("array subscript RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![23]
    );
    assert_eq!(wire::data_rows(frames), vec![vec![Some("4".to_owned())]]);
}

#[test]
fn should_compare_any_all_against_a_bound_integer_array() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT 4 = ANY ($1), 4 = ALL ($1)",
        Some((1007, 0, Some(b"{2,4,6}"))),
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("type-array-any-all-values", &[], vec![cycle]);

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
        vec![vec![Some("t".to_owned()), Some("f".to_owned())]]
    );
}

#[test]
fn should_subscript_a_table_array_slice() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_807_array_slice_rows (id INT, items INT[])",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_807_array_slice_rows VALUES (1, $1)",
            vec![Value::Json(serde_json::json!([10, 20, 30, 40]))],
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT items[2:3]::TEXT FROM issue_807_array_slice_rows WHERE id = 1",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("type-array-slice-select", &setup, vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (25, -1, -1), 0, true);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("{20,30}".to_owned())]]
    );
}

#[test]
fn should_subscript_a_multidimensional_array_expression_with_custom_bounds() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT ($1::INT[])[1][5] AS selected_value",
        Some((1007, 0, Some(b"[0:1][4:5]={{1,2},{3,4}}"))),
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles(
        "type-array-expression-multidimensional-subscript",
        &[],
        vec![cycle],
    );

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (23, 4, -1), 0, true);
    assert_eq!(wire::data_rows(frames), vec![vec![Some("4".to_owned())]]);
}

#[test]
fn should_report_multidimensional_bounds_with_out_of_range_values() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT array_dims($1::INT[]), array_ndims($1::INT[]), array_lower($1::INT[], 1), array_upper($1::INT[], 1), array_length($1::INT[], 1), array_lower($1::INT[], 2), array_upper($1::INT[], 2), array_length($1::INT[], 2), array_lower($1::INT[], 3), array_upper($1::INT[], 3), array_length($1::INT[], 3)",
        Some((1007, 0, Some(b"[0:1][4:5]={{1,2},{3,4}}"))),
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles(
        "type-array-multidimensional-bounds-functions",
        &[],
        vec![cycle],
    );

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("multidimensional array bounds RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| (column.type_oid, column.type_size))
            .collect::<Vec<_>>(),
        vec![
            (25, -1),
            (23, 4),
            (23, 4),
            (23, 4),
            (23, 4),
            (23, 4),
            (23, 4),
            (23, 4),
            (23, 4),
            (23, 4),
            (23, 4),
        ]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![
            Some("[0:1][4:5]".to_owned()),
            Some("2".to_owned()),
            Some("0".to_owned()),
            Some("1".to_owned()),
            Some("2".to_owned()),
            Some("4".to_owned()),
            Some("5".to_owned()),
            Some("2".to_owned()),
            None,
            None,
            None,
        ]]
    );
}
