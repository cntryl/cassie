//! Owner-filtered portal cleanup preserves the transaction across savepoint recovery.

use super::*;
use crate::app::{CassieSession, SessionTransactionStatus};
use crate::config::CassieRuntimeConfig;
use crate::pgwire::protocol::Portal;
use std::sync::mpsc;
use std::time::Duration;
use tokio::io::AsyncWriteExt;

#[derive(Clone, Copy)]
enum Site {
    Direct,
    Sync,
    Recovery,
    SyncWait,
}

fn fixture() -> (Arc<Cassie>, CassieSession, SessionState) {
    let cassie = Arc::new(
        Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).expect("memory fixture"),
    );
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    let mut state = SessionState::new(&cassie.runtime.limits());
    state.session = Some(session.clone());
    state.portals.insert(
        "retained".to_owned(),
        Portal {
            name: "retained".to_owned(),
            statement_name: "source".to_owned(),
            prepared_id: 1,
            params: Vec::new(),
            parameter_memory: Vec::new(),
            result_formats: Vec::new(),
            described: false,
            suspended: None,
            completed_command: None,
        },
    );
    cassie.runtime.record_pgwire_portal_delta(1);
    stamp_current_portal(&session, &mut state);
    (cassie, session, state)
}

fn stamp_current_portal(session: &CassieSession, state: &mut SessionState) {
    let identity = crate::app::FailureAuthority::capture(session)
        .identity()
        .unwrap();
    let owner = state
        .portal_owners
        .prepare("retained", identity, &state.portal_memory_controls)
        .unwrap();
    state.portal_owners.publish(owner);
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("current-thread runtime")
}

fn parse_frame(sql: &str) -> Vec<u8> {
    let mut body = vec![0];
    body.extend_from_slice(sql.as_bytes());
    body.extend_from_slice(&[0, 0, 0]);
    let mut frame = vec![b'P'];
    frame.extend_from_slice(
        &u32::try_from(body.len() + 4)
            .expect("frame size")
            .to_be_bytes(),
    );
    frame.extend_from_slice(&body);
    frame
}

fn frames(bytes: &[u8]) -> Vec<(u8, Vec<u8>)> {
    let mut remaining = bytes;
    let mut result = Vec::new();
    while !remaining.is_empty() {
        assert!(remaining.len() >= 5, "complete frame header");
        let size = usize::try_from(u32::from_be_bytes(
            remaining[1..5].try_into().expect("frame length"),
        ))
        .expect("frame length fits usize");
        assert!(size >= 4 && remaining.len() > size, "complete frame");
        result.push((remaining[0], remaining[5..=size].to_vec()));
        remaining = &remaining[size + 1..];
    }
    result
}

fn error_code(frames: &[(u8, Vec<u8>)]) -> Option<&[u8]> {
    frames
        .iter()
        .find(|(tag, _)| *tag == b'E')
        .and_then(|(_, body)| {
            body.split(|byte| *byte == 0)
                .find_map(|field| field.strip_prefix(b"C"))
        })
}

async fn handle_frame(
    cassie: &Arc<Cassie>,
    state: &mut SessionState,
    frame: &[u8],
    awaiting_sync: &mut bool,
) -> Vec<u8> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener");
    let mut client = tokio::net::TcpStream::connect(listener.local_addr().expect("address"))
        .await
        .expect("client");
    let (server, _) = listener.accept().await.expect("accepted connection");
    let transport = crate::transport::TimedWriteTransport::new(
        super::transport::PgwireTransport::Plain(server),
        Duration::from_secs(5),
    );
    let (read, _write) = tokio::io::split(transport);
    let mut reader = tokio::io::BufReader::new(read);
    client.write_all(frame).await.expect("send frame");
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

fn assert_failed_snapshot_retains_portal(site: Site) {
    let (cassie, session, mut state) = fixture();
    session.begin_transaction(None).expect("begin");
    stamp_current_portal(&session, &mut state);
    session.create_savepoint("s").expect("savepoint");
    session.mark_transaction_failed();
    let owner = session.transaction_unique_owner_id();
    if matches!(site, Site::Recovery) {
        cassie.midge.latch_storage_recovery_required_for_test();
    }
    let (observed_tx, observed_rx) = mpsc::sync_channel(1);
    let (resume_tx, resume_rx) = mpsc::sync_channel(1);
    let coordinator_session = session.clone();
    let coordinator = std::thread::spawn(move || {
        let observed = observed_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("actual status snapshot");
        coordinator_session
            .rollback_to_savepoint("s")
            .expect("savepoint recovery");
        resume_tx.send(()).expect("resume cleanup");
        observed
    });
    let _hook =
        super::transactions::portal_snapshot_test_hooks::install(&session, observed_tx, resume_rx);
    let runtime = runtime();
    let mut awaiting_sync = matches!(site, Site::SyncWait);
    let output = match site {
        Site::Direct => {
            super::transactions::clear_idle_portals(&cassie.runtime, &mut state, &session);
            Vec::new()
        }
        Site::Sync | Site::SyncWait => runtime.block_on(handle_frame(
            &cassie,
            &mut state,
            &[b'S', 0, 0, 0, 4],
            &mut awaiting_sync,
        )),
        Site::Recovery => runtime.block_on(handle_frame(
            &cassie,
            &mut state,
            &parse_frame("SELECT 1"),
            &mut awaiting_sync,
        )),
    };
    let observed = coordinator.join().expect("coordinator");
    let current = session.transaction_unique_owner_id();
    let status = session.transaction_status();
    let retained = state.portals.contains_key("retained");
    if matches!(site, Site::Recovery) {
        let decoded = frames(&output);
        assert!(awaiting_sync);
        assert_eq!(decoded.iter().filter(|(tag, _)| *tag == b'E').count(), 1);
        assert_eq!(error_code(&decoded), Some(&b"58030"[..]));
        assert!(!decoded.iter().any(|(tag, _)| *tag == b'Z'));
    } else if matches!(site, Site::Sync | Site::SyncWait) {
        let decoded = frames(&output);
        assert!(!awaiting_sync);
        assert_eq!(decoded.iter().filter(|(tag, _)| *tag == b'Z').count(), 1);
        assert_eq!(decoded.last(), Some(&(b'Z', vec![b'T'])));
    }
    session.rollback_transaction();
    assert_eq!(observed, SessionTransactionStatus::Failed);
    assert_eq!(current, owner);
    assert_eq!(status, "in_transaction");
    assert!(retained);
    assert_eq!(cassie.runtime.snapshot().pgwire.portals, 1);
}

#[test]
fn should_retain_portal_after_direct_failed_snapshot() {
    const CASE: &str = "idle_portal_direct";
    if !crate::app::query_failure_watchdog::enter_child("pgwire::connection::idle_portal_snapshot_tests::should_retain_portal_after_direct_failed_snapshot", CASE) { return; }
    // Arrange: the helper starts a Failed transaction with a savepoint and portal.
    // Act: it restores the savepoint after the actual snapshot.
    // Assert: the helper verifies the Failed snapshot retained the portal.
    assert_failed_snapshot_retains_portal(Site::Direct);
    crate::app::query_failure_watchdog::complete_child(CASE);
}

#[test]
fn should_retain_portal_after_extended_sync_failed_snapshot() {
    const CASE: &str = "idle_portal_extended_sync";
    if !crate::app::query_failure_watchdog::enter_child("pgwire::connection::idle_portal_snapshot_tests::should_retain_portal_after_extended_sync_failed_snapshot", CASE) { return; }
    // Arrange: the helper starts a Failed transaction with a savepoint and portal.
    // Act: real extended Sync observes Failed before the savepoint is restored.
    // Assert: the helper verifies the portal and the Ready T frame.
    assert_failed_snapshot_retains_portal(Site::Sync);
    crate::app::query_failure_watchdog::complete_child(CASE);
}

#[test]
fn should_retain_portal_after_recovery_error_failed_snapshot() {
    const CASE: &str = "idle_portal_recovery";
    if !crate::app::query_failure_watchdog::enter_child("pgwire::connection::idle_portal_snapshot_tests::should_retain_portal_after_recovery_error_failed_snapshot", CASE) { return; }
    // Arrange: the helper latches recovery with a Failed transaction and portal.
    // Act: real extended Parse observes Failed before the savepoint is restored.
    // Assert: the helper verifies portal retention and storage error 58030.
    assert_failed_snapshot_retains_portal(Site::Recovery);
    crate::app::query_failure_watchdog::complete_child(CASE);
}

#[test]
fn should_retain_portal_after_sync_wait_failed_snapshot() {
    const CASE: &str = "idle_portal_sync_wait";
    if !crate::app::query_failure_watchdog::enter_child("pgwire::connection::idle_portal_snapshot_tests::should_retain_portal_after_sync_wait_failed_snapshot", CASE) { return; }
    // Arrange: the helper starts a Failed transaction with a savepoint and portal.
    // Act: real Sync-wait observes Failed before the savepoint is restored.
    // Assert: the helper verifies the portal and Ready T frame.
    assert_failed_snapshot_retains_portal(Site::SyncWait);
    crate::app::query_failure_watchdog::complete_child(CASE);
}

#[test]
fn should_clear_idle_portals_only_after_idle_snapshot() {
    // Arrange
    let (cassie, session, mut state) = fixture();

    // Act
    transactions::clear_idle_portals(&cassie.runtime, &mut state, &session);

    // Assert
    assert!(state.portals.is_empty());
    assert_eq!(cassie.runtime.snapshot().pgwire.portals, 0);
}

#[test]
fn should_retain_portals_after_active_snapshot() {
    // Arrange
    let (cassie, session, mut state) = fixture();
    session.begin_transaction(None).expect("begin");
    stamp_current_portal(&session, &mut state);

    // Act
    transactions::clear_idle_portals(&cassie.runtime, &mut state, &session);
    session.rollback_transaction();

    // Assert
    assert!(state.portals.contains_key("retained"));
    assert_eq!(cassie.runtime.snapshot().pgwire.portals, 1);
}

#[test]
fn should_retain_portals_after_failed_snapshot() {
    // Arrange
    let (cassie, session, mut state) = fixture();
    session.begin_transaction(None).expect("begin");
    stamp_current_portal(&session, &mut state);
    session.mark_transaction_failed();

    // Act
    transactions::clear_idle_portals(&cassie.runtime, &mut state, &session);
    session.rollback_transaction();

    // Assert
    assert!(state.portals.contains_key("retained"));
    assert_eq!(cassie.runtime.snapshot().pgwire.portals, 1);
}

#[test]
fn should_clear_portals_when_sync_invalidation_is_set() {
    const CASE: &str = "sync_wait_invalidate";
    if !crate::app::query_failure_watchdog::enter_child("pgwire::connection::idle_portal_snapshot_tests::should_clear_portals_when_sync_invalidation_is_set", CASE) { return; }
    {
        // Arrange: test both active and Failed with the explicit invalidation flag.
        for failed in [false, true] {
            let (cassie, session, mut state) = fixture();
            session.begin_transaction(None).expect("begin");
            stamp_current_portal(&session, &mut state);
            if failed {
                session.mark_transaction_failed();
            }
            state.invalidate_portals_on_sync = true;
            state.portal_invalidation_owner = Some(crate::app::FailureAuthority::capture(&session));
            let (observed_tx, observed_rx) = mpsc::sync_channel(1);
            let (resume_tx, resume_rx) = mpsc::sync_channel(1);
            resume_tx
                .send(())
                .expect("pre-release any erroneous snapshot");
            let hook = super::transactions::portal_snapshot_test_hooks::install(
                &session,
                observed_tx,
                resume_rx,
            );
            let runtime = runtime();
            let mut awaiting_sync = true;

            // Act: use the actual Sync-wait handler.
            let output = runtime.block_on(handle_frame(
                &cassie,
                &mut state,
                &[b'S', 0, 0, 0, 4],
                &mut awaiting_sync,
            ));
            let snapshot = observed_rx.try_recv();
            drop(hook);
            session.rollback_transaction();

            // Assert: captured invalidation consumes the flag and clears its own portals without a snapshot.
            let decoded = frames(&output);
            assert!(state.portals.is_empty());
            assert_eq!(cassie.runtime.snapshot().pgwire.portals, 0);
            assert!(!state.invalidate_portals_on_sync);
            assert!(!awaiting_sync);
            assert_eq!(decoded.iter().filter(|(tag, _)| *tag == b'Z').count(), 1);
            assert_eq!(
                decoded.last(),
                Some(&(b'Z', vec![if failed { b'E' } else { b'T' }]))
            );
            assert!(matches!(snapshot, Err(mpsc::TryRecvError::Empty)));
        }
    }
    crate::app::query_failure_watchdog::complete_child(CASE);
}

#[test]
fn should_skip_recovery_portal_check_when_storage_is_healthy() {
    const CASE: &str = "healthy_error_no_idle_check";
    if !crate::app::query_failure_watchdog::enter_child("pgwire::connection::idle_portal_snapshot_tests::should_skip_recovery_portal_check_when_storage_is_healthy", CASE) { return; }
    {
        // Arrange: an unsupported Parse yields an error with healthy storage.
        let (cassie, session, mut state) = fixture();
        session.begin_transaction(None).expect("begin");
        stamp_current_portal(&session, &mut state);
        let (observed_tx, observed_rx) = mpsc::sync_channel(1);
        let (resume_tx, resume_rx) = mpsc::sync_channel(1);
        resume_tx
            .send(())
            .expect("pre-release any erroneous snapshot");
        let hook = super::transactions::portal_snapshot_test_hooks::install(
            &session,
            observed_tx,
            resume_rx,
        );
        let runtime = runtime();
        let mut awaiting_sync = false;

        // Act
        let output = runtime.block_on(handle_frame(
            &cassie,
            &mut state,
            &parse_frame("LISTEN channel"),
            &mut awaiting_sync,
        ));
        let snapshot = observed_rx.try_recv();
        drop(hook);
        let failed = session.is_transaction_failed();
        session.rollback_transaction();

        // Assert: no recovery-only idle snapshot was taken or portal removed.
        let decoded = frames(&output);
        assert!(matches!(snapshot, Err(mpsc::TryRecvError::Empty)));
        assert!(state.portals.contains_key("retained"));
        assert_eq!(cassie.runtime.snapshot().pgwire.portals, 1);
        assert!(failed);
        assert!(awaiting_sync);
        assert_eq!(decoded.iter().filter(|(tag, _)| *tag == b'E').count(), 1);
        assert_eq!(error_code(&decoded), Some(&b"0A000"[..]));
        assert!(!decoded.iter().any(|(tag, _)| *tag == b'Z'));
    }
    crate::app::query_failure_watchdog::complete_child(CASE);
}
