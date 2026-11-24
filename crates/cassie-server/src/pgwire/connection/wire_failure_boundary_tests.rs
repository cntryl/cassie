//! Actual wire wrappers must retain the worker's owner through error output.
use super::*;
use crate::config::CassieRuntimeConfig;
use crate::pgwire::protocol::{Portal, PreparedStatement};
use std::time::Duration;
use tokio::io::AsyncWriteExt;

fn frontend_frame(tag: u8, body: &[u8]) -> Vec<u8> {
    let mut frame = vec![tag];
    frame.extend_from_slice(&u32::try_from(body.len() + 4).unwrap().to_be_bytes());
    frame.extend_from_slice(body);
    frame
}

fn parse_frame(sql: &str) -> Vec<u8> {
    let mut body = b"source\0".to_vec();
    body.extend_from_slice(sql.as_bytes());
    body.extend_from_slice(&[0, 0, 0]);
    frontend_frame(b'P', &body)
}

fn bind_frame() -> Vec<u8> {
    frontend_frame(b'B', b"p\0source\0\0\0\0\0\0\0")
}

fn execute_frame() -> Vec<u8> {
    frontend_frame(b'E', &[b'p', 0, 0, 0, 0, 0])
}

fn tagged_frames(mut bytes: &[u8]) -> Vec<(u8, &[u8])> {
    let mut frames = Vec::new();
    while !bytes.is_empty() {
        let length = usize::try_from(u32::from_be_bytes(bytes[1..5].try_into().unwrap())).unwrap();
        frames.push((bytes[0], &bytes[5..=length]));
        bytes = &bytes[length + 1..];
    }
    frames
}

async fn handle_frame(
    cassie: &Arc<Cassie>,
    state: &mut SessionState,
    frame: &[u8],
    awaiting_sync: &mut bool,
) -> Vec<u8> {
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
    client.write_all(frame).await.unwrap();
    let mut output = Vec::new();
    let step = super::handle_ready(
        Arc::clone(cassie),
        &cassie.runtime,
        &mut reader,
        &mut output,
        state,
        awaiting_sync,
    )
    .await;
    assert!(matches!(
        step,
        ConnectionStep::Continue(HandshakeState::Ready)
    ));
    output
}

#[test]
fn should_preserve_a_named_preparation_portal_across_its_own_explicit_begin() {
    // Arrange
    let (cassie, session, mut state) = fixture();
    session.rollback_transaction();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let mut awaiting = false;
    runtime.block_on(async {
        handle_frame(&cassie, &mut state, &parse_frame("SELECT 7"), &mut awaiting).await;
        handle_frame(&cassie, &mut state, &bind_frame(), &mut awaiting).await;
    });

    // Act
    let (begin, execute, sync) = runtime.block_on(async {
        let begin = handle_frame(
            &cassie,
            &mut state,
            &frontend_frame(b'Q', b"BEGIN\0"),
            &mut awaiting,
        )
        .await;
        let execute = handle_frame(&cassie, &mut state, &execute_frame(), &mut awaiting).await;
        let sync = handle_frame(
            &cassie,
            &mut state,
            &frontend_frame(b'S', &[]),
            &mut awaiting,
        )
        .await;
        (begin, execute, sync)
    });

    // Assert
    assert!(tagged_frames(&begin)
        .iter()
        .any(|(tag, body)| *tag == b'Z' && *body == b"T"));
    assert!(tagged_frames(&execute).iter().any(|(tag, _)| *tag == b'D'));
    assert!(tagged_frames(&sync)
        .iter()
        .any(|(tag, body)| *tag == b'Z' && *body == b"T"));
    assert!(state.portals.contains_key("p"));
}

#[test]
fn should_adopt_idle_bound_portals_only_for_their_own_implicit_execute() {
    // Arrange
    let (cassie, session, mut state) = fixture();
    session.rollback_transaction();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    // Act
    let (before, after, output, sync) = runtime.block_on(async {
        let mut awaiting = false;
        handle_frame(&cassie, &mut state, &parse_frame("SELECT 7"), &mut awaiting).await;
        handle_frame(&cassie, &mut state, &bind_frame(), &mut awaiting).await;
        let before = state.portal_owner_identity("p").unwrap();
        let output = handle_frame(&cassie, &mut state, &execute_frame(), &mut awaiting).await;
        let after = state.portal_owner_identity("p").unwrap();
        let sync = handle_frame(
            &cassie,
            &mut state,
            &frontend_frame(b'S', &[]),
            &mut awaiting,
        )
        .await;
        (before, after, output, sync)
    });

    // Assert
    assert!(before.transaction.is_none());
    assert!(after.transaction.is_some());
    assert_ne!(before.generation, after.generation);
    assert!(tagged_frames(&output)
        .iter()
        .any(|(tag, value)| *tag == b'D' && *value == [0, 1, 0, 0, 0, 1, b'7']));
    assert_eq!(
        tagged_frames(&sync)
            .iter()
            .filter(|(tag, _)| *tag == b'Z')
            .map(|(_, value)| value[0])
            .collect::<Vec<_>>(),
        [b'I']
    );
    assert!(!state.portals.contains_key("p"));
}

#[test]
fn should_reject_a_bound_a_execute_without_failing_b_before_worker_capture() {
    // Arrange
    let (cassie, session, mut state) = fixture();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let mut awaiting = false;
    runtime.block_on(async {
        handle_frame(&cassie, &mut state, &parse_frame("SELECT 7"), &mut awaiting).await;
        handle_frame(&cassie, &mut state, &bind_frame(), &mut awaiting).await;
    });
    let bound_a = state.portal_owner_identity("p").unwrap();
    session.rollback_transaction();
    session.begin_transaction(None).unwrap();
    let owner_b = session.transaction_unique_owner_id();

    // Act
    let output = runtime.block_on(handle_frame(
        &cassie,
        &mut state,
        &execute_frame(),
        &mut awaiting,
    ));

    // Assert
    assert!(output.windows(7).any(|value| value == b"C25000\0"));
    assert!(!tagged_frames(&output).iter().any(|(tag, _)| *tag == b'D'));
    assert_ne!(bound_a.transaction, Some(owner_b));
    assert_eq!(session.transaction_unique_owner_id(), owner_b);
    assert!(session.is_transaction_active());
    assert!(!session.is_transaction_failed());
}

#[test]
fn should_keep_a_bound_portal_alive_across_its_own_savepoint_epoch() {
    // Arrange
    let (cassie, session, mut state) = fixture();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let mut awaiting = false;
    runtime.block_on(async {
        handle_frame(&cassie, &mut state, &parse_frame("SELECT 7"), &mut awaiting).await;
        handle_frame(&cassie, &mut state, &bind_frame(), &mut awaiting).await;
    });
    let bound = state.portal_owner_identity("p").unwrap();
    cassie.execute_sql(&session, "SAVEPOINT s", vec![]).unwrap();
    cassie
        .execute_sql(&session, "ROLLBACK TO SAVEPOINT s", vec![])
        .unwrap();

    // Act
    let output = runtime.block_on(handle_frame(
        &cassie,
        &mut state,
        &execute_frame(),
        &mut awaiting,
    ));
    let synced = runtime.block_on(handle_frame(
        &cassie,
        &mut state,
        &frontend_frame(b'S', &[]),
        &mut awaiting,
    ));

    // Assert
    assert_ne!(session.rollback_epoch_for_test(), bound.rollback_epoch);
    assert!(tagged_frames(&output)
        .iter()
        .any(|(tag, value)| *tag == b'D' && *value == [0, 1, 0, 0, 0, 1, b'7']));
    assert_eq!(
        tagged_frames(&synced)
            .iter()
            .filter(|(tag, _)| *tag == b'Z')
            .map(|(_, value)| value[0])
            .collect::<Vec<_>>(),
        [b'T']
    );
    assert!(state.portals.contains_key("p"));
}

#[test]
fn should_finish_an_unsynced_implicit_write_on_empty_simple_query() {
    // Arrange
    let (cassie, session, mut state) = fixture();
    session.rollback_transaction();
    cassie
        .execute_sql(&session, "CREATE TABLE empty_handoff_rows (id INT)", vec![])
        .unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let mut awaiting = false;
    runtime.block_on(async {
        handle_frame(
            &cassie,
            &mut state,
            &parse_frame("INSERT INTO empty_handoff_rows VALUES (7)"),
            &mut awaiting,
        )
        .await;
        handle_frame(&cassie, &mut state, &bind_frame(), &mut awaiting).await;
        handle_frame(&cassie, &mut state, &execute_frame(), &mut awaiting).await;
    });
    assert!(session.is_implicit_transaction());

    // Act
    let output = runtime.block_on(handle_frame(
        &cassie,
        &mut state,
        &frontend_frame(b'Q', &[0]),
        &mut awaiting,
    ));
    let observer = cassie.create_session("handoff_observer", None);
    let rows = cassie
        .execute_sql(&observer, "SELECT id FROM empty_handoff_rows", vec![])
        .unwrap();

    // Assert
    assert_eq!(rows.rows(), &[vec![crate::types::Value::Int64(7)]]);
    assert_eq!(
        tagged_frames(&output)
            .iter()
            .filter(|(tag, _)| *tag == b'Z')
            .map(|(_, value)| value[0])
            .collect::<Vec<_>>(),
        [b'I']
    );
    assert_eq!(
        tagged_frames(&output)
            .iter()
            .filter(|(tag, _)| *tag == b'I')
            .count(),
        1
    );
}

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
    let original = session
        .operation_failure_authority()
        .and_then(crate::app::FailureAuthority::identity);
    let independent = session.without_operation_failure_authority();
    independent.rollback_transaction();
    independent.begin_transaction(None).unwrap();
    assert_eq!(
        session
            .operation_failure_authority()
            .and_then(crate::app::FailureAuthority::identity),
        original
    );
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

#[test]
fn should_cleanup_an_own_implicit_origin_when_the_worker_slot_is_saturated() {
    // Arrange
    let mut config = CassieRuntimeConfig::default();
    config.limits.max_query_workers = 1;
    let cassie = Cassie::new_in_memory_with_config(config).unwrap();
    cassie.startup().unwrap();
    let root = cassie.create_session("saturated_origin", None);
    let operation = root.with_new_wire_operation();
    operation
        .begin_implicit_transaction_for_operation(operation.operation_failure_authority())
        .unwrap();
    let receipt = operation
        .operation_failure_authority()
        .unwrap()
        .portal_adoption();
    let occupied = cassie.runtime.try_begin_running_query().unwrap();

    // Act
    let result = cassie.execute_pgwire_sql_with_cancellation(
        &operation,
        ("SELECT 7", "SELECT 7"),
        &crate::runtime::QueryCancellationHandle::new(),
    );
    let retained = operation
        .operation_failure_authority()
        .unwrap()
        .portal_adoption();
    operation.fail_operation_transaction();
    drop(occupied);

    // Assert
    assert!(
        matches!(result, Err(CassieError::Execution(ref message)) if message == "query admission exhausted")
    );
    assert!(receipt.is_some());
    assert_eq!(receipt, retained);
    assert!(!root.is_transaction_active());
    assert!(!root.is_transaction_failed());
}

#[test]
fn should_not_fail_b_when_a_captured_operation_later_hits_saturation() {
    // Arrange
    let mut config = CassieRuntimeConfig::default();
    config.limits.max_query_workers = 1;
    let cassie = Cassie::new_in_memory_with_config(config).unwrap();
    cassie.startup().unwrap();
    let root = cassie.create_session("saturated_replacement", None);
    root.begin_transaction(None).unwrap();
    let operation = root.with_new_wire_operation();
    let captured = crate::app::FailureAuthority::capture_for_operation(&operation).identity();
    replace_transaction(&root);
    let current = root.transaction_unique_owner_id();
    let occupied = cassie.runtime.try_begin_running_query().unwrap();

    // Act
    let result = cassie.execute_pgwire_sql_with_cancellation(
        &operation,
        ("SELECT 7", "SELECT 7"),
        &crate::runtime::QueryCancellationHandle::new(),
    );
    operation.fail_operation_transaction();
    drop(occupied);

    // Assert
    assert!(
        matches!(result, Err(CassieError::Execution(ref message)) if message == "query admission exhausted")
    );
    assert_eq!(
        operation.operation_failure_authority().unwrap().identity(),
        captured
    );
    assert_eq!(root.transaction_unique_owner_id(), current);
    assert!(root.is_transaction_active());
    assert!(!root.is_transaction_failed());
}

#[test]
fn should_return_an_uncaptured_saturation_error_while_the_root_is_contended() {
    // Arrange
    let mut config = CassieRuntimeConfig::default();
    config.limits.max_query_workers = 1;
    let cassie = Arc::new(Cassie::new_in_memory_with_config(config).unwrap());
    cassie.startup().unwrap();
    let root = cassie.create_session("saturated_contended", None);
    root.begin_transaction(None).unwrap();
    let operation = root.with_new_wire_operation();
    let occupied = cassie.runtime.try_begin_running_query().unwrap();
    let (held_tx, held_rx) = std::sync::mpsc::sync_channel(1);
    let (release_tx, release_rx) = std::sync::mpsc::sync_channel(1);
    let holding_root = root.clone();
    let holder = std::thread::spawn(move || {
        holding_root.hold_transaction_for_test(
            &held_tx,
            &release_rx,
            Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );
    });
    held_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let (done_tx, done_rx) = std::sync::mpsc::sync_channel(1);
    let worker_cassie = Arc::clone(&cassie);
    let worker = std::thread::spawn(move || {
        let result = worker_cassie.execute_pgwire_sql_with_cancellation(
            &operation,
            ("SELECT 7", "SELECT 7"),
            &crate::runtime::QueryCancellationHandle::new(),
        );
        operation.fail_operation_transaction();
        done_tx
            .send((
                result,
                operation.operation_failure_authority().unwrap().identity(),
            ))
            .unwrap();
    });

    // Act
    let completed_without_root = done_rx.recv_timeout(Duration::from_secs(5));
    release_tx.send(()).unwrap();
    holder.join().unwrap();
    worker.join().unwrap();
    drop(occupied);

    // Assert
    let (result, authority) = completed_without_root
        .expect("uncaptured rejection and outer failure must not await root capture");
    assert!(
        matches!(result, Err(CassieError::Execution(ref message)) if message == "query admission exhausted")
    );
    assert!(authority.is_none());
    assert!(root.is_transaction_active());
    assert!(!root.is_transaction_failed());
}
