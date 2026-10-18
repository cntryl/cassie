#[test]
fn should_expose_the_view_select_rewrite_rule_in_pg_rewrite() {
    // Arrange
    let batches = run_script(
        "issue-913-view-select-rule-catalog",
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame(
                "CREATE TABLE issue_913_select_source (id INTEGER, active BOOLEAN)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "INSERT INTO issue_913_select_source VALUES \
                 (1, TRUE), (2, FALSE), (3, TRUE)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "CREATE VIEW issue_913_select_view AS \
                 SELECT id FROM issue_913_select_source WHERE active",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT id FROM issue_913_select_view ORDER BY id",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT rule.rulename, rule.ev_type::TEXT, rule.is_instead, \
                        rule.ev_enabled::TEXT \
                 FROM pg_catalog.pg_rewrite AS rule \
                 JOIN pg_catalog.pg_class AS relation ON relation.oid = rule.ev_class \
                 WHERE relation.relname = 'issue_913_select_view' \
                 ORDER BY rule.rulename",
            )]),
        ],
    );

    // Act
    let setup = &batches[..3];
    let view_select = &batches[3];
    let select_rule = &batches[4];

    // Assert
    assert!(setup
        .iter()
        .all(|frames| wire::error_code(frames).is_none()));
    assert_eq!(wire::error_code(view_select), None);
    assert_eq!(command_tags(view_select), vec!["SELECT 2"]);
    let view_columns = view_select
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("view SELECT RowDescription");
    assert_eq!(wire::parse_row_description(&view_columns.1)[0].type_oid, 23);
    assert_eq!(
        wire::data_rows(view_select),
        vec![vec![Some("1".to_owned())], vec![Some("3".to_owned())]]
    );

    assert_eq!(wire::error_code(select_rule), None);
    assert_eq!(command_tags(select_rule), vec!["SELECT 1"]);
    let rule_columns = select_rule
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("pg_rewrite SELECT RowDescription");
    assert_eq!(
        wire::parse_row_description(&rule_columns.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![19, 25, 16, 25]
    );
    assert_eq!(
        wire::data_rows(select_rule),
        vec![vec![
            Some("_RETURN".to_owned()),
            Some("1".to_owned()),
            Some("t".to_owned()),
            Some("O".to_owned()),
        ]]
    );
}
