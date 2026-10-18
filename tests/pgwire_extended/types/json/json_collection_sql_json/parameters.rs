#[test]
fn should_bind_passing_variables_in_sql_json_query_functions() {
    // Arrange
    let sql = "SELECT \
        JSON_VALUE('{\"n\":40}'::JSONB, '$.n + $increment' PASSING 2 AS increment RETURNING BIGINT), \
        JSON_QUERY('{\"items\":[{\"n\":1},{\"n\":2}]}'::JSONB, '$.items[*] ? (@.n == $desired)' PASSING 2 AS desired RETURNING JSONB), \
        JSON_EXISTS('{\"items\":[{\"n\":1},{\"n\":2}]}'::JSONB, '$.items[*] ? (@.n > $threshold)' PASSING 1 AS threshold)";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("issue-810-sql-json-passing-variables", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_eq!(column_type_oids(frames), vec![20, 3_802, 16]);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![
            Some("42".to_owned()),
            Some("{\"n\": 2}".to_owned()),
            Some("t".to_owned()),
        ]]
    );
}
