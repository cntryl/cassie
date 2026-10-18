use super::*;

#[test]
fn should_rename_a_rule_without_changing_its_rewrite_action() {
    // Arrange
    let batches = run_script(
        "issue-913-alter-rule-rename-lifecycle",
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame(
                "CREATE TABLE issue_913_alter_source (id INTEGER)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "CREATE TABLE issue_913_alter_log (source_id INTEGER)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "CREATE RULE issue_913_before AS ON INSERT TO issue_913_alter_source \
                 DO ALSO INSERT INTO issue_913_alter_log VALUES (NEW.id)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "ALTER RULE issue_913_before ON issue_913_alter_source \
                 RENAME TO issue_913_after",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT rulename::TEXT, ev_type::TEXT, ev_enabled::TEXT \
                 FROM pg_catalog.pg_rewrite \
                 WHERE ev_class = 'issue_913_alter_source'::regclass \
                   AND rulename IN ('issue_913_before', 'issue_913_after') \
                 ORDER BY rulename",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "INSERT INTO issue_913_alter_source VALUES (23)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT source_id FROM issue_913_alter_log ORDER BY source_id",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "DROP RULE issue_913_after ON issue_913_alter_source",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "INSERT INTO issue_913_alter_source VALUES (24)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT source_id FROM issue_913_alter_log ORDER BY source_id",
            )]),
        ],
    );

    // Act
    let create_rule = &batches[2];
    let rename_rule = &batches[3];
    let renamed_catalog = &batches[4];
    let rewritten_insert = &batches[5];
    let log_after_rename = &batches[6];
    let drop_rule = &batches[7];
    let insert_after_drop = &batches[8];
    let log_after_drop = &batches[9];

    // Assert
    assert!(batches[..2]
        .iter()
        .all(|frames| wire::error_code(frames).is_none()));
    assert_eq!(wire::error_code(create_rule), None);
    assert_eq!(command_tags(create_rule), vec!["CREATE RULE"]);
    assert_eq!(wire::error_code(rename_rule), None);
    assert_eq!(command_tags(rename_rule), vec!["ALTER RULE"]);
    assert_eq!(wire::error_code(renamed_catalog), None);
    assert_eq!(command_tags(renamed_catalog), vec!["SELECT 1"]);
    let catalog_columns = renamed_catalog
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("renamed pg_rewrite RowDescription");
    assert_eq!(
        wire::parse_row_description(&catalog_columns.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![25, 25, 25]
    );
    assert_eq!(
        wire::data_rows(renamed_catalog),
        vec![vec![
            Some("issue_913_after".to_owned()),
            Some("3".to_owned()),
            Some("O".to_owned()),
        ]]
    );

    assert_eq!(wire::error_code(rewritten_insert), None);
    assert_eq!(command_tags(rewritten_insert), vec!["INSERT 0 1"]);
    assert_eq!(wire::error_code(log_after_rename), None);
    assert_eq!(
        wire::data_rows(log_after_rename),
        vec![vec![Some("23".to_owned())]]
    );

    assert_eq!(wire::error_code(drop_rule), None);
    assert_eq!(command_tags(drop_rule), vec!["DROP RULE"]);
    assert_eq!(wire::error_code(insert_after_drop), None);
    assert_eq!(command_tags(insert_after_drop), vec!["INSERT 0 1"]);
    assert_eq!(wire::error_code(log_after_drop), None);
    assert_eq!(command_tags(log_after_drop), vec!["SELECT 1"]);
    assert_eq!(
        wire::data_rows(log_after_drop),
        vec![vec![Some("23".to_owned())]]
    );
}
