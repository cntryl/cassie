#[test]
fn should_preserve_json_builder_result_type_identities() {
    // Arrange
    let sql = "SELECT json_build_object('wide',9007199254740993,'keep',NULL), \
        jsonb_build_object('wide',9007199254740993,'keep',NULL), \
        json_build_array(1,NULL,'x'), jsonb_build_array(1,NULL,'x')";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("issue-810-json-builder-alias-oids", &[], vec![cycle]);

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
            Some(r#"{"wide" : 9007199254740993, "keep" : null}"#.to_owned()),
            Some(r#"{"keep": null, "wide": 9007199254740993}"#.to_owned()),
            Some(r#"[1, null, "x"]"#.to_owned()),
            Some(r#"[1, null, "x"]"#.to_owned()),
        ]]
    );
}

#[test]
fn should_preserve_to_json_result_type_identity() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT to_json(42), to_json('Ada'::TEXT), to_jsonb(42)",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-810-to-json-type-identity", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_eq!(
        column_signatures(frames),
        vec![(114, -1, -1), (114, -1, -1), (3_802, -1, -1)]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![
            Some("42".to_owned()),
            Some(r#""Ada""#.to_owned()),
            Some("42".to_owned()),
        ]]
    );
}

#[test]
fn should_expand_json_object_keys_as_text_rows() {
    // Arrange
    let sql = "SELECT json_object_keys('{\"z\":1,\"a\":2}'::JSON) ORDER BY 1";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("issue-810-json-object-keys-json-alias", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_eq!(column_signatures(frames), vec![(25, -1, -1)]);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("a".to_owned())], vec![Some("z".to_owned())]]
    );
}
