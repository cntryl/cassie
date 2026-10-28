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

