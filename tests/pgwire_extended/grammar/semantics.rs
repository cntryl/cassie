#[test]
fn should_parse_digit_separators_as_integer_literal_syntax() {
    // Arrange
    let batches = run_script(
        "issue-796-integer-digit-separators",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT 1_000_000 AS separated_integer",
        )])],
    );

    // Act
    let result = &batches[0];

    // Assert
    assert_eq!(wire::error_code(result), None);
    let description = result
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("digit-separated integer RowDescription");
    assert_eq!(wire::parse_row_description(&description.1)[0].type_oid, 23);
    assert_eq!(
        wire::data_rows(result),
        vec![vec![Some("1000000".to_owned())]]
    );
}

#[test]
fn should_decode_unicode_escapes_with_a_custom_escape_character() {
    // Arrange
    let batches = run_script(
        "issue-796-custom-unicode-escape-character",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT U&'a!0062c' UESCAPE '!', U&'d!!t' UESCAPE '!'",
        )])],
    );

    // Act
    let result = &batches[0];

    // Assert
    let errors = result
        .iter()
        .filter(|frame| frame.0 == b'E')
        .map(|frame| wire::parse_error_fields(&frame.1))
        .collect::<Vec<_>>();
    assert!(
        errors.is_empty(),
        "custom Unicode escape query errors: {errors:?}"
    );
    let description = result
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("custom Unicode escape RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![25, 25]
    );
    assert_eq!(
        wire::data_rows(result),
        vec![vec![Some("abc".to_owned()), Some("d!t".to_owned())]]
    );
}

#[test]
fn should_compare_values_from_literal_row_constructors() {
    // Arrange
    let batches = run_script(
        "issue-800-literal-row-equality",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT ROW(1, 2) = ROW(1, 2)",
        )])],
    );

    // Act
    let result = &batches[0];

    // Assert
    assert_eq!(wire::error_code(result), None);
    assert_eq!(wire::data_rows(result), vec![vec![Some("t".to_owned())]]);
}

#[test]
fn should_report_undefined_operator_for_literal_row_image_comparisons() {
    // Arrange
    let operators = ["*=", "*<>", "*<", "*<=", "*>", "*>="];
    let steps = operators
        .iter()
        .map(|operator| {
            Step::ThroughReady(vec![wire::simple_query_frame(&format!(
                "SELECT ROW(1, 2) {operator} ROW(1, 2)"
            ))])
        })
        .collect();
    let batches = run_script("issue-800-row-image-operator-sqlstate", steps);

    // Act
    let errors = batches
        .iter()
        .map(|frames| wire::error_code(frames))
        .collect::<Vec<_>>();

    // Assert
    assert_eq!(errors, vec![Some("42883".to_owned()); operators.len()]);
}

#[test]
fn should_sample_rows_with_a_valid_bernoulli_percentage() {
    // Arrange
    let batches = run_script(
        "issue-800-tablesample-valid-rate",
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame(
                "CREATE TABLE issue_800_sample_valid_rows (id INT)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "INSERT INTO issue_800_sample_valid_rows VALUES (1)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT id FROM issue_800_sample_valid_rows TABLESAMPLE BERNOULLI (100)",
            )]),
        ],
    );

    // Act
    let created = &batches[0];
    let inserted = &batches[1];
    let sampled = &batches[2];

    // Assert
    assert_eq!(wire::error_code(created), None);
    assert_eq!(wire::error_code(inserted), None);
    assert_eq!(wire::error_code(sampled), None);
    assert_eq!(wire::data_rows(sampled), vec![vec![Some("1".to_owned())]]);
}

#[test]
fn should_return_ordered_integer_rows_from_generate_series() {
    // Arrange
    let batches = run_script(
        "issue-808-generate-series-integer-rows",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT generate_series(1, 3)",
        )])],
    );

    // Act
    let frames = &batches[0];

    // Assert
    assert_eq!(
        wire::error_code(frames),
        None,
        "wire error fields: {:?}",
        frames
            .iter()
            .filter(|(tag, _)| *tag == b'E')
            .map(|(_, payload)| wire::parse_error_fields(payload))
            .collect::<Vec<_>>()
    );
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("generate_series RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![23]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![
            vec![Some("1".to_owned())],
            vec![Some("2".to_owned())],
            vec![Some("3".to_owned())],
        ]
    );
}

#[test]
fn should_return_lockstep_columns_from_rows_from_sources() {
    // Arrange
    let batches = run_script(
        "issue-808-rows-from-lockstep-sources",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT * FROM ROWS FROM(generate_series(1, 3), generate_series(10, 12)) AS generated(first_value, second_value)",
        )])],
    );

    // Act
    let frames = &batches[0];

    // Assert
    assert_eq!(
        wire::error_code(frames),
        None,
        "wire error fields: {:?}",
        frames
            .iter()
            .filter(|(tag, _)| *tag == b'E')
            .map(|(_, payload)| wire::parse_error_fields(payload))
            .collect::<Vec<_>>()
    );
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("ROWS FROM RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![23, 23]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![
            vec![Some("1".to_owned()), Some("10".to_owned())],
            vec![Some("2".to_owned()), Some("11".to_owned())],
            vec![Some("3".to_owned()), Some("12".to_owned())],
        ]
    );
}
