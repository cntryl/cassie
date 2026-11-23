
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

#[test]
fn should_keep_sql_null_values_as_json_null_in_object_arrays() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT json_object('{a,b}', '{1,NULL}'), jsonb_object('{a,NULL}'), json_object('{a}', '{NULL}')",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-1468-null-json-object-values", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![
            Some(r#"{"a" : "1", "b" : null}"#.to_owned()),
            Some(r#"{"a": null}"#.to_owned()),
            Some(r#"{"a" : null}"#.to_owned()),
        ]]
    );
}

#[test]
fn should_return_sql_null_for_whole_null_standard_object_arrays() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT json_object(NULL::TEXT[]), jsonb_object(NULL::TEXT[]), json_object(ARRAY['a']::TEXT[], NULL::TEXT[])",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-1468-whole-null-object-arrays", &[], vec![cycle]);

    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![None, None, None]]
    );
}

#[test]
fn should_resolve_case_insensitive_and_qualified_standard_object_names() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT JSON_OBJECT('{a,b}'), jSoN_Object('{a,b}'), pg_catalog.JSON_OBJECT('{a,b}'), JSONB_OBJECT(NULL::TEXT[]), pg_catalog.JSONB_OBJECT(NULL::TEXT[])",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-1468-object-name-resolution", &[], vec![cycle]);

    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![
            Some(r#"{"a" : "b"}"#.to_owned()),
            Some(r#"{"a" : "b"}"#.to_owned()),
            Some(r#"{"a" : "b"}"#.to_owned()),
            None,
            None,
        ]]
    );
}

#[test]
fn should_report_json_object_array_shape_sqlstates() {
    // Arrange
    let odd = fixture::execute_cycle("SELECT json_object('{a}')", None, 0, true);
    let mismatch = fixture::execute_cycle(
        "SELECT jsonb_object('{a,b}', '{1}')",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles(
        "issue-1470-json-object-array-errors",
        &[],
        vec![odd, mismatch],
    );

    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("2202E"));
    assert_eq!(wire::error_code(&batches[1]).as_deref(), Some("2202E"));
}

#[test]
fn should_report_null_json_object_keys_with_null_not_null_sqlstate() {
    // Arrange
    let cycle = fixture::execute_cycle("SELECT json_object('{NULL}', '{1}')", None, 0, true);

    // Act
    let batches = fixture::run_cycles("issue-1470-json-object-null-key", &[], vec![cycle]);

    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("22004"));
}
