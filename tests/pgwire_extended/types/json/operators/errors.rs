
#[test]
fn should_distinguish_lax_singleton_wrap_from_strict_jsonpath_error() {
    // Arrange
    let lax = fixture::execute_cycle(
        "SELECT jsonb_path_query_array('{\"a\":1}'::JSONB, 'lax $.a[*]')",
        None,
        0,
        true,
    );
    let strict = fixture::execute_cycle(
        "SELECT jsonb_path_query_array('{\"a\":1}'::JSONB, 'strict $.a[*]')",
        None,
        0,
        true,
    );
    let recovery = fixture::execute_cycle("SELECT 42", None, 0, true);

    // Act
    let batches = fixture::run_cycles(
        "issue-809-jsonpath-strict-lax-singleton",
        &[],
        vec![lax, strict, recovery],
    );

    // Assert
    fixture::assert_success(&batches[0]);
    fixture::assert_single_column_descriptors(&batches[0], (3_802, -1, -1), 0, true);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![Some("[1]".to_owned())]]
    );

    let frames = &batches[1];
    assert_eq!(wire::error_code(frames).as_deref(), Some("22039"));
    let errors = frames
        .iter()
        .filter(|(tag, _)| *tag == b'E')
        .collect::<Vec<_>>();
    assert_eq!(errors.len(), 1);
    let message = wire::parse_error_fields(&errors[0].1)
        .into_iter()
        .find_map(|(key, value)| (key == 'M').then_some(value))
        .expect("strict JSONPath error message");
    assert_eq!(
        message,
        "jsonpath wildcard array accessor can only be applied to an array"
    );
    assert_eq!(wire::data_rows(frames), Vec::<Vec<Option<String>>>::new());

    fixture::assert_success(&batches[2]);
    assert_eq!(
        wire::data_rows(&batches[2]),
        vec![vec![Some("42".to_owned())]]
    );
}

#[test]
fn should_suppress_jsonpath_structural_errors_only_for_silent_forms() {
    // Arrange
    let unsuppressed = fixture::execute_cycle(
        "SELECT jsonb_path_exists('{\"a\":1}'::JSONB, 'strict $.a[*]')",
        None,
        0,
        true,
    );
    let suppressed = fixture::execute_cycle(
        "SELECT jsonb_path_exists('{\"a\":1}'::JSONB, 'strict $.a[*]', '{}'::JSONB, true), '{\"a\":1}'::JSONB @? 'strict $.a[*]'",
        None,
        0,
        true,
    );
    let recovery = fixture::execute_cycle("SELECT 42", None, 0, true);

    // Act
    let batches = fixture::run_cycles(
        "issue-809-jsonpath-silent-structural-errors",
        &[],
        vec![unsuppressed, suppressed, recovery],
    );

    // Assert
    let frames = &batches[0];
    assert_eq!(wire::error_code(frames).as_deref(), Some("22039"));
    let errors = frames
        .iter()
        .filter(|(tag, _)| *tag == b'E')
        .collect::<Vec<_>>();
    assert_eq!(errors.len(), 1);
    let message = wire::parse_error_fields(&errors[0].1)
        .into_iter()
        .find_map(|(key, value)| (key == 'M').then_some(value))
        .expect("unsuppressed JSONPath structural error message");
    assert_eq!(
        message,
        "jsonpath wildcard array accessor can only be applied to an array"
    );
    assert_eq!(wire::data_rows(frames), Vec::<Vec<Option<String>>>::new());

    let frames = &batches[1];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("silent JSONPath forms RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![16, 16]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("f".to_owned()), Some("f".to_owned())]]
    );

    fixture::assert_success(&batches[2]);
    assert_eq!(
        wire::data_rows(&batches[2]),
        vec![vec![Some("42".to_owned())]]
    );
}

#[test]
fn should_reject_json_comparison_signatures() {
    // Arrange
    let json_equality = fixture::execute_cycle(
        "SELECT '{}'::JSON = '{}'::JSON",
        None,
        0,
        true,
    );
    let json_literal = fixture::execute_cycle("SELECT '{}'::JSON = '{}'", None, 0, true);
    let json_text = fixture::execute_cycle("SELECT '{}'::JSON = '{}'::TEXT", None, 0, true);
    let json_distinct = fixture::execute_cycle(
        "SELECT '{}'::JSON IS DISTINCT FROM '{}'::JSON",
        None,
        0,
        true,
    );
    let mixed = fixture::execute_cycle("SELECT '{}'::JSONB = '{}'::JSON", None, 0, true);

    // Act
    let batches = fixture::run_cycles(
        "issue-1469-json-comparison-signatures",
        &[],
        vec![json_equality, json_literal, json_text, json_distinct, mixed],
    );

    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("42883"));
    assert_eq!(wire::error_code(&batches[1]).as_deref(), Some("42883"));
    let message = batches[1]
        .iter()
        .find(|(tag, _)| *tag == b'E')
        .and_then(|(_, payload)| {
            wire::parse_error_fields(payload)
                .into_iter()
                .find_map(|(key, value)| (key == 'M').then_some(value))
        })
        .expect("json = unknown error message");
    assert_eq!(message, "operator does not exist: json = unknown");
    assert_eq!(wire::error_code(&batches[2]).as_deref(), Some("42883"));
    let message = batches[2]
        .iter()
        .find(|(tag, _)| *tag == b'E')
        .and_then(|(_, payload)| {
            wire::parse_error_fields(payload)
                .into_iter()
                .find_map(|(key, value)| (key == 'M').then_some(value))
        })
        .expect("json = text error message");
    assert_eq!(message, "operator does not exist: json = text");
    assert_eq!(wire::error_code(&batches[3]).as_deref(), Some("42883"));
    assert_eq!(wire::error_code(&batches[4]).as_deref(), Some("42883"));
}

#[test]
fn should_keep_jsonb_equality_available() {
    // Arrange
    let cycle = fixture::execute_cycle("SELECT '{}'::JSONB = '{}'::JSONB", None, 0, true);

    // Act
    let batches = fixture::run_cycles("issue-1469-valid-jsonb-equality", &[], vec![cycle]);

    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(wire::data_rows(&batches[0]), vec![vec![Some("t".to_owned())]]);
}

#[test]
fn should_report_invalid_jsonb_comparison_literals_with_json_input_sqlstate() {
    // Arrange
    let invalid_left = fixture::execute_cycle("SELECT 'bad' = '{}'::JSONB", None, 0, true);
    let invalid_right = fixture::execute_cycle("SELECT '{}'::JSONB = 'bad'", None, 0, true);

    // Act
    let batches = fixture::run_cycles(
        "issue-1471-invalid-jsonb-comparison-literals",
        &[],
        vec![invalid_left, invalid_right],
    );

    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("22P02"));
    assert_eq!(wire::error_code(&batches[1]).as_deref(), Some("22P02"));
}
