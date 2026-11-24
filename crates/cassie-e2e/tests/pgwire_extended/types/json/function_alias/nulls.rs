#[test]
fn should_return_json_array_lengths_with_sql_nulls() {
    // Arrange
    let sql = "SELECT json_array_length('[1,null,3]'::JSON), \
        jsonb_array_length('[1,null,3]'::JSONB), \
        json_array_length('[]'::JSON), jsonb_array_length('[]'::JSONB), \
        json_array_length(NULL::JSON), jsonb_array_length(NULL::JSONB)";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("issue-810-json-array-length-null-aliases", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_eq!(column_signatures(frames), vec![(23, 4, -1); 6]);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![
            Some("3".to_owned()),
            Some("3".to_owned()),
            Some("0".to_owned()),
            Some("0".to_owned()),
            None,
            None,
        ]]
    );
}

#[test]
fn should_strip_json_null_fields_under_the_array_option() {
    // Arrange
    let document = r#"{"drop":null,"items":[null,1]}"#;
    let sql = format!(
        "SELECT json_strip_nulls('{document}'::JSON), \
            jsonb_strip_nulls('{document}'::JSONB), \
            json_strip_nulls('{document}'::JSON, TRUE), \
            jsonb_strip_nulls('{document}'::JSONB, TRUE)"
    );
    let cycle = fixture::execute_cycle(&sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("issue-810-json-strip-nulls-array-option", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_eq!(
        column_signatures(frames),
        vec![
            (114, -1, -1),
            (3_802, -1, -1),
            (114, -1, -1),
            (3_802, -1, -1)
        ]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![
            Some(r#"{"items":[null,1]}"#.to_owned()),
            Some(r#"{"items": [null, 1]}"#.to_owned()),
            Some(r#"{"items":[1]}"#.to_owned()),
            Some(r#"{"items": [1]}"#.to_owned()),
        ]]
    );
}

#[test]
fn should_apply_empty_or_null_key_arrays_to_jsonb_existence_functions() {
    // Arrange
    let sql = "SELECT jsonb_exists_any('{\"a\":1}'::JSONB, '{}'::TEXT[]), \
        jsonb_exists_all('{\"a\":1}'::JSONB, '{}'::TEXT[]), \
        jsonb_exists_any(NULL::JSONB, '{a}'::TEXT[]), \
        jsonb_exists_all('{\"a\":1}'::JSONB, NULL::TEXT[]), \
        jsonb_exists_any('null'::JSONB, '{a}'::TEXT[]), \
        jsonb_exists_all('null'::JSONB, '{}'::TEXT[])";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("issue-810-jsonb-exists-null-empty-keys", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_eq!(column_signatures(frames), vec![(16, 1, -1); 6]);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![
            Some("f".to_owned()),
            Some("t".to_owned()),
            None,
            None,
            Some("f".to_owned()),
            Some("t".to_owned()),
        ]]
    );
}

#[test]
fn should_apply_jsonb_set_lax_null_treatments() {
    // Arrange
    let sql = "SELECT \
        jsonb_set_lax('{\"a\":1,\"b\":2}'::JSONB, '{a}', NULL::JSONB, TRUE, 'use_json_null'), \
        jsonb_set_lax('{\"a\":1,\"b\":2}'::JSONB, '{a}', NULL::JSONB, TRUE, 'return_target'), \
        jsonb_set_lax('{\"a\":1,\"b\":2}'::JSONB, '{a}', NULL::JSONB, TRUE, 'delete_key'), \
        jsonb_set_lax('{\"a\":1,\"b\":2}'::JSONB, '{missing}', NULL::JSONB, TRUE, 'delete_key')";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("issue-810-jsonb-set-lax-null-treatments", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_eq!(column_signatures(frames), vec![(3_802, -1, -1); 4]);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![
            Some(r#"{"a": null, "b": 2}"#.to_owned()),
            Some(r#"{"a": 1, "b": 2}"#.to_owned()),
            Some(r#"{"b": 2}"#.to_owned()),
            Some(r#"{"a": 1, "b": 2}"#.to_owned()),
        ]]
    );
}
