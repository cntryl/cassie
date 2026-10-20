#[test]
fn should_return_parenthesized_row_arithmetic_from_update() {
    // Arrange
    let batches = run_script(
        "issue-796-returning-arithmetic",
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame(
                "CREATE TABLE issue_796_returning_arithmetic (id BIGINT)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "INSERT INTO issue_796_returning_arithmetic VALUES (1), (2)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "UPDATE issue_796_returning_arithmetic SET id=id \
                 RETURNING id, CASE WHEN id<0 THEN id / (id - id) ELSE id END AS selected",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "UPDATE issue_796_returning_arithmetic SET id=id+10 WHERE id<0 \
                 RETURNING id / (id - id) AS selected",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT id FROM issue_796_returning_arithmetic ORDER BY id",
            )]),
        ],
    );

    // Act
    let setup = &batches[..2];
    let returned_rows = &batches[2];
    let returned_no_rows = &batches[3];
    let readback = &batches[4];

    // Assert
    for (index, frames) in setup.iter().enumerate() {
        assert_eq!(
            wire::error_code(frames),
            None,
            "setup statement {index} ErrorResponse: {:?}",
            frames
                .iter()
                .filter(|frame| frame.0 == b'E')
                .map(|frame| wire::parse_error_fields(&frame.1))
                .collect::<Vec<_>>()
        );
    }
    for (label, frames) in [
        ("row-dependent RETURNING", returned_rows),
        ("zero-row RETURNING", returned_no_rows),
    ] {
        assert_eq!(
            wire::error_code(frames),
            None,
            "{label} ErrorResponse: {:?}",
            frames
                .iter()
                .filter(|frame| frame.0 == b'E')
                .map(|frame| wire::parse_error_fields(&frame.1))
                .collect::<Vec<_>>()
        );
    }
    let returned_description = returned_rows
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("row-dependent RETURNING RowDescription");
    assert_eq!(
        wire::parse_row_description(&returned_description.1)
            .iter()
            .map(|column| (column.type_oid, column.type_size, column.type_mod))
            .collect::<Vec<_>>(),
        [(20, 8, -1), (20, 8, -1)]
    );
    assert_eq!(
        wire::data_rows(returned_rows),
        vec![
            vec![Some("1".to_owned()), Some("1".to_owned())],
            vec![Some("2".to_owned()), Some("2".to_owned())],
        ]
    );
    let no_rows_description = returned_no_rows
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("zero-row RETURNING RowDescription");
    let no_rows_columns = wire::parse_row_description(&no_rows_description.1);
    assert_eq!(no_rows_columns.len(), 1);
    let no_rows_column = &no_rows_columns[0];
    assert_eq!(
        (
            no_rows_column.type_oid,
            no_rows_column.type_size,
            no_rows_column.type_mod
        ),
        (20, 8, -1)
    );
    assert_eq!(
        wire::data_rows(returned_no_rows),
        Vec::<Vec<Option<String>>>::new()
    );
    assert_eq!(
        wire::data_rows(readback),
        vec![vec![Some("1".to_owned())], vec![Some("2".to_owned())]]
    );
}
