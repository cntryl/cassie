#[test]
fn should_correlate_series_ordinality_to_each_outer_row() {
    // Arrange
    let sql = "SELECT source.id, generated.value, generated.ordinality FROM (VALUES (1, 2, 4), (2, 5, 6), (3, 3, 2)) AS source(id, first_value, last_value) LEFT JOIN LATERAL generate_series(source.first_value, source.last_value) WITH ORDINALITY AS generated(value, ordinality) ON TRUE ORDER BY source.id, generated.ordinality";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles(
        "issue-808-correlated-generate-series-ordinality",
        &[],
        vec![cycle],
    );

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("correlated generate_series RowDescription");
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
                Some("2".to_owned()),
                Some("1".to_owned())
            ],
            vec![
                Some("1".to_owned()),
                Some("3".to_owned()),
                Some("2".to_owned())
            ],
            vec![
                Some("1".to_owned()),
                Some("4".to_owned()),
                Some("3".to_owned())
            ],
            vec![
                Some("2".to_owned()),
                Some("5".to_owned()),
                Some("1".to_owned())
            ],
            vec![
                Some("2".to_owned()),
                Some("6".to_owned()),
                Some("2".to_owned())
            ],
            vec![Some("3".to_owned()), None, None],
        ]
    );
}

#[test]
fn should_correlate_scalar_function_from_rows_with_ordinality() {
    // Arrange
    let sql = "SELECT source.id, generated.value, generated.ordinality \
               FROM (VALUES (1, 'ALPHA'), (2, 'Beta')) AS source(id, name) \
               CROSS JOIN LATERAL lower(source.name) WITH ORDINALITY \
                 AS generated(value, ordinality) \
               ORDER BY source.id";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles(
        "issue-808-correlated-scalar-from-ordinality",
        &[],
        vec![cycle],
    );

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("correlated scalar function RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![23, 25, 20]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![
            vec![
                Some("1".to_owned()),
                Some("alpha".to_owned()),
                Some("1".to_owned()),
            ],
            vec![
                Some("2".to_owned()),
                Some("beta".to_owned()),
                Some("1".to_owned()),
            ],
        ]
    );
}
