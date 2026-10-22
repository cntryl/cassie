//! CYCLE recursion cancellation, partial streaming and same-session recovery (#801).

use std::time::{Duration, Instant};

use crate::support_pgwire as wire;
use tokio::io::AsyncWriteExt;

struct CycleFixtureDirectory(String);

impl Drop for CycleFixtureDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn current_thread_runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
}

async fn connect_activity_observer(
    address: std::net::SocketAddr,
) -> (
    tokio::net::tcp::OwnedReadHalf,
    tokio::net::tcp::OwnedWriteHalf,
) {
    let socket = tokio::net::TcpStream::connect(address)
        .await
        .expect("activity observer connection");
    let (mut reader, mut writer) = socket.into_split();
    wire::complete_startup_with_backend_key(&mut reader, &mut writer).await;
    (reader, writer)
}

async fn wait_for_active_query(
    reader: &mut tokio::net::tcp::OwnedReadHalf,
    writer: &mut tokio::net::tcp::OwnedWriteHalf,
    target_pid: i32,
    marker: &str,
) -> bool {
    let deadline = Instant::now() + Duration::from_secs(3);
    let sql =
        format!("SELECT state, query FROM pg_catalog.pg_stat_activity WHERE pid = {target_pid}");
    loop {
        wire::write_frames(writer, vec![wire::simple_query_frame(&sql)]).await;
        let frames = wire::read_frames_until_ready_within(reader, Duration::from_secs(2)).await;
        if wire::error_code(&frames).is_none()
            && wire::data_rows(&frames).iter().any(|row| {
                row[0].as_deref() == Some("active")
                    && row[1]
                        .as_deref()
                        .is_some_and(|query| query.contains(marker))
            })
        {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

const FINITE_CYCLE_SQL: &str = "WITH RECURSIVE edges(source_node,target_node) AS (\
    VALUES (1::INT4,2::INT4),(2,3),(3,1)\
), walk(node) AS (\
    SELECT 1::INT4\
    UNION ALL\
    SELECT edge.target_node FROM edges AS edge\
    JOIN walk AS parent ON edge.source_node = parent.node\
    WHERE pg_sleep_for(INTERVAL '0 seconds') IS NOT NULL\
) CYCLE node SET is_cycle USING path\
SELECT node, is_cycle, path FROM walk";

const SLEEPING_CYCLE_SQL: &str = "WITH RECURSIVE edges(source_node,target_node) AS (\
    VALUES (1::INT4,2::INT4),(2,3),(3,1)\
), walk(node) AS (\
    SELECT 1::INT4\
    UNION ALL\
    SELECT edge.target_node FROM edges AS edge\
    JOIN walk AS parent ON edge.source_node = parent.node\
    WHERE pg_sleep_for(INTERVAL '60 seconds') IS NOT NULL\
) CYCLE node SET is_cycle USING path\
SELECT node, is_cycle, path FROM walk /* issue_801_cycle_cancel */";

type Frames = Vec<(u8, Vec<u8>)>;

struct CycleCancellationEvidence {
    baseline: Frames,
    cancelled: Frames,
    recovery: Frames,
    seed_streamed: bool,
    active_query_observed: bool,
}

async fn cancel_cycle_after_seed_stream(
    address: std::net::SocketAddr,
    reader: &mut tokio::net::tcp::OwnedReadHalf,
    writer: &mut tokio::net::tcp::OwnedWriteHalf,
    observer_reader: &mut tokio::net::tcp::OwnedReadHalf,
    observer_writer: &mut tokio::net::tcp::OwnedWriteHalf,
    process_id: i32,
    secret_key: i32,
) -> (Frames, bool, bool) {
    wire::write_frames(writer, vec![wire::simple_query_frame(SLEEPING_CYCLE_SQL)]).await;
    let mut streamed_prefix = Vec::new();
    let seed_deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    let mut seed_streamed = false;
    let mut query_ended = false;
    while tokio::time::Instant::now() < seed_deadline {
        let remaining = seed_deadline.saturating_duration_since(tokio::time::Instant::now());
        let next = tokio::time::timeout(remaining, wire::read_wire_frame(reader)).await;
        let Ok(frame) = next else {
            break;
        };
        seed_streamed |= frame.0 == b'D';
        query_ended = frame.0 == b'Z';
        streamed_prefix.push(frame);
        if seed_streamed || query_ended {
            break;
        }
    }

    // The seed row is the feature-specific progress witness. Confirm that the
    // same marked query remains active before cancellation.
    let active_query_observed = if seed_streamed {
        wait_for_active_query(
            observer_reader,
            observer_writer,
            process_id,
            "issue_801_cycle_cancel",
        )
        .await
    } else {
        false
    };
    if seed_streamed || !query_ended {
        let mut cancel_socket = tokio::net::TcpStream::connect(address)
            .await
            .expect("cancel connection");
        cancel_socket
            .write_all(&wire::cancel_request_frame(process_id, secret_key))
            .await
            .expect("write CYCLE cancellation request");
        cancel_socket
            .shutdown()
            .await
            .expect("close CYCLE cancellation request");
    }
    let mut cancelled = streamed_prefix;
    if !query_ended {
        cancelled
            .extend(wire::read_frames_until_ready_within(reader, Duration::from_secs(5)).await);
    }
    (cancelled, seed_streamed, active_query_observed)
}

async fn run_cycle_cancellation(_path: &str) -> CycleCancellationEvidence {
    let cassie = crate::support_sql::new_in_memory_cassie().expect("Cassie");
    cassie.startup().expect("start CYCLE cancellation fixture");
    let server = wire::spawn_server(cassie).await;
    let (mut observer_reader, mut observer_writer) = connect_activity_observer(server.addr).await;
    let socket = tokio::net::TcpStream::connect(server.addr)
        .await
        .expect("CYCLE query connection");
    let (mut reader, mut writer) = socket.into_split();
    let (process_id, secret_key) =
        wire::complete_startup_with_backend_key(&mut reader, &mut writer).await;

    wire::write_frames(
        &mut writer,
        vec![wire::simple_query_frame(FINITE_CYCLE_SQL)],
    )
    .await;
    let baseline = wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;
    let (cancelled, seed_streamed, active_query_observed) = cancel_cycle_after_seed_stream(
        server.addr,
        &mut reader,
        &mut writer,
        &mut observer_reader,
        &mut observer_writer,
        process_id,
        secret_key,
    )
    .await;

    wire::write_frames(
        &mut writer,
        vec![wire::simple_query_frame(FINITE_CYCLE_SQL)],
    )
    .await;
    let recovery = wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;

    drop(reader);
    drop(writer);
    drop(observer_reader);
    drop(observer_writer);
    server.stop().await;
    CycleCancellationEvidence {
        baseline,
        cancelled,
        recovery,
        seed_streamed,
        active_query_observed,
    }
}

fn assert_cycle_cancellation(evidence: &CycleCancellationEvidence) {
    assert_eq!(
        wire::error_code(&evidence.baseline),
        None,
        "finite CYCLE ErrorResponse: {:?}",
        evidence
            .baseline
            .iter()
            .filter(|frame| frame.0 == b'E')
            .map(|frame| wire::parse_error_fields(&frame.1))
            .collect::<Vec<_>>()
    );
    let baseline_description = evidence
        .baseline
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("finite CYCLE RowDescription");
    let columns = wire::parse_row_description(&baseline_description.1);
    assert_eq!(
        columns
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        [23, 16, 2287]
    );
    assert_eq!(
        wire::data_rows(&evidence.baseline),
        vec![
            vec![
                Some("1".to_owned()),
                Some("f".to_owned()),
                Some("{(1)}".to_owned()),
            ],
            vec![
                Some("2".to_owned()),
                Some("f".to_owned()),
                Some("{(1),(2)}".to_owned()),
            ],
            vec![
                Some("3".to_owned()),
                Some("f".to_owned()),
                Some("{(1),(2),(3)}".to_owned()),
            ],
            vec![
                Some("1".to_owned()),
                Some("t".to_owned()),
                Some("{(1),(2),(3),(1)}".to_owned()),
            ],
        ]
    );
    assert!(
        evidence.seed_streamed,
        "the CYCLE query must stream its seed row before cancellation"
    );
    assert!(
        evidence.active_query_observed,
        "the marked CYCLE query must remain active after streaming its seed row"
    );
    assert!(
        evidence.cancelled.iter().any(|frame| frame.0 == b'D'),
        "the seed DataRow must be streamed before cancellation; frames: {:?}",
        evidence
            .cancelled
            .iter()
            .map(|frame| frame.0 as char)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        wire::data_rows(&evidence.cancelled),
        vec![vec![
            Some("1".to_owned()),
            Some("f".to_owned()),
            Some("{(1)}".to_owned()),
        ]],
        "only the seed row may precede cancellation"
    );
    assert_eq!(
        wire::error_code(&evidence.cancelled).as_deref(),
        Some("57014"),
        "cancelled CYCLE ErrorResponse: {:?}",
        evidence
            .cancelled
            .iter()
            .filter(|frame| frame.0 == b'E')
            .map(|frame| wire::parse_error_fields(&frame.1))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        evidence
            .cancelled
            .iter()
            .filter(|frame| frame.0 == b'C')
            .count(),
        0
    );
    assert_eq!(evidence.cancelled.last().map(|frame| frame.0), Some(b'Z'));
    assert_eq!(wire::error_code(&evidence.recovery), None);
    assert_eq!(
        wire::data_rows(&evidence.recovery),
        wire::data_rows(&evidence.baseline)
    );
}

#[test]
fn should_return_57014_given_cycle_recursion_has_started_when_cancel_request_arrives() {
    // Arrange
    let path = wire::data_dir("cycle-recursion-cancel-and-recovery");
    let _directory = CycleFixtureDirectory(path.clone());
    let runtime = current_thread_runtime();

    // Act
    let evidence = runtime.block_on(run_cycle_cancellation(&path));
    drop(runtime);

    // Assert
    assert_cycle_cancellation(&evidence);
}
