#[test]
fn should_rollback_rule_definition_before_normal_insert_visibility() {
    // Arrange
    let batches = run_script(
        "issue-913-transactional-rule-visibility",
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame(
                "CREATE TABLE issue_913_rollback_source (id INTEGER)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "CREATE TABLE issue_913_rollback_target (id INTEGER)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame("BEGIN")]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "CREATE RULE issue_913_rollback_redirect AS \
                 ON INSERT TO issue_913_rollback_source \
                 DO INSTEAD INSERT INTO issue_913_rollback_target VALUES (NEW.id)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "INSERT INTO issue_913_rollback_source VALUES (10)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT (SELECT count(*) FROM issue_913_rollback_source), \
                        (SELECT count(*) FROM issue_913_rollback_target)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame("ROLLBACK")]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "INSERT INTO issue_913_rollback_source VALUES (11)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT (SELECT count(*) FROM issue_913_rollback_source), \
                        (SELECT count(*) FROM issue_913_rollback_target)",
            )]),
        ],
    );

    // Act
    let begin = &batches[2];
    let rule = &batches[3];
    let rewritten_insert = &batches[4];
    let inside_counts = &batches[5];
    let rollback = &batches[6];
    let normal_insert = &batches[7];
    let outside_counts = &batches[8];

    // Assert
    assert!(batches[..2]
        .iter()
        .all(|frames| wire::error_code(frames).is_none()));
    assert_eq!(wire::error_code(begin), None);
    assert_eq!(command_tags(begin), vec!["BEGIN"]);
    assert_eq!(
        begin.last().map(|frame| frame.1.as_slice()),
        Some(&b"T"[..])
    );
    assert_eq!(wire::error_code(rule), None);
    assert_eq!(command_tags(rule), vec!["CREATE RULE"]);
    assert_eq!(wire::error_code(rewritten_insert), None);
    assert_eq!(command_tags(rewritten_insert), vec!["INSERT 0 1"]);
    assert_eq!(
        rewritten_insert.last().map(|frame| frame.1.as_slice()),
        Some(&b"T"[..])
    );
    assert_eq!(wire::error_code(inside_counts), None);
    assert_eq!(
        wire::data_rows(inside_counts),
        vec![vec![Some("0".to_owned()), Some("1".to_owned())]]
    );
    assert_eq!(wire::error_code(rollback), None);
    assert_eq!(command_tags(rollback), vec!["ROLLBACK"]);
    assert_eq!(
        rollback.last().map(|frame| frame.1.as_slice()),
        Some(&b"I"[..])
    );
    assert_eq!(wire::error_code(normal_insert), None);
    assert_eq!(command_tags(normal_insert), vec!["INSERT 0 1"]);
    assert_eq!(wire::error_code(outside_counts), None);
    assert_eq!(
        wire::data_rows(outside_counts),
        vec![vec![Some("1".to_owned()), Some("0".to_owned())]]
    );
}
