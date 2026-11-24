//! The captured Bind owner is validated before any replacement becomes visible.

use super::*;
use crate::app::FailureAuthority;
use crate::config::CassieRuntimeConfig;
use crate::pgwire::protocol::PreparedStatement;

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
}

#[test]
fn should_reject_stale_bind_without_replacing_a_newer_portal() {
    // Arrange
    let cassie = Arc::new(
        Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).expect("memory fixture"),
    );
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(&session, "BEGIN", Vec::new())
        .expect("begin A");
    let operation = session.with_new_wire_operation();
    FailureAuthority::capture_for_operation(&operation);
    cassie
        .execute_sql(&session, "ROLLBACK", Vec::new())
        .expect("rollback A");
    cassie
        .execute_sql(&session, "BEGIN", Vec::new())
        .expect("begin B");
    let mut state = SessionState::new(&cassie.runtime().limits());
    state.prepared_statements.insert(
        "source".to_owned(),
        PreparedStatement {
            id: 1,
            name: "source".to_owned(),
            query: String::new(),
            parsed: None,
            sql_fingerprint: 0,
            parameter_count: 0,
            parameter_types: Vec::new(),
            described: false,
        },
    );
    let current = FailureAuthority::capture(&session)
        .identity()
        .expect("owner B");
    state
        .publish_bound_portal(
            Portal {
                name: "p".to_owned(),
                statement_name: "source".to_owned(),
                prepared_id: 1,
                params: Vec::new(),
                parameter_memory: Vec::new(),
                result_formats: Vec::new(),
                described: false,
                suspended: None,
                completed_command: Some("B".to_owned()),
            },
            current,
            None,
            &cassie.runtime(),
            1,
        )
        .expect("portal B");
    let mut output = Vec::new();
    // Act
    let result = runtime().block_on(handle_bind(
        Arc::clone(&cassie),
        &cassie.runtime(),
        &mut output,
        &mut state,
        &operation,
        BindRequest {
            portal: "p".to_owned(),
            statement: "source".to_owned(),
            parameter_formats: Vec::new(),
            parameters: Vec::new(),
            result_formats: Vec::new(),
        },
    ));
    // Assert
    assert_eq!(result.expect_err("stale Bind").pg_error.code, "25000");
    assert!(output.is_empty(), "stale Bind must not emit BindComplete");
    assert_eq!(state.portals["p"].completed_command.as_deref(), Some("B"));
    assert_eq!(state.portal_owner_identity("p"), Some(current));
}

#[test]
fn should_publish_an_idle_bind_without_starting_a_mutation_transaction() {
    // Arrange
    let cassie = Arc::new(
        Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).expect("memory fixture"),
    );
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    let operation = session.with_new_wire_operation();
    let owner = FailureAuthority::capture_for_operation(&operation)
        .identity()
        .expect("idle owner");
    let mut state = SessionState::new(&cassie.runtime().limits());
    state.prepared_statements.insert(
        "source".to_owned(),
        PreparedStatement {
            id: 1,
            name: "source".to_owned(),
            query: String::new(),
            parsed: None,
            sql_fingerprint: 0,
            parameter_count: 0,
            parameter_types: Vec::new(),
            described: false,
        },
    );
    let mut output = Vec::new();
    // Act
    runtime()
        .block_on(handle_bind(
            Arc::clone(&cassie),
            &cassie.runtime(),
            &mut output,
            &mut state,
            &operation,
            BindRequest {
                portal: "p".to_owned(),
                statement: "source".to_owned(),
                parameter_formats: Vec::new(),
                parameters: Vec::new(),
                result_formats: Vec::new(),
            },
        ))
        .expect("idle Bind");
    // Assert
    assert_eq!(output, vec![b'2', 0, 0, 0, 4]);
    assert_eq!(session.transaction_status(), "idle");
    assert!(state.portal_owners.belongs_to("p", owner));
    assert!(state.portal_memory_controls.current_query_memory_bytes() > 0);
    state.remove_portal("p");
    assert_eq!(state.portal_memory_controls.current_query_memory_bytes(), 0);
}
