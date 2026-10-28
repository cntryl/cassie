
#[test]
fn should_preserve_large_integer_values_in_jsonb_constructors() {
    // Arrange
    let sql = "SELECT jsonb_build_object('wide', 9007199254740993, 'sql_null', NULL), jsonb_build_array(1, NULL, 'x'), jsonb_typeof(jsonb_build_object('a', 1))";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("type-jsonb-builders-exact-values", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("JSONB builder RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![3802, 3802, 25]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![
            Some(r#"{"wide": 9007199254740993, "sql_null": null}"#.to_owned()),
            Some(r#"[1, null, "x"]"#.to_owned()),
            Some("object".to_owned()),
        ]]
    );
}

#[test]
fn should_preserve_json_object_types_for_strict_unique_forms() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_811_json_object_variants (key TEXT, value INT)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_811_json_object_variants VALUES ('a', 1), ('b', NULL)",
            Vec::new(),
        ),
    ];
    let cases = [
        (
            "json_object_agg_strict",
            "SELECT json_object_agg_strict(key, value) FROM issue_811_json_object_variants",
            114,
            "{ \"a\" : 1 }",
        ),
        (
            "json_object_agg_unique",
            "SELECT json_object_agg_unique(key, value) FROM issue_811_json_object_variants",
            114,
            "{ \"a\" : 1, \"b\" : null }",
        ),
        (
            "jsonb_object_agg_unique",
            "SELECT jsonb_object_agg_unique(key, value) FROM issue_811_json_object_variants",
            3_802,
            "{\"a\": 1, \"b\": null}",
        ),
        (
            "json_object_agg_unique_strict",
            "SELECT json_object_agg_unique_strict(key, value) FROM issue_811_json_object_variants",
            114,
            "{ \"a\" : 1 }",
        ),
    ];
    let cycles = cases
        .iter()
        .map(|(_, sql, _, _)| fixture::execute_cycle(sql, None, 0, true))
        .collect();

    // Act
    let batches = fixture::run_cycles("issue-811-json-object-strict-unique", &setup, cycles);

    // Assert
    let mut mismatches = Vec::new();
    for ((name, _, expected_oid, expected_value), frames) in cases.iter().zip(&batches) {
        if let Some(code) = wire::error_code(frames) {
            mismatches.push(format!("{name} returned SQLSTATE {code}"));
            continue;
        }
        let Some(description) = frames.iter().find(|(tag, _)| *tag == b'T') else {
            mismatches.push(format!("{name} returned no RowDescription"));
            continue;
        };
        let actual_oid = wire::parse_row_description(&description.1)[0].type_oid;
        if actual_oid != *expected_oid {
            mismatches.push(format!("{name} returned type OID {actual_oid}"));
        }
        let expected_rows = vec![vec![Some((*expected_value).to_owned())]];
        let actual_rows = wire::data_rows(frames);
        if actual_rows != expected_rows {
            mismatches.push(format!("{name} returned rows {actual_rows:?}"));
        }
    }
    assert!(
        mismatches.is_empty(),
        "aggregate mismatches: {mismatches:?}"
    );
}

#[test]
fn should_convert_scalar_values_with_to_jsonb() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_810_to_jsonb_values (value INT)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_810_to_jsonb_values VALUES (42)",
            Vec::new(),
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT to_jsonb(value) FROM issue_810_to_jsonb_values",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-810-to-jsonb-scalar", &setup, vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (3802, -1, -1), 0, true);
    assert_eq!(wire::data_rows(frames), vec![vec![Some("42".to_owned())]]);
}
