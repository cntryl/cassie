//! PostgreSQL 18 rewrite-rule ordering, recursion, catalog, and rollback contracts.

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

mod alter_rule_lifecycle;

fn command_tags(frames: &[(u8, Vec<u8>)]) -> Vec<String> {
    frames
        .iter()
        .filter(|frame| frame.0 == b'C')
        .map(|(_, payload)| {
            let terminator = payload
                .iter()
                .position(|byte| *byte == 0)
                .expect("CommandComplete tag terminator");
            String::from_utf8(payload[..terminator].to_vec()).expect("command tag is UTF-8")
        })
        .collect()
}

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
    assert!(wire::data_rows(recursive_insert).is_empty());
    assert!(command_tags(recursive_insert).is_empty());
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
