#[test]
fn should_preserve_left_rows_for_nonproducing_lateral_unnest_inputs() {
    // Arrange
    let sql = "SELECT source.id, expanded.value, expanded.ordinality \
        FROM (VALUES (1, NULL::INT4[]), (2, ARRAY[]::INT4[])) \
             AS source(id, items) \
        LEFT JOIN LATERAL unnest(source.items) WITH ORDINALITY \
             AS expanded(value, ordinality) ON TRUE \
        ORDER BY source.id, expanded.ordinality";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("issue-808-lateral-unnest-null-empty", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("lateral UNNEST RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        [23, 23, 20]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![
            vec![Some("1".to_owned()), None, None],
            vec![Some("2".to_owned()), None, None],
        ]
    );
}
