//! A materialized graph portal validates its complete wire output before its first page.

use crate::support_pgwire as wire;
use cassie::app::Cassie;
use cassie::config::CassieRuntimeConfig;

type Frames = Vec<(u8, Vec<u8>)>;

const SETUP: &[&str] = &[
    "CREATE TABLE v(id BIGINT PRIMARY KEY)",
    "CREATE TABLE e(id BIGINT PRIMARY KEY,a BIGINT,b BIGINT)",
    "INSERT INTO v VALUES(1),(2)",
    "CREATE PROPERTY GRAPH g VERTEX TABLES(v AS n KEY(id) LABEL node PROPERTIES(id)) \
     EDGE TABLES(e AS l KEY(id) SOURCE KEY(a) REFERENCES n(id) \
     DESTINATION KEY(b) REFERENCES n(id) LABEL link PROPERTIES(id))",
];

fn pages(sql: &str) -> Vec<Frames> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("current-thread wire runtime");
    runtime.block_on(async {
        let mut config = crate::support_sql::with_test_root_password(
            CassieRuntimeConfig::from_env().expect("configuration"),
        );
        config.limits.query_memory_budget_bytes = 2 * 1024 * 1024 * 1024;
        let cassie = Cassie::new_in_memory_with_config(config).expect("memory fixture");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        for sql in SETUP {
            cassie
                .execute_sql(&session, sql, vec![])
                .expect("seed graph");
        }
        let server = wire::spawn_server(cassie).await;
        let socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("connect");
        let (mut reader, mut writer) = tokio::io::split(socket);
        wire::complete_startup(&mut reader, &mut writer).await;
        let cycles = vec![
            vec![wire::simple_query_frame("BEGIN")],
            vec![
                wire::parse_frame("graph", sql),
                wire::bind_frame("page", "graph", &[]),
                wire::execute_limited_frame("page", 1),
                wire::sync_frame(),
            ],
            vec![wire::execute_limited_frame("page", 1), wire::sync_frame()],
            vec![wire::simple_query_frame("ROLLBACK")],
        ];
        let mut result = Vec::new();
        for cycle in cycles {
            wire::write_frames(&mut writer, cycle).await;
            result.push(
                wire::read_frames_until_ready_within(
                    &mut reader,
                    std::time::Duration::from_secs(10),
                )
                .await,
            );
        }
        drop(reader);
        drop(writer);
        server.stop().await;
        result
    })
}

#[test]
fn should_reject_a_late_unencodable_graph_row_before_the_first_portal_page() {
    // Arrange
    // Two rows cross the fixed backend frame boundary; this exercises the resource
    // contract without a throughput workload or a scheduling race.
    let query = "SELECT t.id,t.body FROM GRAPH_TABLE(g MATCH(v IS node) COLUMNS(v.id AS id, \
        CASE WHEN v.id=1 THEN 'ok' ELSE repeat('x',16777216) END AS body)) AS t ORDER BY t.id";

    // Act
    let frames = pages(query);

    // Assert
    assert_eq!(wire::error_code(&frames[1]).as_deref(), Some("54000"));
    let message = frames[1]
        .iter()
        .find(|(tag, _)| *tag == b'E')
        .and_then(|(_, body)| {
            wire::parse_error_fields(body)
                .into_iter()
                .find(|(kind, _)| *kind == 'M')
        })
        .map(|(_, message)| message)
        .expect("encoding error message");
    assert!(
        message.contains("backend frame exceeds"),
        "must reach the wire boundary: {message}"
    );
    assert!(
        !frames[1]
            .iter()
            .any(|(tag, _)| matches!(*tag, b'T' | b'D' | b's' | b'C')),
        "no successful graph page may precede a later row's encoding failure"
    );
    assert_eq!(
        frames[1].last().map(|(_, body)| body.as_slice()),
        Some(&b"E"[..])
    );
    assert_eq!(
        frames[3].last().map(|(_, body)| body.as_slice()),
        Some(&b"I"[..])
    );
}

#[test]
fn should_resume_a_valid_preflighted_graph_portal_with_one_description() {
    // Arrange
    let query =
        "SELECT t.id FROM GRAPH_TABLE(g MATCH(v IS node) COLUMNS(v.id AS id)) AS t ORDER BY t.id";

    // Act
    let frames = pages(query);

    // Assert
    assert_eq!(wire::error_code(&frames[1]), None);
    assert_eq!(wire::error_code(&frames[2]), None);
    assert_eq!(
        wire::data_rows(&frames[1]),
        vec![vec![Some("1".to_owned())]]
    );
    assert_eq!(
        wire::data_rows(&frames[2]),
        vec![vec![Some("2".to_owned())]]
    );
    assert!(frames[1].iter().any(|(tag, _)| *tag == b's'));
    assert!(frames[2].iter().any(|(tag, _)| *tag == b'C'));
    assert!(!frames[2].iter().any(|(tag, _)| *tag == b'T'));
}
