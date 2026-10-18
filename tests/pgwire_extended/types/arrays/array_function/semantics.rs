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
fn should_prepend_elements_while_preserving_nondefault_array_bounds() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT ARRAY_PREPEND(0, $1::INT[])::TEXT",
        Some((1_007, 0, Some(b"[0:2]={7,NULL,9}"))),
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-807-array-prepend-bounds", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("[0:3]={0,7,NULL,9}".to_owned())]]
    );
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

#[test]
fn should_fill_multidimensional_arrays_from_explicit_lower_bounds() {
    // Arrange
    let cycle = vec![
        wire::parse_frame_with_types(
            "",
            "SELECT ARRAY_FILL(7, $1::INT[], $2::INT[])::TEXT",
            &[1_007, 1_007],
        ),
        wire::bind_frame_with_formats("", "", &[0, 0], &[Some(b"{2,3}"), Some(b"{0,-1}")], &[]),
        wire::execute_frame(""),
        wire::sync_frame(),
    ];

    // Act
    let batches = fixture::run_cycles("issue-807-array-fill-lower-bounds", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("[0:1][-1:1]={{7,7,7},{7,7,7}}".to_owned())]]
    );
}

#[test]
fn should_generate_reverse_subscripts_for_each_array_dimension() {
    // Arrange
    let sql = "SELECT dimensions.dimension, subscripts.subscript, subscripts.ordinality \
               FROM (VALUES (1), (2), (3)) AS dimensions(dimension) \
               LEFT JOIN LATERAL generate_subscripts( \
                 '[0:1][2:3]={{10,20},{30,40}}'::INTEGER[], \
                 dimensions.dimension, TRUE \
               ) WITH ORDINALITY AS subscripts(subscript, ordinality) ON TRUE \
               ORDER BY dimensions.dimension, subscripts.ordinality";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles(
        "issue-808-multidimensional-reverse-subscripts",
        &[],
        vec![cycle],
    );

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("generate_subscripts RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![23, 23, 20]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![
            vec![
                Some("1".to_owned()),
                Some("1".to_owned()),
                Some("1".to_owned())
            ],
            vec![
                Some("1".to_owned()),
                Some("0".to_owned()),
                Some("2".to_owned())
            ],
            vec![
                Some("2".to_owned()),
                Some("3".to_owned()),
                Some("1".to_owned())
            ],
            vec![
                Some("2".to_owned()),
                Some("2".to_owned()),
                Some("2".to_owned())
            ],
            vec![Some("3".to_owned()), None, None],
        ]
    );
}
