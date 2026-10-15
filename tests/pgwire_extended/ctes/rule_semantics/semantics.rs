#[test]
fn should_detect_recursive_insert_rule_with_42p17_without_mutating_the_source() {
    // Arrange
    let batches = run_script(
        "issue-913-recursive-insert-rule",
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame(
                "CREATE TABLE issue_913_recursive_source (id INTEGER)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "CREATE RULE issue_913_recursive_insert AS \
                 ON INSERT TO issue_913_recursive_source \
                 DO ALSO INSERT INTO issue_913_recursive_source VALUES (NEW.id)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "INSERT INTO issue_913_recursive_source VALUES (1)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT count(*) FROM issue_913_recursive_source",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "DROP RULE issue_913_recursive_insert ON issue_913_recursive_source",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "INSERT INTO issue_913_recursive_source VALUES (2)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT id FROM issue_913_recursive_source ORDER BY id",
            )]),
        ],
    );

    // Act
    let setup = &batches[..2];
    let recursive_insert = &batches[2];
    let after_error = &batches[3];
    let recovery_insert = &batches[5];
    let recovered_rows = &batches[6];

    // Assert
    assert!(setup
        .iter()
        .all(|frames| wire::error_code(frames).is_none()));
    assert_eq!(wire::error_code(recursive_insert).as_deref(), Some("42P17"));
    let error = recursive_insert
        .iter()
        .find(|frame| frame.0 == b'E')
        .expect("recursive rewrite ErrorResponse");
    let fields = wire::parse_error_fields(&error.1);
    assert_eq!(
        fields
            .iter()
            .find(|field| field.0 == 'M')
            .map(|(_, message)| message.as_str()),
        Some("infinite recursion detected in rules for relation \"issue_913_recursive_source\"")
    );
    assert_eq!(
        wire::data_rows(recursive_insert),
        Vec::<Vec<Option<String>>>::new()
    );
    assert_eq!(command_tags(recursive_insert), Vec::<String>::new());
    assert_eq!(recursive_insert.last().map(|frame| frame.0), Some(b'Z'));

    assert_eq!(wire::error_code(after_error), None);
    assert_eq!(command_tags(after_error), vec!["SELECT 1"]);
    assert_eq!(
        wire::data_rows(after_error),
        vec![vec![Some("0".to_owned())]]
    );
    assert_eq!(wire::error_code(&batches[4]), None);
    assert_eq!(command_tags(&batches[4]), vec!["DROP RULE"]);
    assert_eq!(wire::error_code(recovery_insert), None);
    assert_eq!(command_tags(recovery_insert), vec!["INSERT 0 1"]);
    assert_eq!(wire::error_code(recovered_rows), None);
    assert_eq!(
        wire::data_rows(recovered_rows),
        vec![vec![Some("2".to_owned())]]
    );
}
