#[test]
fn should_return_a_boolean_for_a_matching_json_exists_path() {
    // Arrange
    let sql = "SELECT JSON_EXISTS('{\"n\":42}', '$.n')";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("type-sql-json-query-functions", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("JSON_EXISTS RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![16]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("t".to_owned())]]
    );
}

#[test]
fn should_default_json_exists_structural_errors_to_false() {
    // Arrange
    let sql = "SELECT JSON_EXISTS('{\"a\":1}'::JSONB, 'strict $.a[*]'), \
        JSON_EXISTS('{}'::JSONB, 'strict $.missing')";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("type-sql-json-exists-default-error-policy", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("JSON_EXISTS default policy RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![16, 16]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("f".to_owned()), Some("f".to_owned())]]
    );
}
