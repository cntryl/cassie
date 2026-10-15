#[test]
fn should_return_typed_sql_json_query_exists_results() {
    // Arrange
    let sql = "SELECT JSON_VALUE('{\"n\":42}', '$.n' RETURNING INTEGER), JSON_QUERY('{\"n\":[1,2]}', '$.n'), JSON_EXISTS('{\"n\":42}', '$.n')";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("type-sql-json-query-functions", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("SQL/JSON query function RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![23, 114, 16]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![
            Some("42".to_owned()),
            Some("[1, 2]".to_owned()),
            Some("t".to_owned()),
        ]]
    );
}

#[test]
fn should_preserve_bigint_returning_identity_for_json_value() {
    // Arrange
    let sql = "SELECT JSON_VALUE(\
                   '{\"n\":9223372036854775807}', \
                   '$.n' RETURNING BIGINT\
               ) AS value";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("issue-810-json-value-returning-bigint", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (20, 8, -1), 0, true);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("9223372036854775807".to_owned())]]
    );
}

#[test]
fn should_preserve_jsonb_returning_identity_for_json_query() {
    // Arrange
    let sql = "SELECT JSON_QUERY(\
                   '{\"n\":[1,2]}'::JSONB, \
                   '$.n' RETURNING JSONB\
               ) AS value";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("issue-810-json-query-returning-jsonb", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (3_802, -1, -1), 0, true);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("[1, 2]".to_owned())]]
    );
}

#[test]
fn should_apply_sql_json_policy_distinctions_with_declared_result_types() {
    // Arrange
    let value_policy = fixture::execute_cycle(
        "SELECT JSON_VALUE('{}', '$.n' RETURNING INTEGER DEFAULT 7 ON EMPTY DEFAULT 9 ON ERROR), JSON_VALUE('{\"n\":\"bad\"}', '$.n' RETURNING INTEGER DEFAULT 7 ON EMPTY DEFAULT 9 ON ERROR)",
        None,
        0,
        true,
    );
    let query_empty = fixture::execute_cycle(
        "SELECT JSON_QUERY('{}', '$.n' RETURNING JSONB EMPTY ARRAY ON EMPTY ERROR ON ERROR)",
        None,
        0,
        true,
    );
    let query_multiple = fixture::execute_cycle(
        "SELECT JSON_QUERY('{\"n\":[1,2]}', '$.n[*]' RETURNING JSONB ERROR ON EMPTY ERROR ON ERROR)",
        None,
        0,
        true,
    );
    let recovery = fixture::execute_cycle("SELECT 42", None, 0, true);

    // Act
    let batches = fixture::run_cycles(
        "issue-810-sql-json-empty-error-policies",
        &[],
        vec![value_policy, query_empty, query_multiple, recovery],
    );

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("JSON_VALUE policy RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![23, 23]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("7".to_owned()), Some("9".to_owned())]]
    );

    let frames = &batches[1];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (3_802, -1, -1), 0, true);
    assert_eq!(wire::data_rows(frames), vec![vec![Some("[]".to_owned())]]);

    let frames = &batches[2];
    assert_eq!(wire::error_code(frames).as_deref(), Some("22034"));
    let errors = frames
        .iter()
        .filter(|(tag, _)| *tag == b'E')
        .collect::<Vec<_>>();
    assert_eq!(errors.len(), 1);
    let message = wire::parse_error_fields(&errors[0].1)
        .into_iter()
        .find_map(|(key, value)| (key == 'M').then_some(value))
        .expect("JSON_QUERY cardinality error message");
    assert_eq!(
        message,
        "JSON path expression in JSON_QUERY must return single item when no wrapper is requested"
    );
    assert_eq!(wire::data_rows(frames), Vec::<Vec<Option<String>>>::new());

    fixture::assert_success(&batches[3]);
    assert_eq!(
        wire::data_rows(&batches[3]),
        vec![vec![Some("42".to_owned())]]
    );
}
