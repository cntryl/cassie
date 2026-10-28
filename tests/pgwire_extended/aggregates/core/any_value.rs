#[test]
fn should_preserve_any_value_result_oids_for_explicit_types_given_simple_query() {
    // Arrange
    let cases = [
        ("SELECT any_value('typed'::TEXT)", 25, Some("typed")),
        ("SELECT any_value(NULL::INT)", 23, None),
        ("SELECT any_value('[1,2]'::JSON)", 114, Some("[1,2]")),
        (
            "SELECT any_value(ARRAY['a','b']::TEXT[])",
            1009,
            Some("{a,b}"),
        ),
    ];
    let batches = run_script(
        "issue-1063-any-value-result-types",
        cases
            .iter()
            .map(|(sql, _, _)| Step::ThroughReady(vec![wire::simple_query_frame(sql)]))
            .collect(),
    );

    // Act
    let results = &batches;

    // Assert
    for ((sql, type_oid, value), result) in cases.iter().zip(results) {
        assert_eq!(wire::error_code(result), None, "{sql}");
        let description = result
            .iter()
            .find(|(tag, _)| *tag == b'T')
            .expect("RowDescription for ANY_VALUE input");
        assert_eq!(
            wire::parse_row_description(&description.1)[0].type_oid,
            *type_oid,
            "{sql} result type"
        );
        assert_eq!(
            wire::data_rows(result),
            vec![vec![value.map(str::to_owned)]],
            "{sql} result"
        );
    }
}

#[test]
fn should_reject_untyped_any_value_literals_given_simple_query_without_partial_results() {
    // Arrange
    let sql = ["SELECT any_value('x')", "SELECT any_value(NULL)"];
    let mut steps = sql
        .iter()
        .map(|query| Step::ThroughReady(vec![wire::simple_query_frame(query)]))
        .collect::<Vec<_>>();
    steps.push(Step::ThroughReady(vec![wire::simple_query_frame(
        "SELECT 42 AS recovery",
    )]));

    // Act
    let batches = run_script("issue-1063-any-value-unknown-literals", steps);

    // Assert
    for (query, rejected) in sql.iter().zip(&batches) {
        assert_eq!(wire::error_code(rejected).as_deref(), Some("42804"), "{query}");
        let fields = rejected
            .iter()
            .find(|(tag, _)| *tag == b'E')
            .map(|(_, payload)| wire::parse_error_fields(payload))
            .expect("polymorphic input ErrorResponse");
        assert!(fields.iter().any(|(kind, message)| {
            *kind == 'M'
                && message
                    == "could not determine polymorphic type because input has type unknown"
        }));
        assert_eq!(wire::data_rows(rejected), Vec::<Vec<Option<String>>>::new());
        assert!(!rejected.iter().any(|(tag, _)| *tag == b'C'));
    }
    assert_eq!(wire::error_code(&batches[2]), None);
    assert_eq!(
        wire::data_rows(&batches[2]),
        vec![vec![Some("42".to_owned())]]
    );
}

#[test]
fn should_reject_untyped_any_value_parameter_given_extended_parse() {
    // Arrange
    let statement = "issue_1063_any_value_untyped";
    let steps = vec![
        Step::ParseFlush(wire::parse_frame_with_types(
            statement,
            "SELECT any_value($1)",
            &[0],
        )),
        Step::ThroughReady(vec![wire::sync_frame()]),
    ];

    // Act
    let batches = run_script("issue-1063-any-value-untyped-parameter", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("42804"));
    let fields = batches[0]
        .iter()
        .find(|(tag, _)| *tag == b'E')
        .map(|(_, payload)| wire::parse_error_fields(payload))
        .expect("untyped polymorphic parameter ErrorResponse");
    assert!(fields.iter().any(|(kind, message)| {
        *kind == 'M'
            && message == "could not determine polymorphic type because input has type unknown"
    }));
    assert_eq!(batches[1].last().map(|frame| frame.0), Some(b'Z'));
}

#[test]
fn should_preserve_typed_any_value_parameter_identity() {
    // Arrange
    let statement = "issue_1063_any_value_typed";
    let portal = "issue_1063_any_value_typed_portal";
    let array_statement = "issue_1063_any_value_array";
    let array_portal = "issue_1063_any_value_array_portal";
    let steps = vec![
        Step::ParseFlush(wire::parse_frame_with_types(
            statement,
            "SELECT any_value($1)",
            &[23],
        )),
        Step::ThroughReady(vec![
            wire::describe_statement_frame(statement),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame(portal, statement, &["7"]),
            wire::describe_portal_frame(portal),
            wire::execute_frame(portal),
            wire::sync_frame(),
        ]),
        Step::ParseFlush(wire::parse_frame_with_types(
            array_statement,
            "SELECT any_value($1)",
            &[1007],
        )),
        Step::ThroughReady(vec![
            wire::describe_statement_frame(array_statement),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame(array_portal, array_statement, &["{1,2}"]),
            wire::describe_portal_frame(array_portal),
            wire::execute_frame(array_portal),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("issue-1063-any-value-parameter-identity", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);

    let described = &batches[1];
    assert_eq!(wire::error_code(described), None);
    let parameters = described
        .iter()
        .find(|(tag, _)| *tag == b't')
        .expect("ParameterDescription for explicitly typed input");
    assert_eq!(wire::parse_parameter_description(&parameters.1), vec![23]);
    let row_description = described
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("RowDescription for ANY_VALUE result");
    assert_eq!(wire::parse_row_description(&row_description.1)[0].type_oid, 23);

    let executed = &batches[2];
    assert_eq!(wire::error_code(executed), None);
    assert_eq!(wire::data_rows(executed), vec![vec![Some("7".to_owned())]]);

    let array_described = &batches[4];
    assert_eq!(wire::error_code(array_described), None);
    let array_parameters = array_described
        .iter()
        .find(|(tag, _)| *tag == b't')
        .expect("ParameterDescription for explicitly typed array input");
    assert_eq!(
        wire::parse_parameter_description(&array_parameters.1),
        vec![1007]
    );
    let array_row_description = array_described
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("RowDescription for ANY_VALUE array result");
    assert_eq!(
        wire::parse_row_description(&array_row_description.1)[0].type_oid,
        1007
    );
    let array_executed = &batches[5];
    assert_eq!(wire::error_code(array_executed), None);
    assert_eq!(
        wire::data_rows(array_executed),
        vec![vec![Some("{1,2}".to_owned())]]
    );
}
