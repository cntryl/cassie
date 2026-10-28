//! Prepared result descriptors after catalog changes and rollback.

use crate::support_parameter_occurrence_wire::{run_script, run_script_with_config, Step};
use crate::support_pgwire as wire;

fn error_response_fields(frames: &[(u8, Vec<u8>)]) -> Vec<(char, String)> {
    frames
        .iter()
        .find(|(tag, _)| *tag == b'E')
        .map(|(_, payload)| wire::parse_error_fields(payload))
        .unwrap_or_default()
}

#[test]
fn should_bound_retained_prepared_descriptors_by_connection_query_budget() {
    // Arrange
    let mut config = cassie::config::CassieRuntimeConfig::from_env().expect("runtime config");
    config.limits.query_memory_budget_bytes = 2_048;
    let alias = "x".repeat(512);
    let mut steps = Vec::new();
    for index in 0..8 {
        let statement = format!("budgeted_descriptor_{index}");
        let sql = format!("SELECT 1 AS \"{alias}\"");
        steps.push(Step::ParseFlush(wire::parse_frame(&statement, &sql)));
        steps.push(Step::ThroughReady(vec![
            wire::describe_statement_frame(&statement),
            wire::sync_frame(),
        ]));
    }
    steps.push(Step::ThroughReady(vec![wire::simple_query_frame(
        "SELECT 42 AS recovery",
    )]));

    // Act
    let batches = run_script_with_config("budgeted-prepared-descriptor", steps, config);

    // Assert
    assert_eq!(batches.len(), 17);
    let mut rejected_descriptor = false;
    for index in 0..8 {
        let parse_index = index * 2;
        let describe_index = parse_index + 1;
        assert_eq!(
            batches[parse_index].first().map(|(tag, _)| *tag),
            Some(b'1')
        );
        match wire::error_code(&batches[describe_index]).as_deref() {
            None => {
                assert!(!rejected_descriptor);
                assert!(batches[describe_index].iter().any(|(tag, _)| *tag == b'T'));
            }
            Some("54000") => {
                rejected_descriptor = true;
                assert!(!batches[describe_index]
                    .iter()
                    .any(|(tag, _)| matches!(*tag, b't' | b'T')));
            }
            other => panic!("unexpected Describe result: {other:?}"),
        }
    }
    assert!(rejected_descriptor);
    assert_eq!(wire::error_code(&batches[16]), None);
    assert_eq!(
        wire::data_rows(&batches[16]),
        vec![vec![Some("42".to_owned())]]
    );
}

#[test]
fn should_reject_changed_result_types_given_a_prepared_statement_when_ddl_changes_its_output_type()
{
    // Arrange
    let statement = "issue_904_changed_result_type";
    let first_portal = "issue_904_changed_result_type_first";
    let changed_portal = "issue_904_changed_result_type_changed";
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(
            "CREATE TABLE issue_904_prepared_result (id INT PRIMARY KEY, value INT)",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "INSERT INTO issue_904_prepared_result VALUES (1, 7)",
        )]),
        Step::ParseFlush(wire::parse_frame_with_types(
            statement,
            "SELECT value FROM issue_904_prepared_result WHERE id = $1",
            &[23],
        )),
        Step::ThroughReady(vec![
            wire::describe_statement_frame(statement),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame(first_portal, statement, &["1"]),
            wire::describe_portal_frame(first_portal),
            wire::execute_frame(first_portal),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "DROP TABLE issue_904_prepared_result",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "CREATE TABLE issue_904_prepared_result (id INT PRIMARY KEY, value TEXT)",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "INSERT INTO issue_904_prepared_result VALUES (1, 'seven')",
        )]),
        Step::ThroughReady(vec![
            wire::bind_frame(changed_portal, statement, &["1"]),
            wire::execute_frame(changed_portal),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT 42 AS recovery")]),
    ];

    // Act
    let batches = run_script("issue-904-prepared-result-type-change", steps);

    // Assert
    assert_replaced_result_type_is_rejected(&batches);
}

fn assert_replaced_result_type_is_rejected(batches: &[Vec<(u8, Vec<u8>)>]) {
    assert_eq!(
        wire::error_code(&batches[0]),
        None,
        "table setup: {:?}",
        error_response_fields(&batches[0])
    );
    assert_eq!(
        wire::error_code(&batches[1]),
        None,
        "row setup: {:?}",
        error_response_fields(&batches[1])
    );
    assert_eq!(batches[2].first().map(|(tag, _)| *tag), Some(b'1'));
    let described = &batches[3];
    assert_eq!(
        wire::error_code(described),
        None,
        "Describe: {:?}",
        error_response_fields(described)
    );
    let result = described
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("prepared result RowDescription");
    assert_eq!(wire::parse_row_description(&result.1)[0].type_oid, 23);
    let first_execution = &batches[4];
    assert_eq!(
        wire::error_code(first_execution),
        None,
        "first Execute: {:?}",
        error_response_fields(first_execution)
    );
    assert_eq!(
        wire::data_rows(first_execution),
        vec![vec![Some("7".to_owned())]]
    );
    for (index, label) in [
        (5, "drop"),
        (6, "replacement table"),
        (7, "replacement row"),
    ] {
        assert_eq!(
            wire::error_code(&batches[index]),
            None,
            "{label}: {:?}",
            error_response_fields(&batches[index])
        );
    }
    let changed = &batches[8];
    assert_eq!(wire::error_code(changed).as_deref(), Some("0A000"));
    assert!(error_response_fields(changed).iter().any(
        |(kind, message)| *kind == 'M' && message == "cached plan must not change result type"
    ));
    assert!(!changed.iter().any(|(tag, _)| matches!(*tag, b'D' | b'C')));
    assert_eq!(wire::error_code(&batches[9]), None);
    assert_eq!(
        wire::data_rows(&batches[9]),
        vec![vec![Some("42".to_owned())]]
    );
}

#[test]
fn should_reject_result_type_change_after_bind_before_execute() {
    // Arrange
    let statement = "issue_904_bound_result_type";
    let portal = "issue_904_bound_result_type_portal";
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(
            "CREATE TABLE issue_904_bound_result (id INT PRIMARY KEY, value INT)",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "INSERT INTO issue_904_bound_result VALUES (1, 7)",
        )]),
        Step::ParseFlush(wire::parse_frame(
            statement,
            "SELECT value FROM issue_904_bound_result WHERE id = 1",
        )),
        Step::ThroughReady(vec![
            wire::describe_statement_frame(statement),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![wire::simple_query_frame("BEGIN")]),
        Step::ThroughReady(vec![
            wire::bind_frame(portal, statement, &[]),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "ALTER TABLE issue_904_bound_result ALTER COLUMN value TYPE TEXT USING value::text",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "UPDATE issue_904_bound_result SET value = 'seven' WHERE id = 1",
        )]),
        Step::ThroughReady(vec![wire::execute_frame(portal), wire::sync_frame()]),
        Step::ThroughReady(vec![wire::simple_query_frame("ROLLBACK")]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT 42 AS recovery")]),
    ];

    // Act
    let batches = run_script("issue-904-bound-result-type-change", steps);

    // Assert
    for (index, label) in [(0, "table setup"), (1, "row setup"), (4, "begin")] {
        assert_eq!(
            wire::error_code(&batches[index]),
            None,
            "{label}: {:?}",
            error_response_fields(&batches[index])
        );
    }
    assert_eq!(batches[2].first().map(|(tag, _)| *tag), Some(b'1'));
    let described = &batches[3];
    let row_description = described
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("prepared result RowDescription");
    assert_eq!(
        wire::parse_row_description(&row_description.1)[0].type_oid,
        23
    );
    assert_eq!(
        batches[5].first().map(|(tag, _)| *tag),
        Some(b'2'),
        "BindComplete before DDL"
    );
    assert_eq!(
        wire::error_code(&batches[6]),
        None,
        "alter: {:?}",
        error_response_fields(&batches[6])
    );
    assert_eq!(
        wire::error_code(&batches[7]),
        None,
        "update: {:?}",
        error_response_fields(&batches[7])
    );
    let changed = &batches[8];
    assert_eq!(wire::error_code(changed).as_deref(), Some("0A000"));
    assert!(error_response_fields(changed).iter().any(
        |(kind, message)| *kind == 'M' && message == "cached plan must not change result type"
    ));
    assert!(!changed.iter().any(|(tag, _)| matches!(*tag, b'D' | b'C')));
    assert_eq!(
        wire::error_code(&batches[9]),
        None,
        "rollback: {:?}",
        error_response_fields(&batches[9])
    );
    assert_eq!(wire::error_code(&batches[10]), None);
    assert_eq!(
        wire::data_rows(&batches[10]),
        vec![vec![Some("42".to_owned())]]
    );
}

#[test]
fn should_restore_prepared_resolution_given_rolled_back_catalog_ddl_when_a_statement_is_reused() {
    // Arrange
    let statement = "issue_904_rollback_result_type";
    let portal_before = "issue_904_rollback_before";
    let portal_after = "issue_904_rollback_after";
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(
            "CREATE TABLE issue_904_rollback_result (id INT PRIMARY KEY, value INT)",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "INSERT INTO issue_904_rollback_result VALUES (1, 7)",
        )]),
        Step::ParseFlush(wire::parse_frame(
            statement,
            "SELECT value FROM issue_904_rollback_result WHERE id = 1",
        )),
        Step::ThroughReady(vec![
            wire::describe_statement_frame(statement),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame(portal_before, statement, &[]),
            wire::execute_frame(portal_before),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![wire::simple_query_frame("BEGIN")]),
        Step::ThroughReady(vec![wire::simple_query_frame("SAVEPOINT before_retype")]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "ALTER TABLE issue_904_rollback_result ALTER COLUMN value TYPE TEXT USING value::text",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "ROLLBACK TO SAVEPOINT before_retype",
        )]),
        Step::ThroughReady(vec![
            wire::bind_frame(portal_after, statement, &[]),
            wire::describe_portal_frame(portal_after),
            wire::execute_frame(portal_after),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![wire::simple_query_frame("COMMIT")]),
    ];

    // Act
    let batches = run_script("issue-904-prepared-result-type-rollback", steps);

    // Assert
    assert_rollback_restores_prepared_resolution(&batches);
}

fn assert_rollback_restores_prepared_resolution(batches: &[Vec<(u8, Vec<u8>)>]) {
    assert_eq!(
        wire::error_code(&batches[0]),
        None,
        "table setup: {:?}",
        error_response_fields(&batches[0])
    );
    assert_eq!(
        wire::error_code(&batches[1]),
        None,
        "row setup: {:?}",
        error_response_fields(&batches[1])
    );
    assert_eq!(batches[2].first().map(|(tag, _)| *tag), Some(b'1'));
    let described = &batches[3];
    assert_eq!(
        wire::error_code(described),
        None,
        "Describe: {:?}",
        error_response_fields(described)
    );
    let result = described
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("prepared result RowDescription");
    assert_eq!(wire::parse_row_description(&result.1)[0].type_oid, 23);
    assert_eq!(
        wire::error_code(&batches[4]),
        None,
        "initial Execute: {:?}",
        error_response_fields(&batches[4])
    );
    assert_eq!(
        wire::data_rows(&batches[4]),
        vec![vec![Some("7".to_owned())]]
    );
    assert_eq!(
        wire::error_code(&batches[5]),
        None,
        "begin: {:?}",
        error_response_fields(&batches[5])
    );
    for (index, label) in [(6, "savepoint"), (7, "alter"), (8, "catalog rollback")] {
        assert_eq!(
            wire::error_code(&batches[index]),
            None,
            "{label}: {:?}",
            error_response_fields(&batches[index])
        );
    }
    let after_rollback = &batches[9];
    assert_eq!(
        wire::error_code(after_rollback),
        None,
        "reused Execute: {:?}",
        error_response_fields(after_rollback)
    );
    let restored = after_rollback
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("restored portal RowDescription");
    assert_eq!(wire::parse_row_description(&restored.1)[0].type_oid, 23);
    assert_eq!(
        wire::data_rows(after_rollback),
        vec![vec![Some("7".to_owned())]]
    );
    assert_eq!(
        wire::error_code(&batches[10]),
        None,
        "commit: {:?}",
        error_response_fields(&batches[10])
    );
}
