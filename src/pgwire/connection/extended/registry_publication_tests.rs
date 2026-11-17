//! Actual registry completion cannot publish a stale frame into connection state.
use super::*;
use crate::app::FailureAuthority;
use crate::config::CassieRuntimeConfig;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

type Action = Box<dyn FnOnce(&CassieSession) + Send>;
static ACTIONS: OnceLock<Mutex<HashMap<usize, Action>>> = OnceLock::new();

pub(in crate::pgwire::connection::extended) fn after_registry(
    cassie: &Cassie,
    session: &CassieSession,
) {
    let action = ACTIONS.get().and_then(|actions| {
        actions
            .lock()
            .unwrap()
            .remove(&(std::sync::Arc::as_ptr(&cassie.runtime) as usize))
    });
    if let Some(action) = action {
        action(session);
    }
}

fn install(cassie: &Cassie, action: impl FnOnce(&CassieSession) + Send + 'static) {
    assert!(ACTIONS
        .get_or_init(Mutex::default)
        .lock()
        .unwrap()
        .insert(
            std::sync::Arc::as_ptr(&cassie.runtime) as usize,
            Box::new(action)
        )
        .is_none());
}

fn fixture() -> (
    std::sync::Arc<Cassie>,
    CassieSession,
    CassieSession,
    SessionState,
) {
    let cassie = std::sync::Arc::new(
        Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).unwrap(),
    );
    cassie.startup().unwrap();
    let root = cassie.create_session("registry_publication", None);
    root.begin_transaction(None).unwrap();
    let operation = root.with_new_wire_operation();
    FailureAuthority::capture_for_operation(&operation);
    let mut state = SessionState::new(&cassie.runtime.limits());
    state.session = Some(root.clone());
    (cassie, root, operation, state)
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

fn prepared(name: &str, id: u64) -> PreparedStatement {
    PreparedStatement {
        name: name.into(),
        id,
        query: "SELECT 3".into(),
        parsed: None,
        sql_fingerprint: 0,
        parameter_count: 0,
        parameter_types: vec![],
        described: false,
    }
}

#[test]
fn should_not_publish_named_or_unnamed_parse_after_registry_owner_replacement() {
    for name in ["", "late"] {
        // Arrange
        let (cassie, root, operation, mut state) = fixture();
        if name.is_empty() {
            state
                .prepared_statements
                .insert(String::new(), prepared("", 444));
        }
        install(&cassie, |session| {
            let root = session.without_operation_failure_authority();
            root.rollback_transaction();
            root.begin_transaction(None).unwrap();
            root.register_wire_prepared("late", 999, "SELECT 9")
                .unwrap();
        });
        let mut output = Vec::new();

        // Act
        let result = runtime().block_on(super::super::parse::handle_parse(
            std::sync::Arc::clone(&cassie),
            &cassie.runtime,
            &mut output,
            &mut state,
            &operation,
            super::super::parse::ParseRequest {
                name: name.into(),
                query: "SELECT 7".into(),
                parameter_type_oids: vec![],
            },
        ));

        // Assert
        assert_eq!(result.unwrap_err().pg_error.code, "25000");
        assert_eq!(output, [] as [u8; 0]);
        assert!(!state.prepared_statements.contains_key("late"));
        if name.is_empty() {
            assert_eq!(state.prepared_statements[""].id, 444);
        }
        assert_eq!(
            root.with_session_objects(|objects| objects
                .prepared("late")
                .and_then(|value| value.wire_id)),
            Some(999)
        );
        assert!(!root.is_transaction_failed());
    }
}

#[test]
fn should_not_publish_close_or_remove_portals_after_registry_owner_replacement() {
    // Arrange
    let (cassie, root, operation, mut state) = fixture();
    root.register_wire_prepared("source", 444, "SELECT 3")
        .unwrap();
    state
        .prepared_statements
        .insert("source".into(), prepared("source", 444));
    install(&cassie, |session| {
        let root = session.without_operation_failure_authority();
        root.rollback_transaction();
        root.begin_transaction(None).unwrap();
        root.register_wire_prepared("source", 999, "SELECT 9")
            .unwrap();
    });
    let mut output = Vec::new();

    // Act
    let result = runtime().block_on(super::super::handle_close(
        std::sync::Arc::clone(&cassie),
        &cassie.runtime,
        &mut output,
        (&mut state, &operation),
        super::super::DescribeTarget::Statement,
        "source".into(),
    ));

    // Assert
    assert_eq!(result.unwrap_err().pg_error.code, "25000");
    assert_eq!(output, [] as [u8; 0]);
    assert_eq!(state.prepared_statements["source"].id, 444);
    assert_eq!(
        root.with_session_objects(|objects| objects
            .prepared("source")
            .and_then(|value| value.wire_id)),
        Some(999)
    );
    assert!(!root.is_transaction_failed());
}

#[test]
fn should_apply_prepared_namespace_changes_for_the_current_frame_owner() {
    // Arrange
    let (cassie, root, operation, mut state) = fixture();
    let mut output = Vec::new();

    // Act
    runtime().block_on(async {
        super::super::parse::handle_parse(
            std::sync::Arc::clone(&cassie),
            &cassie.runtime,
            &mut output,
            &mut state,
            &operation,
            super::super::parse::ParseRequest {
                name: "source".into(),
                query: "SELECT 7".into(),
                parameter_type_oids: vec![],
            },
        )
        .await
        .unwrap();
        assert!(state.prepared_statements.contains_key("source"));
        super::super::handle_close(
            std::sync::Arc::clone(&cassie),
            &cassie.runtime,
            &mut output,
            (&mut state, &operation),
            super::super::DescribeTarget::Statement,
            "source".into(),
        )
        .await
        .unwrap();
    });

    // Assert
    assert!(!state.prepared_statements.contains_key("source"));
    assert!(root.with_session_objects(|objects| objects.prepared("source").is_none()));
    assert!(!root.is_transaction_failed());
}

#[test]
fn should_preserve_b_namespace_when_an_a_close_reaches_the_shared_leaf() {
    // Arrange
    let (cassie, root, operation, mut state) = fixture();
    root.rollback_transaction();
    root.begin_transaction(None).unwrap();
    root.register_wire_prepared("source", 999, "SELECT 9")
        .unwrap();
    state
        .prepared_statements
        .insert("source".into(), prepared("source", 999));
    let owner = FailureAuthority::capture(&root).identity().unwrap();
    state
        .publish_bound_portal(
            crate::pgwire::protocol::Portal {
                name: "b".into(),
                statement_name: "source".into(),
                prepared_id: 999,
                params: vec![],
                parameter_memory: vec![],
                result_formats: vec![],
                described: false,
                suspended: None,
                completed_command: None,
            },
            owner,
            None,
            &cassie.runtime,
            999,
        )
        .unwrap();
    let mut output = Vec::new();

    // Act
    let result = runtime().block_on(super::super::handle_close(
        std::sync::Arc::clone(&cassie),
        &cassie.runtime,
        &mut output,
        (&mut state, &operation),
        super::super::DescribeTarget::Statement,
        "source".into(),
    ));

    // Assert
    assert_eq!(result.unwrap_err().pg_error.code, "25000");
    assert_eq!(output, [] as [u8; 0]);
    assert_eq!(state.prepared_statements["source"].id, 999);
    assert_eq!(state.portal_owner_identity("b"), Some(owner));
    assert!(state.portals.contains_key("b"));
    assert_eq!(
        root.with_session_objects(|objects| objects
            .prepared("source")
            .and_then(|value| value.wire_id)),
        Some(999)
    );
    assert!(!root.is_transaction_failed());
}

#[test]
fn should_not_remove_b_portals_for_an_a_portal_close() {
    // Arrange
    let (cassie, root, operation, mut state) = fixture();
    root.rollback_transaction();
    root.begin_transaction(None).unwrap();
    let owner = FailureAuthority::capture(&root).identity().unwrap();
    state
        .publish_bound_portal(
            crate::pgwire::protocol::Portal {
                name: "b".into(),
                statement_name: "source".into(),
                prepared_id: 999,
                params: vec![],
                parameter_memory: vec![],
                result_formats: vec![],
                described: false,
                suspended: None,
                completed_command: None,
            },
            owner,
            None,
            &cassie.runtime,
            999,
        )
        .unwrap();
    let mut output = Vec::new();

    // Act
    let result = runtime().block_on(super::super::handle_close(
        std::sync::Arc::clone(&cassie),
        &cassie.runtime,
        &mut output,
        (&mut state, &operation),
        super::super::DescribeTarget::Portal,
        "b".into(),
    ));

    // Assert
    assert_eq!(result.unwrap_err().pg_error.code, "25000");
    assert_eq!(output, [] as [u8; 0]);
    assert!(state.portals.contains_key("b"));
    assert_eq!(state.portal_owner_identity("b"), Some(owner));
    assert!(!root.is_transaction_failed());
}
