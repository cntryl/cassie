#[test]
fn should_reject_non_boolean_boolean_aggregate_arguments_with_42883() {
    // Arrange
    let batches = run_script(
        "issue-805-boolean-aggregate-argument-type",
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT BOOL_AND(flag) FROM (VALUES (TRUE), (NULL::BOOLEAN)) AS input(flag)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame("SELECT BOOL_AND(1)")]),
            Step::ThroughReady(vec![wire::simple_query_frame("SELECT 45 AS recovery")]),
        ],
    );

    // Act
    let valid = &batches[0];
    let invalid = &batches[1];
    let recovery = &batches[2];

    // Assert
    assert_eq!(wire::error_code(valid), None);
    let description = valid
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("RowDescription for Boolean aggregate");
    assert_eq!(wire::parse_row_description(&description.1)[0].type_oid, 16);
    assert_eq!(wire::data_rows(valid), vec![vec![Some("t".to_owned())]]);
    assert_eq!(wire::error_code(invalid).as_deref(), Some("42883"));
    let fields = invalid
        .iter()
        .find(|(tag, _)| *tag == b'E')
        .map(|(_, payload)| wire::parse_error_fields(payload))
        .expect("undefined Boolean aggregate overload ErrorResponse");
    assert!(fields.iter().any(|(kind, message)| {
        *kind == 'M' && message.starts_with("function bool_and(integer) does not exist")
    }));
    assert_eq!(wire::data_rows(invalid), Vec::<Vec<Option<String>>>::new());
    assert_eq!(wire::error_code(recovery), None);
    assert_eq!(wire::data_rows(recovery), vec![vec![Some("45".to_owned())]]);
}

#[test]
fn should_reject_invalid_boolean_calls_given_wrong_types_literals_arity_and_star_when_parse_flush_is_sent(
) {
    // Arrange
    let cases = [
        ("SELECT BOOL_AND(1)", vec![], "42883"),
        ("SELECT BOOL_OR(CAST(NULL AS TEXT))", vec![], "42883"),
        ("SELECT EVERY($1)", vec![23], "42883"),
        ("SELECT BOOL_AND()", vec![], "42883"),
        ("SELECT BOOL_AND(*)", vec![], "42883"),
        ("SELECT BOOL_OR(TRUE, FALSE)", vec![], "42883"),
        ("SELECT EVERY('bad')", vec![], "22P02"),
    ];
    let mut steps = Vec::new();
    for (sql, supplied, _) in &cases {
        steps.push(Step::ParseFlush(wire::parse_frame_with_types(
            "", sql, supplied,
        )));
        steps.push(Step::ThroughReady(vec![wire::sync_frame()]));
        steps.push(Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT 42",
        )]));
    }

    // Act
    let batches = run_script("issue-805-boolean-parse-errors", steps);

    // Assert
    for ((sql, _, expected), batches) in cases.iter().zip(batches.as_chunks::<3>().0) {
        assert_eq!(
            wire::error_code(&batches[0]).as_deref(),
            Some(*expected),
            "{sql}"
        );
        assert!(!batches[0].iter().any(|(tag, _)| *tag == b'1'));
        assert_eq!(wire::data_rows(&batches[0]), Vec::<Vec<Option<String>>>::new());
        assert_eq!(batches[1].last().map(|frame| frame.0), Some(b'Z'));
        assert_eq!(wire::error_code(&batches[2]), None);
        assert_eq!(
            wire::data_rows(&batches[2]),
            vec![vec![Some("42".to_owned())]]
        );
    }
}

#[test]
fn should_reject_invalid_boolean_calls_given_simple_queries_when_no_partial_result_is_published() {
    // Arrange
    let cases = [
        ("SELECT BOOL_AND(1)", "42883"),
        ("SELECT BOOL_OR(CAST(NULL AS TEXT))", "42883"),
        ("SELECT BOOL_AND()", "42883"),
        ("SELECT BOOL_AND(*)", "42883"),
        ("SELECT BOOL_OR(TRUE, FALSE)", "42883"),
        ("SELECT EVERY('bad')", "22P02"),
    ];
    let mut steps = cases
        .iter()
        .map(|(sql, _)| Step::ThroughReady(vec![wire::simple_query_frame(sql)]))
        .collect::<Vec<_>>();
    steps.push(Step::ThroughReady(vec![wire::simple_query_frame(
        "SELECT 42",
    )]));

    // Act
    let batches = run_script("issue-805-boolean-simple-errors", steps);

    // Assert
    for ((sql, expected), frames) in cases.iter().zip(&batches) {
        assert_eq!(
            wire::error_code(frames).as_deref(),
            Some(*expected),
            "{sql}"
        );
        assert_eq!(wire::data_rows(frames), Vec::<Vec<Option<String>>>::new());
        assert!(!frames.iter().any(|(tag, _)| *tag == b'C'));
    }
    let recovery = batches.last().expect("recovery query");
    assert_eq!(wire::error_code(recovery), None);
    assert_eq!(wire::data_rows(recovery), vec![vec![Some("42".to_owned())]]);
}

#[test]
fn should_reject_typed_non_boolean_inputs_given_an_empty_table_when_simple_and_parse_queries_are_sent(
) {
    // Arrange
    let cases = [
        "SELECT BOOL_AND(label) FROM boolean_empty_typed_inputs",
        "SELECT BOOL_OR(amount) FROM boolean_empty_typed_inputs",
        "SELECT EVERY(label) FROM boolean_empty_typed_inputs WHERE FALSE",
        "SELECT BOOL_AND(amount) FROM boolean_empty_typed_inputs WHERE FALSE",
    ];
    let mut steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(
            "CREATE TABLE boolean_empty_typed_inputs (label TEXT, amount INT, flag BOOLEAN)",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT BOOL_AND(flag), BOOL_OR(flag), EVERY(flag) FROM boolean_empty_typed_inputs",
        )]),
    ];
    for sql in cases {
        steps.push(Step::ThroughReady(vec![wire::simple_query_frame(sql)]));
        steps.push(Step::ParseFlush(wire::parse_frame_with_types("", sql, &[])));
        steps.push(Step::ThroughReady(vec![wire::sync_frame()]));
        steps.push(Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT 42",
        )]));
    }

    // Act
    let batches = run_script("issue-805-empty-typed-boolean-input-errors", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    assert_eq!(wire::error_code(&batches[1]), None);
    assert_eq!(wire::data_rows(&batches[1]), vec![vec![None, None, None]]);
    let description = batches[1]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("empty Boolean aggregates retain result identity");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![16, 16, 16]
    );
    for (sql, phases) in cases.iter().zip(batches[2..].as_chunks::<4>().0) {
        for (index, error) in phases[..2].iter().enumerate() {
            assert_eq!(wire::error_code(error).as_deref(), Some("42883"), "{sql}");
            assert!(wire::data_rows(error).is_empty(), "{sql}");
            assert!(!error.iter().any(|(tag, _)| *tag == b'C'), "{sql}");
            if index == 1 {
                assert!(!error.iter().any(|(tag, _)| *tag == b'1'), "{sql}");
            }
        }
        assert_eq!(phases[2].last().map(|frame| frame.0), Some(b'Z'));
        assert_eq!(wire::error_code(&phases[3]), None);
        assert_eq!(
            wire::data_rows(&phases[3]),
            vec![vec![Some("42".to_owned())]]
        );
    }
}
