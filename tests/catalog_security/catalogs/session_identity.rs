//! Current SQL text and identity in a trust-style session.

use std::time::Duration;

use cassie::{app::Cassie, CassieRuntimeConfig};

use crate::support_pgwire as wire;

type Frames = Vec<(u8, Vec<u8>)>;

struct FixtureDirectory(String);

impl Drop for FixtureDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn run_queries(label: &str, queries: Vec<String>) -> Vec<Frames> {
    run_queries_with_config(label, queries, CassieRuntimeConfig::default())
}

fn run_queries_with_config(
    label: &str,
    queries: Vec<String>,
    config: CassieRuntimeConfig,
) -> Vec<Frames> {
    let path = crate::support_sql::data_dir(label);
    let _directory = FixtureDirectory(path.clone());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_in_memory_with_config(config).expect("Cassie");
        cassie.startup().expect("startup catalog identity fixture");
        let server = wire::spawn_server(cassie).await;
        let socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("catalog identity connection");
        let (mut reader, mut writer) = tokio::io::split(socket);
        wire::complete_startup(&mut reader, &mut writer).await;
        let mut batches = Vec::with_capacity(queries.len());
        for sql in queries {
            wire::write_frames(&mut writer, vec![wire::simple_query_frame(&sql)]).await;
            batches.push(
                wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await,
            );
        }
        drop(reader);
        drop(writer);
        server.stop().await;
        batches
    })
}

#[test]
fn should_report_the_active_query_given_a_running_statement_when_current_query_is_called() {
    // Arrange
    let sql = "SELECT current_query() AS active_query";

    // Act
    let batches = run_queries_with_config(
        "issue-817-current-query",
        vec![sql.to_owned()],
        CassieRuntimeConfig::default(),
    );

    // Assert
    let result = &batches[0];
    assert_eq!(wire::error_code(result), None);
    let description = result
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("current_query RowDescription");
    assert_eq!(wire::parse_row_description(&description.1)[0].type_oid, 25);
    assert_eq!(wire::data_rows(result), vec![vec![Some(sql.to_owned())]]);
}

#[test]
fn should_reject_session_authorization_change() {
    // Arrange
    let query = "SET SESSION AUTHORIZATION root".to_owned();

    // Act
    let batches = run_queries("issue-817-session-authorization", vec![query]);

    // Assert
    assert_eq!(batches.len(), 1);
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("0A000"));
    assert_eq!(
        wire::data_rows(&batches[0]),
        Vec::<Vec<Option<String>>>::new()
    );
}

#[test]
fn should_report_authenticated_root_session_identity() {
    // Arrange
    let query = "SELECT current_user, session_user".to_owned();

    // Act
    let batches = run_queries("issue-817-session-identity", vec![query]);
    let identity = &batches[0];

    // Assert
    assert_eq!(batches.len(), 1);
    assert_eq!(wire::error_code(identity), None);
    let description = identity
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("session identity RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        [19, 19]
    );
    assert_eq!(
        wire::data_rows(identity),
        vec![vec![Some("root".to_owned()), Some("root".to_owned()),]]
    );
}
