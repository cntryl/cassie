//! Actual wire wrappers must retain the worker's owner through error output.
use super::*;
use crate::config::CassieRuntimeConfig;
use crate::pgwire::protocol::{Portal, PreparedStatement};
use std::time::Duration;
use tokio::io::AsyncWriteExt;

fn fixture() -> (Arc<Cassie>, CassieSession, SessionState) {
    let cassie =
        Arc::new(Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).unwrap());
    cassie.startup().unwrap();
    let session = cassie.create_session("outer_wire", None);
    session.begin_transaction(None).unwrap();
    let mut state = SessionState::new(&cassie.runtime.limits());
    state.session = Some(session.clone());
    state.backend_registration = Some(cassie.runtime.register_pgwire_backend());
    (cassie, session, state)
}

fn replace_transaction(session: &CassieSession) {
    session.rollback_transaction();
    session.begin_transaction(None).unwrap();
}

#[test]
fn should_keep_the_replacement_healthy_after_a_simple_worker_error() {
    // Arrange
    let (cassie, session, mut state) = fixture();
    let parsed = transactions::preflight(&cassie, &session, &["SELECT 1 / 0".into()]).unwrap();
    let _hook = wire_failure_test_hooks::install(&cassie, replace_transaction);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    // Act
    let mut output = Vec::new();
    runtime
        .block_on(simple_execution::run_batch(
            Arc::clone(&cassie),
            &cassie.runtime,
            &mut output,
            &mut state,
            &session,
            (vec!["SELECT 1 / 0".into()], "SELECT 1 / 0".into()),
            parsed,
        ))
        .unwrap();

    // Assert
    assert!(session.is_transaction_active());
    assert!(!session.is_transaction_failed());
    assert!(cassie.execute_sql(&session, "SELECT 1", vec![]).is_ok());
}

#[test]
fn should_keep_the_replacement_healthy_after_an_extended_worker_error() {
    // Arrange
    let (cassie, session, mut state) = fixture();
    let parsed = cassie
        .parse_statement_for_session(Some(&session), "SELECT 1 / 0")
        .unwrap();
    state.prepared_statements.insert(
        "source".into(),
        PreparedStatement {
            id: 1,
            name: "source".into(),
            query: "SELECT 1 / 0".into(),
            sql_fingerprint: crate::runtime::sql_fingerprint(&parsed),
            parsed: Some(parsed),
            parameter_count: 0,
            parameter_types: vec![],
            described: false,
        },
    );
    state.portals.insert(
        "p".into(),
        Portal {
            name: "p".into(),
            statement_name: "source".into(),
            prepared_id: 1,
            params: vec![],
            parameter_memory: vec![],
            result_formats: vec![],
            described: false,
            suspended: None,
            completed_command: None,
        },
    );
    cassie.runtime.record_pgwire_portal_delta(1);
    let _hook = wire_failure_test_hooks::install(&cassie, replace_transaction);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    // Act
    runtime.block_on(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut client = tokio::net::TcpStream::connect(listener.local_addr().unwrap())
            .await
            .unwrap();
        let (server, _) = listener.accept().await.unwrap();
        let transport = crate::transport::TimedWriteTransport::new(
            super::transport::PgwireTransport::Plain(server),
            Duration::from_secs(5),
        );
        let (read, _write) = tokio::io::split(transport);
        let mut reader = tokio::io::BufReader::new(read);
        client
            .write_all(&[b'E', 0, 0, 0, 10, b'p', 0, 0, 0, 0, 0])
            .await
            .unwrap();
        let mut output = Vec::new();
        let mut awaiting_sync = false;
        extended::handle_frontend_message(
            Arc::clone(&cassie),
            &cassie.runtime,
            &mut reader,
            &mut output,
            &mut state,
            &mut awaiting_sync,
            &session,
        )
        .await;
        assert!(awaiting_sync);
    });

    // Assert
    assert!(session.is_transaction_active());
    assert!(!session.is_transaction_failed());
    assert!(cassie.execute_sql(&session, "SELECT 1", vec![]).is_ok());
}
