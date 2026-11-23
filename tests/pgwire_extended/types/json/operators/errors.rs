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
fn should_reject_json_equality_and_report_invalid_jsonb_comparands() {
    // Arrange
    let corpus: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../corpus/postgresql18/type-oracles.json"
    ))
    .expect("PostgreSQL type oracle corpus");
    let corpus_case = corpus["cases"]
        .as_array()
        .expect("type oracle cases")
        .iter()
        .find(|case| case["id"] == "json_equality_error")
        .expect("json_equality_error corpus entry");
    let corpus_sql = corpus_case["sql"]
        .as_str()
        .expect("corpus SQL text");
    let corpus_sqlstate = corpus_case["paths"]["simple_text"][0]["sqlstate"]
        .as_str()
        .expect("corpus SQLSTATE");
    let corpus_cycle = fixture::execute_cycle(corpus_sql, None, 0, true);
    let setup = [
        (
            "CREATE TABLE issue_1469_json_comparison (document JSON)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_1469_json_comparison VALUES ('{}')",
            Vec::new(),
        ),
        (
            "CREATE TABLE issue_1471_jsonb_comparison (document JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_1471_jsonb_comparison VALUES ('{\"a\":1}')",
            Vec::new(),
        ),
    ];
    let cases = [
        (
            "SELECT '{}'::JSON = '{}'::JSON",
            "42883",
            "operator does not exist: json = json",
        ),
        (
            "SELECT '{\"a\":1}'::JSON = 'abc'",
            "42883",
            "operator does not exist: json = unknown",
        ),
        (
            "SELECT * FROM issue_1469_json_comparison WHERE document = '{}'::JSON",
            "42883",
            "operator does not exist: json = json",
        ),
        (
            "SELECT * FROM issue_1471_jsonb_comparison WHERE document = 'abc'",
            "22P02",
            "invalid input syntax for type json",
        ),
        (
            "SELECT '{\"a\":1}'::JSONB <> 'abc'",
            "22P02",
            "invalid input syntax for type json",
        ),
        (
            "SELECT 'abc' = '{\"a\":1}'::JSONB",
            "22P02",
            "invalid input syntax for type json",
        ),
    ];
    let cycles = cases
        .iter()
        .map(|(sql, _, _)| fixture::execute_cycle(sql, None, 0, true))
        .collect();
    let controls = [
        "SELECT '{\"a\":1,\"a\":2}'::JSONB = '{\"a\":2}'::JSONB",
        "SELECT '[1]'::JSONB = '[1.0]'::JSONB",
        "SELECT '{\"a\":1}'::JSONB = '{\"a\":1}'",
    ]
    .map(|sql| fixture::execute_cycle(sql, None, 0, true));

    // Act
    let failures = fixture::run_cycles("issues-1469-1471-json-comparison-errors", &setup, cycles);
    let corpus_result = fixture::run_cycles("issue-1469-json-equality-type-oracle", &[], vec![corpus_cycle]);
    let successes =
        fixture::run_cycles("issue-1471-jsonb-comparison-control", &[], controls.into());

    // Assert
    assert_eq!(failures.len(), cases.len());
    assert_eq!(
        wire::error_code(&corpus_result[0]).as_deref(),
        Some(corpus_sqlstate),
        "{corpus_sql}"
    );
    for (frames, (sql, state, fragment)) in failures.iter().zip(cases) {
        assert_eq!(wire::error_code(frames).as_deref(), Some(state), "{sql}");
        let message = frames
            .iter()
            .find(|(tag, _)| *tag == b'E')
            .and_then(|(_, payload)| {
                wire::parse_error_fields(payload)
                    .into_iter()
                    .find(|(kind, _)| *kind == 'M')
                    .map(|(_, message)| message)
            })
            .unwrap_or_default();
        assert!(message.contains(fragment), "{sql}: {message}");
        assert!(wire::data_rows(frames).is_empty(), "{sql}");
    }
    for frames in &successes {
        fixture::assert_success(frames);
        assert_eq!(wire::data_rows(frames), vec![vec![Some("t".to_owned())]]);
    }
}
