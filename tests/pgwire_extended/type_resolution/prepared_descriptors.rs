//! Prepared result descriptors after catalog changes and rollback.

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

fn error_response_fields(frames: &[(u8, Vec<u8>)]) -> Vec<(char, String)> {
    frames
        .iter()
        .find(|(tag, _)| *tag == b'E')
        .map(|(_, payload)| wire::parse_error_fields(payload))
        .unwrap_or_default()
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
            "CREATE TEMP TABLE issue_904_prepared_result (id INT PRIMARY KEY, value INT); \
             INSERT INTO issue_904_prepared_result VALUES (1, 7)",
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
            "DROP TABLE issue_904_prepared_result; \
             CREATE TEMP TABLE issue_904_prepared_result (id INT PRIMARY KEY, value TEXT); \
             INSERT INTO issue_904_prepared_result VALUES (1, 'seven')",
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
    assert_eq!(
        wire::error_code(&batches[0]),
        None,
        "table setup: {:?}",
        error_response_fields(&batches[0])
    );
    assert_eq!(batches[1].first().map(|(tag, _)| *tag), Some(b'1'));
    let described = &batches[2];
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
    let first_execution = &batches[3];
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
    let changed = &batches[5];
    assert_eq!(wire::error_code(changed).as_deref(), Some("0A000"));
    assert!(error_response_fields(changed).iter().any(
        |(kind, message)| *kind == 'M' && message == "cached plan must not change result type"
    ));
    assert!(!changed.iter().any(|(tag, _)| matches!(*tag, b'D' | b'C')));
    assert_eq!(wire::error_code(&batches[6]), None);
    assert_eq!(
        wire::data_rows(&batches[6]),
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
            "CREATE TEMP TABLE issue_904_rollback_result (id INT PRIMARY KEY, value INT); \
             INSERT INTO issue_904_rollback_result VALUES (1, 7)",
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
        Step::ThroughReady(vec![wire::simple_query_frame(
            "BEGIN; SAVEPOINT before_retype; \
             DROP TABLE issue_904_rollback_result; \
             CREATE TEMP TABLE issue_904_rollback_result (id INT PRIMARY KEY, value TEXT); \
             INSERT INTO issue_904_rollback_result VALUES (1, 'seven'); \
             ROLLBACK TO SAVEPOINT before_retype",
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
    assert_eq!(
        wire::error_code(&batches[0]),
        None,
        "table setup: {:?}",
        error_response_fields(&batches[0])
    );
    assert_eq!(batches[1].first().map(|(tag, _)| *tag), Some(b'1'));
    let described = &batches[2];
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
        wire::error_code(&batches[3]),
        None,
        "initial Execute: {:?}",
        error_response_fields(&batches[3])
    );
    assert_eq!(
        wire::data_rows(&batches[3]),
        vec![vec![Some("7".to_owned())]]
    );
    assert_eq!(
        wire::error_code(&batches[4]),
        None,
        "catalog rollback: {:?}",
        error_response_fields(&batches[4])
    );
    let after_rollback = &batches[5];
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
        wire::error_code(&batches[6]),
        None,
        "commit: {:?}",
        error_response_fields(&batches[6])
    );
}
