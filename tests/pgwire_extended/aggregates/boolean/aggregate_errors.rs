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
