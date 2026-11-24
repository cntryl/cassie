//! Owner-filtered portal retirement and explicit preparation adoption.

use super::*;
use crate::app::{Cassie, FailureAuthority};
use crate::config::CassieRuntimeConfig;
use std::sync::Arc;

fn fixture() -> (Arc<Cassie>, CassieSession, SessionState) {
    let cassie = Arc::new(
        Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).expect("memory fixture"),
    );
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    let state = SessionState::new(&cassie.runtime().limits());
    (cassie, session, state)
}

fn portal(name: &str) -> Portal {
    Portal {
        name: name.to_owned(),
        statement_name: "source".to_owned(),
        prepared_id: 1,
        params: Vec::new(),
        parameter_memory: Vec::new(),
        result_formats: Vec::new(),
        described: false,
        suspended: None,
        completed_command: None,
    }
}

#[test]
fn should_retire_old_portals_without_clearing_the_replacement_transaction() {
    // Arrange
    let (cassie, session, mut state) = fixture();
    cassie
        .execute_sql(&session, "BEGIN", Vec::new())
        .expect("begin A");
    let old = FailureAuthority::capture(&session)
        .identity()
        .expect("owner A");
    state
        .publish_bound_portal(portal("old"), old, None, &cassie.runtime(), 2)
        .expect("portal A");
    cassie
        .execute_sql(&session, "ROLLBACK", Vec::new())
        .expect("rollback A");
    cassie
        .execute_sql(&session, "BEGIN", Vec::new())
        .expect("begin B");
    let current = FailureAuthority::capture(&session)
        .identity()
        .expect("owner B");
    state
        .publish_bound_portal(portal("current"), current, None, &cassie.runtime(), 2)
        .expect("portal B");
    // Act
    state.clear_portals_owned_by(old, &cassie.runtime());
    // Assert
    assert!(!state.portals.contains_key("old"));
    assert!(state.portals.contains_key("current"));
    assert_eq!(state.portal_owner_identity("current"), Some(current));
    state.remove_portal("current");
    assert_eq!(state.portal_memory_controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_retain_idle_bound_portals_only_for_their_own_implicit_start() {
    // Arrange
    let (cassie, session, mut state) = fixture();
    let operation = session.with_new_wire_operation();
    let authority = FailureAuthority::capture_for_operation(&operation);
    let before = authority.identity().expect("preparation owner");
    state
        .publish_bound_portal(portal("bound"), before, None, &cassie.runtime(), 1)
        .expect("idle portal");
    operation
        .begin_implicit_transaction_for_operation(Some(&authority))
        .expect("own implicit start");
    let after = authority.identity().expect("implicit owner");
    // Act
    state.adopt_operation_portals(&authority);
    state.reconcile_portals(after, &cassie.runtime());
    // Assert
    assert!(state.portals.contains_key("bound"));
    assert!(state.portal_owners.belongs_to("bound", after));
    session.rollback_transaction();
    session
        .begin_implicit_transaction()
        .expect("unrelated implicit start");
    let replacement = FailureAuthority::capture(&session)
        .identity()
        .expect("replacement owner");
    state.reconcile_portals(replacement, &cassie.runtime());
    assert!(state.portals.is_empty());
    assert_eq!(state.portal_memory_controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_portal_owner_metadata_when_a_prepared_statement_closes() {
    // Arrange
    let (cassie, session, mut state) = fixture();
    let owner = FailureAuthority::capture(&session)
        .identity()
        .expect("preparation owner");
    state
        .publish_bound_portal(portal("p"), owner, None, &cassie.runtime(), 1)
        .expect("portal");
    // Act
    let removed = state.remove_portals_for_prepared_id(1);
    // Assert
    assert_eq!(removed, 1);
    assert!(state.portals.is_empty());
    assert!(!state.portal_owners.belongs_to("p", owner));
    assert_eq!(state.portal_memory_controls.current_query_memory_bytes(), 0);
}
