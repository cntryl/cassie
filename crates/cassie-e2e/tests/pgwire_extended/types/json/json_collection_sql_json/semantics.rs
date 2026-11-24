
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
