#[test]
fn should_evaluate_jsonpath_array_accessors_in_both_modes() {
    // Arrange
    let sql = "SELECT jsonb_path_query_array('{\"a\":[1,2]}'::JSONB, 'strict $.a[*]'), jsonb_path_query_array('{\"a\":1}'::JSONB, 'lax $.a[*]')";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("type-jsonpath-strict-lax-array-access", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("JSONPath result RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![3802, 3802]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("[1, 2]".to_owned()), Some("[1]".to_owned())]]
    );
}

#[test]
fn should_apply_jsonpath_filter_variables_to_array_members() {
    // Arrange
    let sql = "SELECT jsonb_path_exists('{\"a\":[1,2,3]}'::JSONB, '$.a[*] ? (@ > $min)', '{\"min\":2}'::JSONB)";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("issue-809-jsonpath-filter-variable", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_eq!(wire::data_rows(frames), vec![vec![Some("t".to_owned())]]);
}

#[test]
fn should_evaluate_jsonpath_match_predicates() {
    // Arrange
    let sql = "SELECT jsonb_path_match('{\"a\":2}'::JSONB, '$.a == 2')";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("issue-809-jsonpath-match-predicate", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (16, 1, -1), 0, true);
    assert_eq!(
        fixture::data_row_payloads(frames),
        vec![fixture::one_field_row(b"t")]
    );
}

#[test]
fn should_insert_jsonb_values_at_array_paths() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_810_jsonb_insert_values (document JSONB, path TEXT[], new_value JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_810_jsonb_insert_values VALUES ($1, $2, $3)",
            vec![
                Value::Json(serde_json::json!([1, 3])),
                Value::Json(serde_json::json!(["1"])),
                Value::Json(serde_json::json!(2)),
            ],
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT jsonb_insert(document, path, new_value) FROM issue_810_jsonb_insert_values",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-810-jsonb-insert-array-path", &setup, vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (3802, -1, -1), 0, true);
    assert_eq!(
        fixture::data_row_payloads(frames),
        vec![fixture::one_field_row(b"[1, 2, 3]")]
    );
}

#[test]
fn should_preserve_ordinary_json_scalar_predicate_carriers() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE ordinary_json_number_controls (k INT, doc JSON)",
            Vec::new(),
        ),
        (
            "INSERT INTO ordinary_json_number_controls (k, doc) VALUES (1, $1), (2, $2), (3, $3)",
            vec![
                Value::Json(
                    serde_json::from_str("9007199254740993").expect("exact signed JSON number"),
                ),
                Value::Json(serde_json::from_str("3.5").expect("floating JSON document")),
                Value::Json(serde_json::from_str("false").expect("Boolean JSON document")),
            ],
        ),
    ];
    let cases = [
        (
            "SELECT doc FROM ordinary_json_number_controls WHERE CAST(doc AS BIGINT) = 9007199254740993",
            b"9007199254740993".as_slice(),
        ),
        (
            "SELECT doc FROM ordinary_json_number_controls WHERE CAST(doc AS FLOAT) = 3.5",
            b"3.5".as_slice(),
        ),
        (
            "SELECT doc FROM ordinary_json_number_controls WHERE CAST(doc AS BOOLEAN) = FALSE",
            b"false".as_slice(),
        ),
    ];
    let cycles = cases
        .into_iter()
        .flat_map(|(sql, _)| [0, 1].map(|format| fixture::execute_cycle(sql, None, format, true)))
        .collect();

    // Act
    let batches = fixture::run_cycles("type-json-ordinary-scalar-predicates", &setup, cycles);

    // Assert
    assert_eq!(batches.len(), 6);
    for ((_, body), pair) in cases.into_iter().zip(batches.as_chunks::<2>().0) {
        for (format, frames) in [0, 1].into_iter().zip(pair) {
            fixture::assert_success(frames);
            fixture::assert_single_column_descriptors(frames, (114, -1, -1), format, true);
            assert_eq!(
                fixture::data_row_payloads(frames),
                vec![fixture::one_field_row(body)]
            );
        }
    }
}

#[test]
fn should_apply_jsonb_containment_with_boolean_result_identity() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE jsonb_containment_input (document JSONB, subset JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO jsonb_containment_input VALUES ('{\"a\":1,\"b\":[1,2]}', '{\"a\":1}')",
            Vec::new(),
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT document @> subset AS contains FROM jsonb_containment_input",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles(
        "type-jsonb-containment-result-identity",
        &setup,
        vec![cycle],
    );

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (16, 1, -1), 0, true);
    assert_eq!(wire::data_rows(frames), vec![vec![Some("t".to_owned())]]);
}

#[test]
fn should_replace_a_jsonb_object_field_without_changing_document_type() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT jsonb_set('{\"a\":1}'::JSONB, $1, jsonb_build_array(2))",
        Some((1009, 0, Some(&b"{a}"[..]))),
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-809-jsonb-set-object-field", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (3802, -1, -1), 0, true);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("{\"a\": [2]}".to_owned())]]
    );
}

#[test]
fn should_match_any_jsonb_object_key_from_a_text_array() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_809_jsonb_any_keys (document JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_809_jsonb_any_keys VALUES ('{\"a\":1,\"b\":2}')",
            Vec::new(),
        ),
    ];
    let sql = "SELECT document?|'{b,missing}', document ?| '{missing}', \
        document ?| '{}'::TEXT[], document ?| NULL::TEXT[], \
        document ?| '{b}'::TEXT[], document ?| '{NULL,b}'::TEXT[], \
        document ?| '{NULL}'::TEXT[] FROM issue_809_jsonb_any_keys";
    let cycle = fixture::execute_cycle(sql, None, 0, true);
    let parameter = fixture::execute_cycle(
        "SELECT '{\"a\":1,\"b\":2}'::JSONB ?| $1",
        Some((0, 0, Some(b"{b,missing}"))),
        0,
        true,
    );
    let invalid_operand = fixture::execute_cycle(
        "SELECT '{\"a\":1}'::JSONB ?| TRUE",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles(
        "issue-809-jsonb-any-key-array",
        &setup,
        vec![cycle, parameter, invalid_operand],
    );

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_eq!(
        wire::parse_row_description(
            &frames
                .iter()
                .find(|(tag, _)| *tag == b'T')
                .expect("JSONB any-key RowDescription")
                .1
        )
        .iter()
        .map(|column| column.type_oid)
        .collect::<Vec<_>>(),
        vec![16, 16, 16, 16, 16, 16, 16]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![
            Some("t".to_owned()),
            Some("f".to_owned()),
            Some("f".to_owned()),
            None,
            Some("t".to_owned()),
            Some("t".to_owned()),
            Some("f".to_owned())
        ]]
    );
    let parameter_frames = &batches[1];
    fixture::assert_success(parameter_frames);
    let parameter_types = parameter_frames
        .iter()
        .find(|(tag, _)| *tag == b't')
        .expect("text-array operator ParameterDescription");
    assert_eq!(wire::parse_parameter_description(&parameter_types.1), [1009]);
    assert_eq!(wire::data_rows(parameter_frames), vec![vec![Some("t".to_owned())]]);
    assert_eq!(wire::error_code(&batches[2]).as_deref(), Some("42883"));
}

#[test]
fn should_require_every_jsonb_object_key_from_a_text_array() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_809_jsonb_all_keys (document JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_809_jsonb_all_keys VALUES ('{\"a\":1,\"b\":2}')",
            Vec::new(),
        ),
    ];
    let sql = "SELECT document?&'{a,b}', document ?& '{a,missing}', \
        document ?& '{}'::TEXT[], document ?& NULL::TEXT[], \
        document ?& '{NULL,a,b}'::TEXT[], document ?& '{NULL}'::TEXT[] \
        FROM issue_809_jsonb_all_keys";
    let cycle = fixture::execute_cycle(sql, None, 0, true);
    let parameter = fixture::execute_cycle(
        "SELECT '{\"a\":1,\"b\":2}'::JSONB ?& $1",
        Some((0, 0, Some(b"{a,b}"))),
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles(
        "issue-809-jsonb-all-key-array",
        &setup,
        vec![cycle, parameter],
    );

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![
            Some("t".to_owned()),
            Some("f".to_owned()),
            Some("t".to_owned()),
            None,
            Some("t".to_owned()),
            Some("t".to_owned())
        ]]
    );
    let parameter_frames = &batches[1];
    fixture::assert_success(parameter_frames);
    let parameter_types = parameter_frames
        .iter()
        .find(|(tag, _)| *tag == b't')
        .expect("text-array operator ParameterDescription");
    assert_eq!(wire::parse_parameter_description(&parameter_types.1), [1009]);
    assert_eq!(wire::data_rows(parameter_frames), vec![vec![Some("t".to_owned())]]);
}

