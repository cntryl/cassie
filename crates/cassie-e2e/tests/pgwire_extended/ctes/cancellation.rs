//! CYCLE query result metadata and same-session recovery (#801).

use std::time::Duration;

use crate::support_pgwire as wire;

fn current_thread_runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
}

const CYCLE_SQL: &str = r"
WITH RECURSIVE edges(source_node,target_node) AS (
    VALUES (1::INT4,2::INT4),(2,3),(3,1)
), walk(node) AS (
    SELECT 1::INT4
    UNION ALL
    SELECT edge.target_node FROM edges AS edge
    JOIN walk AS parent ON edge.source_node = parent.node
) CYCLE node SET is_cycle USING path
SELECT node, is_cycle, path FROM walk
";

type Frames = Vec<(u8, Vec<u8>)>;

async fn run_cycle_query_twice() -> (Frames, Frames) {
    let cassie = crate::support_sql::new_in_memory_cassie().expect("Cassie");
    cassie.startup().expect("start CYCLE query fixture");
    let server = wire::spawn_server(cassie).await;
    let socket = tokio::net::TcpStream::connect(server.addr)
        .await
        .expect("CYCLE query connection");
    let (mut reader, mut writer) = socket.into_split();
    wire::complete_startup_with_backend_key(&mut reader, &mut writer).await;

    wire::write_frames(&mut writer, vec![wire::simple_query_frame(CYCLE_SQL)]).await;
    let first = wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;
    wire::write_frames(&mut writer, vec![wire::simple_query_frame(CYCLE_SQL)]).await;
    let second = wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;

    drop(reader);
    drop(writer);
    server.stop().await;
    (first, second)
}

#[test]
fn should_recover_same_session_after_cycle_query() {
    // Arrange
    let runtime = current_thread_runtime();

    // Act
    let (first, second) = runtime.block_on(run_cycle_query_twice());
    drop(runtime);

    // Assert
    assert_eq!(wire::error_code(&first), None);
    assert_eq!(wire::error_code(&second), None);
    let description = first
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("CYCLE RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        [23, 16, 2287]
    );
    let expected_rows = vec![
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
    ];
    assert_eq!(wire::data_rows(&first), expected_rows);
    assert_eq!(wire::data_rows(&second), expected_rows);
}
