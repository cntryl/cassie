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

#[test]
fn should_delete_a_jsonb_object_key_with_the_minus_operator() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_809_jsonb_delete_values (id INTEGER, document JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_809_jsonb_delete_values VALUES (1, '{\"a\":1,\"b\":2}')",
            Vec::new(),
        ),
    ];
    let sql = "SELECT (document - 'a')::TEXT, (document - 'missing')::TEXT \
        FROM issue_809_jsonb_delete_values WHERE id = 1";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("issue-809-jsonb-delete-object-key", &setup, vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![
            Some("{\"b\": 2}".to_owned()),
            Some("{\"a\": 1, \"b\": 2}".to_owned())
        ]]
    );
}

#[test]
fn should_delete_matching_jsonb_array_strings_with_the_minus_operator() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_809_jsonb_delete_array_values (document JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_809_jsonb_delete_array_values VALUES ('[\"a\",\"b\",\"a\",2]')",
            Vec::new(),
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT (document - 'a')::TEXT, (document - 'missing')::TEXT \
        FROM issue_809_jsonb_delete_array_values",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-809-jsonb-delete-array-values", &setup, vec![cycle]);

    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![
            Some("[\"b\", 2]".to_owned()),
            Some("[\"a\", \"b\", \"a\", 2]".to_owned())
        ]]
    );
}

#[test]
fn should_infer_text_for_jsonb_deletion_parameters() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_809_jsonb_delete_parameter_values (document JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_809_jsonb_delete_parameter_values VALUES ('{\"a\":1,\"b\":2}')",
            Vec::new(),
        ),
    ];
    let parameter = fixture::execute_cycle(
        "SELECT (document - $1)::TEXT FROM issue_809_jsonb_delete_parameter_values",
        Some((0, 0, Some(b"a"))),
        0,
        true,
    );
    // Act
    let batches = fixture::run_cycles(
        "issue-809-jsonb-delete-parameter-values",
        &setup,
        vec![parameter],
    );

    // Assert
    let parameter_frames = &batches[0];
    fixture::assert_success(parameter_frames);
    let parameter_types = parameter_frames
        .iter()
        .find(|(tag, _)| *tag == b't')
        .expect("text key parameter description");
    assert_eq!(wire::parse_parameter_description(&parameter_types.1), [25]);
    assert_eq!(
        wire::data_rows(parameter_frames),
        vec![vec![Some("{\"b\": 2}".to_owned())]]
    );
}

#[test]
fn should_propagate_sql_null_for_jsonb_text_key_deletion() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT NULL::JSONB - NULL::TEXT",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-809-jsonb-delete-null", &[], vec![cycle]);

    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(wire::data_rows(&batches[0]), vec![vec![None]]);
}

#[test]
fn should_reject_jsonb_deletion_with_unsupported_operand_types() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_809_jsonb_delete_overload_values (document JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_809_jsonb_delete_overload_values VALUES ('{\"a\":1}')",
            Vec::new(),
        ),
    ];
    let invalid_boolean = fixture::execute_cycle(
        "SELECT document - TRUE FROM issue_809_jsonb_delete_overload_values",
        None,
        0,
        true,
    );
    let unsupported_bigint = fixture::execute_cycle(
        "SELECT document - 1::BIGINT FROM issue_809_jsonb_delete_overload_values",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles(
        "issue-809-jsonb-delete-overload-values",
        &setup,
        vec![invalid_boolean, unsupported_bigint],
    );

    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("42883"));
    assert_eq!(wire::error_code(&batches[1]).as_deref(), Some("42883"));
}

#[test]
fn should_reject_json_scalar_documents_for_text_key_deletion() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_809_jsonb_delete_scalar_values (document JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_809_jsonb_delete_scalar_values VALUES ('4')",
            Vec::new(),
        ),
    ];
    let cast_scalar = fixture::execute_cycle("SELECT '4'::JSONB - 'a'", None, 0, true);
    let stored_scalar = fixture::execute_cycle(
        "SELECT document - 'a' FROM issue_809_jsonb_delete_scalar_values",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles(
        "issue-809-jsonb-delete-scalar-values",
        &setup,
        vec![cast_scalar, stored_scalar],
    );

    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("22023"));
    assert_eq!(wire::error_code(&batches[1]).as_deref(), Some("22023"));
}

#[test]
fn should_preserve_jsonb_object_key_order_after_text_key_deletion() {
    // Arrange
    let nested = fixture::execute_cycle(
        "SELECT ('{\"z\":1,\"aa\":2,\"a\":3,\"x\":{\"n\":9007199254740993,\
            \"v\":[1.00,1e2]}}'::JSONB - 'a')::TEXT",
        None,
        0,
        true,
    );
    // Act
    let batches = fixture::run_cycles("issue-809-jsonb-delete-ordering", &[], vec![nested]);

    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![Some(
            "{\"x\": {\"n\": 9007199254740993, \"v\": [1.00, 100]}, \"z\": 1, \"aa\": 2}"
                .to_owned()
        )]]
    );

}

#[test]
fn should_preserve_exact_nested_numbers_after_text_key_deletion() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT ('{\"keep\":{\"n\":9007199254740993,\"v\":[1.00,1e2]},\"drop\":1}'::JSONB - 'drop')::TEXT",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-809-jsonb-delete-numbers", &[], vec![cycle]);

    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![Some(
            "{\"keep\": {\"n\": 9007199254740993, \"v\": [1.00, 100]}}".to_owned()
        )]]
    );
}

#[test]
fn should_preserve_jsonb_result_type_after_text_key_deletion() {
    // Arrange
    let cycle = fixture::execute_cycle("SELECT '{\"a\":1,\"b\":2}'::JSONB - 'a'", None, 0, true);

    // Act
    let batches = fixture::run_cycles("issue-809-jsonb-delete-result-type", &[], vec![cycle]);

    // Assert
    fixture::assert_success(&batches[0]);
    fixture::assert_single_column_descriptors(&batches[0], (3802, -1, -1), 0, true);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![Some("{\"b\": 2}".to_owned())]]
    );
}

#[test]
fn should_count_elements_in_a_jsonb_array() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_810_jsonb_length_values (document JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_810_jsonb_length_values VALUES ('[2,4]')",
            Vec::new(),
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT jsonb_array_length(document) FROM issue_810_jsonb_length_values",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-810-jsonb-array-length", &setup, vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (23, 4, -1), 0, true);
    assert_eq!(wire::data_rows(frames), vec![vec![Some("2".to_owned())]]);
}

#[test]
fn should_detect_any_jsonb_key_from_a_text_array_function() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_810_jsonb_exists_values (document JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_810_jsonb_exists_values VALUES ('{\"a\":1}')",
            Vec::new(),
        ),
    ];
    let sql = "SELECT jsonb_exists_any(document, '{a,b}') FROM issue_810_jsonb_exists_values";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles(
        "issue-810-jsonb-exists-array-functions",
        &setup,
        vec![cycle],
    );

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_eq!(wire::data_rows(frames), vec![vec![Some("t".to_owned())]]);
}

#[test]
fn should_require_all_jsonb_keys_from_a_text_array_function() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_810_jsonb_exists_all_values (document JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_810_jsonb_exists_all_values VALUES ('{\"a\":1}')",
            Vec::new(),
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT jsonb_exists_all(document, '{a,b}') FROM issue_810_jsonb_exists_all_values",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles(
        "issue-810-jsonb-exists-all-array-function",
        &setup,
        vec![cycle],
    );

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_eq!(wire::data_rows(frames), vec![vec![Some("f".to_owned())]]);
}

#[test]
fn should_return_jsonb_text_with_the_arrow_text_operator() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_809_arrow_text_values (document JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_809_arrow_text_values VALUES ('{\"label\":\"value\"}')",
            Vec::new(),
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT document ->> 'label' FROM issue_809_arrow_text_values",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-809-jsonb-arrow-text", &setup, vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (25, -1, -1), 0, true);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("value".to_owned())]]
    );
}

#[test]
fn should_extract_nested_jsonb_values_with_the_path_operator() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_809_path_values (document JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_809_path_values VALUES ('{\"nested\":{\"x\":7}}')",
            Vec::new(),
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT document #> '{nested,x}' FROM issue_809_path_values",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-809-jsonb-path", &setup, vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (3802, -1, -1), 0, true);
    assert_eq!(wire::data_rows(frames), vec![vec![Some("7".to_owned())]]);
}

#[test]
fn should_extract_nested_jsonb_scalars_as_text_with_the_path_text_operator() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_809_path_text_values (document JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_809_path_text_values VALUES ('{\"nested\":{\"x\":7}}')",
            Vec::new(),
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT document #>> '{nested,x}' FROM issue_809_path_text_values",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-809-jsonb-path-text", &setup, vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (25, -1, -1), 0, true);
    assert_eq!(wire::data_rows(frames), vec![vec![Some("7".to_owned())]]);
}

#[test]
fn should_detect_jsonb_object_key_presence_without_other_operators() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_809_key_presence_values (document JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_809_key_presence_values VALUES ('{\"present\":null}')",
            Vec::new(),
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT document?'present', document ? 'missing' FROM issue_809_key_presence_values",
        None,
        0,
        true,
    );
    let parameter = fixture::execute_cycle(
        "SELECT '{\"present\":null}'::JSONB ? $1",
        Some((0, 0, Some(b"present"))),
        0,
        true,
    );
    let invalid_operand = fixture::execute_cycle(
        "SELECT document ? TRUE FROM issue_809_key_presence_values",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles(
        "issue-809-jsonb-key-presence",
        &setup,
        vec![cycle, parameter, invalid_operand],
    );

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("t".to_owned()), Some("f".to_owned())]]
    );
    let parameter_frames = &batches[1];
    fixture::assert_success(parameter_frames);
    let parameter_types = parameter_frames
        .iter()
        .find(|(tag, _)| *tag == b't')
        .expect("text operator ParameterDescription");
    assert_eq!(wire::parse_parameter_description(&parameter_types.1), [25]);
    assert_eq!(wire::data_rows(parameter_frames), vec![vec![Some("t".to_owned())]]);
    assert_eq!(wire::error_code(&batches[2]).as_deref(), Some("42883"));
}

#[test]
fn should_return_the_jsonb_type_name_for_a_stored_document() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_810_typeof_values (document JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_810_typeof_values VALUES ('{\"a\":1}')",
            Vec::new(),
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT jsonb_typeof(document) FROM issue_810_typeof_values",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-810-jsonb-typeof", &setup, vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (25, -1, -1), 0, true);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("object".to_owned())]]
    );
}

#[test]
fn should_pretty_print_jsonb_objects_with_nested_arrays() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_810_pretty_values (document JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_810_pretty_values VALUES ('{\"a\":1,\"b\":[2,3]}')",
            Vec::new(),
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT jsonb_pretty(document) FROM issue_810_pretty_values",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-810-jsonb-pretty", &setup, vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some(
            "{\n    \"a\": 1,\n    \"b\": [\n        2,\n        3\n    ]\n}".to_owned()
        )]]
    );
}

#[test]
fn should_apply_the_jsonb_path_exists_operator() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_809_path_exists_operator_values (document JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_809_path_exists_operator_values VALUES ('{\"a\":1}')",
            Vec::new(),
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT document @? '$.a' FROM issue_809_path_exists_operator_values",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-809-jsonb-path-exists-operator", &setup, vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_eq!(wire::data_rows(frames), vec![vec![Some("t".to_owned())]]);
}

#[test]
fn should_apply_the_jsonb_path_match_operator() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_809_path_match_operator_values (document JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_809_path_match_operator_values VALUES ('{\"a\":1}')",
            Vec::new(),
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT document @@ '$.a == 1' FROM issue_809_path_match_operator_values",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-809-jsonb-path-match-operator", &setup, vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_eq!(wire::data_rows(frames), vec![vec![Some("t".to_owned())]]);
}

#[test]
fn should_subscript_a_jsonb_object_by_key() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_809_subscript_values (document JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_809_subscript_values VALUES ('{\"a\":1}')",
            Vec::new(),
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT document['a'] FROM issue_809_subscript_values",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-809-jsonb-key-subscript", &setup, vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (3802, -1, -1), 0, true);
    assert_eq!(wire::data_rows(frames), vec![vec![Some("1".to_owned())]]);
}

#[test]
fn should_extract_jsonb_path_text_with_the_path_function() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_810_extract_path_text_values (document JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_810_extract_path_text_values VALUES ('{\"nested\":{\"x\":7}}')",
            Vec::new(),
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT jsonb_extract_path_text(document, 'nested', 'x') FROM issue_810_extract_path_text_values",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-810-jsonb-extract-path-text", &setup, vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (25, -1, -1), 0, true);
    assert_eq!(wire::data_rows(frames), vec![vec![Some("7".to_owned())]]);
}

#[test]
fn should_replace_a_jsonb_value_at_a_path_with_jsonb_set() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_810_jsonb_set_values (document JSONB, path TEXT[], replacement JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_810_jsonb_set_values VALUES ($1, $2, $3)",
            vec![
                Value::Json(serde_json::json!({"a": 1, "b": 1})),
                Value::Json(serde_json::json!(["a"])),
                Value::Json(serde_json::json!(2)),
            ],
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT jsonb_set(document, path, replacement)::TEXT FROM issue_810_jsonb_set_values",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-810-jsonb-set-path", &setup, vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("{\"a\": 2, \"b\": 1}".to_owned())]]
    );
}

#[test]
fn should_apply_jsonpath_predicate_operators_with_boolean_wire_types() {
    // Arrange
    let sql = "SELECT payload @? '$.items[*] ? (@.qty >= 2)'::JSONPATH, payload @@ '$.active == true'::JSONPATH FROM (VALUES ('{\"active\":true,\"items\":[{\"qty\":1},{\"qty\":2}]}'::JSONB)) AS input(payload)";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("issue-810-jsonpath-predicate-operators", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("JSONPath predicate operator RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![16, 16]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("t".to_owned()), Some("t".to_owned())]]
    );
}
