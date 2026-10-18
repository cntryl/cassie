#[test]
fn should_expand_json_each_as_json_typed_object_rows() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT key, value FROM json_each('{\"a\":null,\"b\":{\"k\":[1,2]}}') ORDER BY key",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-810-json-each-values", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_eq!(column_type_oids(frames), vec![25, 114]);
    assert_eq!(
        wire::data_rows(frames),
        vec![
            vec![Some("a".to_owned()), Some("null".to_owned())],
            vec![Some("b".to_owned()), Some("{\"k\":[1,2]}".to_owned())],
        ]
    );
}

#[test]
fn should_expand_jsonb_each_as_jsonb_typed_object_rows() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT key, value FROM jsonb_each('{\"a\":null,\"b\":{\"k\":[1,2]}}'::JSONB) ORDER BY key",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-829-jsonb-each-values", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_eq!(column_type_oids(frames), vec![25, 3_802]);
    assert_eq!(
        wire::data_rows(frames),
        vec![
            vec![Some("a".to_owned()), Some("null".to_owned())],
            vec![Some("b".to_owned()), Some("{\"k\": [1, 2]}".to_owned())],
        ]
    );
}

#[test]
fn should_build_text_array_json_objects_with_jsonb_identity() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT \
            json_object(ARRAY['a','1','b','two']::TEXT[]), \
            jsonb_object(ARRAY['a','1','b','two']::TEXT[]), \
            json_object(ARRAY['a','b']::TEXT[], ARRAY['1','two']::TEXT[]), \
            jsonb_object(ARRAY['a','b']::TEXT[], ARRAY['1','two']::TEXT[])",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-810-legacy-text-array-json-objects", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_eq!(column_type_oids(frames), vec![114, 3_802, 114, 3_802]);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![
            Some(r#"{"a" : "1", "b" : "two"}"#.to_owned()),
            Some(r#"{"a": "1", "b": "two"}"#.to_owned()),
            Some(r#"{"a" : "1", "b" : "two"}"#.to_owned()),
            Some(r#"{"a": "1", "b": "two"}"#.to_owned()),
        ]]
    );
}
