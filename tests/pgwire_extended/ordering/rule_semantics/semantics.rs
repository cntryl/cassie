#[test]
fn should_preserve_alphabetical_rule_action_order_with_insert_tag() {
    // Arrange
    let batches = run_script(
        "issue-913-multiple-rule-action-order-and-tag",
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame(
                "CREATE TABLE issue_913_order_source (id INTEGER)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "CREATE TABLE issue_913_order_log ( \
                    sequence BIGINT GENERATED ALWAYS AS IDENTITY, \
                    action TEXT, source_id INTEGER \
                 )",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "CREATE RULE issue_913_order_b AS ON INSERT TO issue_913_order_source \
                 DO ALSO INSERT INTO issue_913_order_log (action, source_id) \
                 VALUES ('b', NEW.id)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "CREATE RULE issue_913_order_a AS ON INSERT TO issue_913_order_source \
                 DO ALSO ( \
                    INSERT INTO issue_913_order_log (action, source_id) VALUES ('a1', NEW.id); \
                    INSERT INTO issue_913_order_log (action, source_id) VALUES ('a2', NEW.id) \
                 )",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "INSERT INTO issue_913_order_source VALUES (9)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT action, source_id FROM issue_913_order_log ORDER BY sequence",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT id FROM issue_913_order_source ORDER BY id",
            )]),
        ],
    );

    // Act
    let setup = &batches[..4];
    let inserted = &batches[4];
    let actions = &batches[5];
    let source = &batches[6];

    // Assert
    assert!(setup
        .iter()
        .all(|frames| wire::error_code(frames).is_none()));
    assert_eq!(command_tags(&batches[2]), vec!["CREATE RULE"]);
    assert_eq!(command_tags(&batches[3]), vec!["CREATE RULE"]);
    assert_eq!(wire::error_code(inserted), None);
    assert_eq!(command_tags(inserted), vec!["INSERT 0 1"]);
    assert_eq!(wire::error_code(actions), None);
    assert_eq!(command_tags(actions), vec!["SELECT 3"]);
    assert_eq!(
        wire::data_rows(actions),
        vec![
            vec![Some("a1".to_owned()), Some("9".to_owned())],
            vec![Some("a2".to_owned()), Some("9".to_owned())],
            vec![Some("b".to_owned()), Some("9".to_owned())],
        ]
    );
    assert_eq!(wire::error_code(source), None);
    assert_eq!(command_tags(source), vec!["SELECT 1"]);
    assert_eq!(wire::data_rows(source), vec![vec![Some("9".to_owned())]]);
}
