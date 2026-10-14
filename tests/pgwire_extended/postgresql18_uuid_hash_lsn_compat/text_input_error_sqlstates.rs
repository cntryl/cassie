//! PostgreSQL 18 UUID and pg_lsn text input errors tracked by #816.

use crate::support_parameter_occurrence_wire::{Step, run_script};
use crate::support_pgwire as wire;

type Batch = Vec<(u8, Vec<u8>)>;

fn run_invalid_text_bind(
    name: &str,
    type_oid: i32,
    sql: &str,
    invalid: &str,
    valid: &str,
) -> Vec<Batch> {
    let statement = format!("{name}_text_input");
    let invalid_portal = format!("{name}_invalid_portal");
    let valid_portal = format!("{name}_valid_portal");
    run_script(
        &format!("issue-816-{name}-text-error"),
        vec![
            Step::ParseFlush(wire::parse_frame_with_types(&statement, sql, &[type_oid])),
            Step::ThroughReady(vec![
                wire::describe_statement_frame(&statement),
                wire::sync_frame(),
            ]),
            Step::ThroughReady(vec![
                wire::bind_frame(&invalid_portal, &statement, &[invalid]),
                wire::describe_portal_frame(&invalid_portal),
                wire::execute_frame(&invalid_portal),
                wire::sync_frame(),
            ]),
            Step::ThroughReady(vec![
                wire::bind_frame(&valid_portal, &statement, &[valid]),
                wire::describe_portal_frame(&valid_portal),
                wire::execute_frame(&valid_portal),
                wire::sync_frame(),
            ]),
        ],
    )
}

fn assert_invalid_text_bind_and_recovery(batches: &[Batch], type_oid: i32, expected_value: &str) {
    assert_eq!(batches.len(), 4);
    assert_eq!(wire::error_code(&batches[0]), None, "Parse");

    let description = &batches[1];
    assert_eq!(wire::error_code(description), None, "Describe");
    let parameters = description
        .iter()
        .find(|(tag, _)| *tag == b't')
        .expect("typed ParameterDescription");
    assert_eq!(
        wire::parse_parameter_description(&parameters.1),
        vec![type_oid]
    );

    let rejected = &batches[2];
    assert_eq!(wire::error_code(rejected).as_deref(), Some("22P02"));
    assert!(
        !rejected
            .iter()
            .any(|(tag, _)| matches!(*tag, b'T' | b'D' | b'C')),
        "invalid text Bind must not return a result"
    );
    assert_eq!(
        rejected.last().map(|frame| frame.1.as_slice()),
        Some(&b"I"[..]),
        "invalid Bind should end at ReadyForQuery"
    );

    let recovered = &batches[3];
    assert_eq!(
        wire::error_code(recovered),
        None,
        "prepared statement recovery"
    );
    let columns = recovered
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .map(|(_, payload)| wire::parse_row_description(payload))
        .expect("recovery RowDescription");
    assert_eq!(columns[0].type_oid, type_oid);
    assert_eq!(
        wire::data_rows(recovered),
        vec![vec![Some(expected_value.to_owned())]]
    );
}

#[test]
fn should_reject_invalid_uuid_text_bind_with_sqlstate() {
    // Arrange
    let type_oid = 2950;
    let sql = "SELECT $1::UUID AS value";

    // Act
    let batches = run_invalid_text_bind(
        "uuid",
        type_oid,
        sql,
        "not-a-uuid",
        "a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11",
    );

    // Assert
    assert_invalid_text_bind_and_recovery(
        &batches,
        type_oid,
        "a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11",
    );
}

#[test]
fn should_reject_invalid_pg_lsn_text_bind_with_sqlstate() {
    // Arrange
    let type_oid = 3220;
    let sql = "SELECT $1::PG_LSN AS value";

    // Act
    let batches = run_invalid_text_bind("pg_lsn", type_oid, sql, "not-an-lsn", "0/0");

    // Assert
    assert_invalid_text_bind_and_recovery(&batches, type_oid, "0/0");
}
