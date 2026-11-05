//! A command overlay may advance while its committed native view stays fixed.

use std::sync::Arc;
use std::time::Instant;

use super::*;
use crate::app::{Cassie, CassieSession, SessionReadScope};
use crate::config::{CassieRuntimeConfig, CassieRuntimeLimits};

fn controls(budget: usize) -> QueryExecutionControls {
    QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: budget,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    )
}

fn fixture() -> (Cassie, CassieSession) {
    let cassie =
        Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).expect("memory fixture");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE command_overlay (value INT)",
            Vec::new(),
        )
        .expect("table");
    cassie
        .midge
        .put_document(
            "command_overlay",
            Some("base".to_owned()),
            serde_json::json!({"value": 1}),
        )
        .expect("seed");
    session.begin_transaction(None).expect("begin");
    (cassie, session)
}

fn payload(
    cassie: &Cassie,
    session: &CassieSession,
    owner: &Arc<StatementDataRead>,
    id: &str,
) -> Option<serde_json::Value> {
    let _data = StatementReadScope::enter(Some(owner));
    let _overlay = SessionReadScope::enter(owner.overlay());
    cassie
        .get_document_for_session(Some(session), "command_overlay", id)
        .expect("read")
        .map(|document| document.payload)
}

#[test]
fn should_keep_committed_data_frozen_across_command_overlays() {
    // Arrange
    let (cassie, session) = fixture();
    let original = controls(16 * 1024 * 1024);
    let retained = StatementDataRead::capture_repeatable_read_with_overlay(
        &cassie.midge,
        "postgres",
        &original,
        None,
    )
    .expect("retained Data");
    session
        .stage_document_write(
            "command_overlay",
            "own".to_owned(),
            serde_json::json!({"value": 2}),
        )
        .expect("first own change");
    let first_controls = controls(16 * 1024 * 1024);
    let first_overlay = session
        .capture_statement_overlay(&first_controls)
        .expect("first overlay");
    let first = StatementDataRead::with_overlay(&retained, Some(first_overlay), &first_controls)
        .expect("first command");
    session.create_savepoint("before_later").expect("savepoint");
    session
        .stage_document_write(
            "command_overlay",
            "later".to_owned(),
            serde_json::json!({"value": 3}),
        )
        .expect("later own change");
    cassie
        .midge
        .put_document(
            "command_overlay",
            Some("base".to_owned()),
            serde_json::json!({"value": 9}),
        )
        .expect("other committed change");
    let second_controls = controls(16 * 1024 * 1024);
    let second_overlay = session
        .capture_statement_overlay(&second_controls)
        .expect("second overlay");
    let second = StatementDataRead::with_overlay(&retained, Some(second_overlay), &second_controls)
        .expect("second command");

    // Act
    session
        .rollback_to_savepoint("before_later")
        .expect("restore own changes");
    let third_controls = controls(16 * 1024 * 1024);
    let third_overlay = session
        .capture_statement_overlay(&third_controls)
        .expect("restored overlay");
    let third = StatementDataRead::with_overlay(&retained, Some(third_overlay), &third_controls)
        .expect("restored command");

    // Assert
    assert!(Arc::ptr_eq(&first.snapshot, &second.snapshot));
    assert!(Arc::ptr_eq(&second.snapshot, &third.snapshot));
    for owner in [&first, &second, &third] {
        assert_eq!(
            payload(&cassie, &session, owner, "base"),
            Some(serde_json::json!({"value": 1}))
        );
        assert_eq!(
            payload(&cassie, &session, owner, "own"),
            Some(serde_json::json!({"value": 2}))
        );
    }
    assert_eq!(payload(&cassie, &session, &first, "later"), None);
    assert_eq!(
        payload(&cassie, &session, &second, "later"),
        Some(serde_json::json!({"value": 3}))
    );
    assert_eq!(payload(&cassie, &session, &third, "later"), None);
}

#[test]
fn should_release_each_command_admission_after_its_last_owner() {
    // Arrange
    let (cassie, session) = fixture();
    let original = controls(16 * 1024 * 1024);
    let retained = StatementDataRead::capture_repeatable_read_with_overlay(
        &cassie.midge,
        "postgres",
        &original,
        None,
    )
    .expect("retained Data");
    let initial_charge = original.current_query_memory_bytes();
    let command_controls = controls(16 * 1024 * 1024);
    let overlay = session
        .capture_statement_overlay(&command_controls)
        .expect("overlay");
    let command = StatementDataRead::with_overlay(&retained, Some(overlay), &command_controls)
        .expect("command");
    let alias = Arc::clone(&command);
    let command_charge = command_controls.current_query_memory_bytes();

    // Act
    drop(retained);
    let shared_charge = original.current_query_memory_bytes();
    drop(command);
    let aliased_charge = command_controls.current_query_memory_bytes();
    drop(alias);

    // Assert
    assert!(initial_charge > shared_charge);
    assert!(shared_charge > 0);
    assert!(command_charge > 0);
    assert_eq!(aliased_charge, command_charge);
    assert_eq!(original.current_query_memory_bytes(), 0);
    assert_eq!(command_controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_deny_command_ownership_without_admitting_its_retained_core_again() {
    // Arrange
    let (cassie, _) = fixture();
    let original = controls(16 * 1024 * 1024);
    let retained = StatementDataRead::capture_repeatable_read_with_overlay(
        &cassie.midge,
        "postgres",
        &original,
        None,
    )
    .expect("retained Data");
    let charged = original.current_query_memory_bytes();
    let denied = controls(1);

    // Act
    let command = StatementDataRead::with_overlay(&retained, None, &denied);

    // Assert
    assert!(matches!(command, Err(CassieError::ResourceLimit(_))));
    assert_eq!(denied.current_query_memory_bytes(), 0);
    assert_eq!(original.current_query_memory_bytes(), charged);
}

#[test]
fn should_rebind_a_command_view_to_its_atomic_working_session() {
    // Arrange
    let (cassie, session) = fixture();
    let original = controls(16 * 1024 * 1024);
    let retained = StatementDataRead::capture_repeatable_read_with_overlay(
        &cassie.midge,
        "postgres",
        &original,
        None,
    )
    .expect("retained Data");
    session
        .stage_document_write(
            "command_overlay",
            "own".to_owned(),
            serde_json::json!({"value": 2}),
        )
        .expect("precommand own row");
    let overlay = session
        .capture_statement_overlay(&original)
        .expect("command overlay");
    let command =
        StatementDataRead::with_overlay(&retained, Some(overlay), &original).expect("command");
    session
        .stage_document_write(
            "command_overlay",
            "later".to_owned(),
            serde_json::json!({"value": 3}),
        )
        .expect("earlier CTE effect");
    let batch = session
        .fork_statement_batch()
        .expect("later CTE working fork");
    let working = batch.session();
    let rebound_controls = controls(16 * 1024 * 1024);

    // Act
    let rebound = working
        .rebind_statement_read(&command, &rebound_controls)
        .expect("rebound command");
    let original_matches_working = command
        .overlay()
        .expect("source overlay")
        .matches_session(working);
    let rebound_matches_working = rebound
        .overlay()
        .expect("rebound overlay")
        .matches_session(working);
    let rebound_matches_original = rebound
        .overlay()
        .expect("rebound overlay")
        .matches_session(&session);
    let later = payload(&cassie, working, &rebound, "later");
    let own = payload(&cassie, working, &rebound, "own");
    drop(rebound);

    // Assert
    assert!(!original_matches_working);
    assert!(rebound_matches_working);
    assert!(!rebound_matches_original);
    assert_eq!(later, None);
    assert_eq!(own, Some(serde_json::json!({"value": 2})));
    assert_eq!(rebound_controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_preserve_descriptor_authority_when_command_owners_share_a_core() {
    // Arrange
    for repeatable in [false, true] {
        let (cassie, session) = fixture();
        session.rollback_transaction();
        let original = controls(16 * 1024 * 1024);
        let retained = if repeatable {
            StatementDataRead::capture_repeatable_read_with_overlay(
                &cassie.midge,
                "postgres",
                &original,
                None,
            )
        } else {
            StatementDataRead::capture_with_overlay(&cassie.midge, "postgres", &original, None)
        }
        .expect("selected committed core");
        cassie
            .execute_sql(
                &session,
                "ALTER TABLE command_overlay ADD COLUMN note TEXT",
                Vec::new(),
            )
            .expect("later encoding descriptor");
        let command_controls = controls(16 * 1024 * 1024);

        // Act
        let command = StatementDataRead::with_overlay(&retained, None, &command_controls)
            .expect("command owner");
        let _scope = StatementReadScope::enter(Some(&command));
        let schema = cassie
            .midge
            .row_schema("command_overlay")
            .expect("selected descriptor");

        // Assert
        assert_eq!(
            schema.fields.iter().any(|field| field.name == "note"),
            repeatable
        );
    }
}
