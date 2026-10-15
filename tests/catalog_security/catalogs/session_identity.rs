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
    crate::support_sql::use_local_storage();
    let path = crate::support_sql::data_dir(label);
    let _directory = FixtureDirectory(path.clone());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        // Arrange a passwordless loopback fixture so PostgreSQL's `trust`
        // authentication profile is the relevant oracle for `system_user`.
        let config = CassieRuntimeConfig {
            password: String::new(),
            ..CassieRuntimeConfig::default()
        };
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).expect("Cassie");
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
    let batches = run_queries("issue-817-current-query", vec![sql.to_owned()]);

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
fn should_report_trust_session_identity_given_session_authorization_when_user_functions_are_read() {
    // Arrange
    let role = "issue_817_session_identity";

    // Act
    let batches = run_queries(
        "issue-817-session-identity",
        vec![
            format!("CREATE ROLE {role}"),
            format!("SET SESSION AUTHORIZATION {role}"),
            "SELECT USER, current_user, session_user, system_user IS NULL \
             AS system_user_is_unavailable"
                .to_owned(),
            "RESET SESSION AUTHORIZATION".to_owned(),
            format!("DROP ROLE {role}"),
        ],
    );

    // Assert
    assert_eq!(batches.len(), 5);
    assert!(batches[..2]
        .iter()
        .all(|frames| wire::error_code(frames).is_none()));
    let identity = &batches[2];
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
        [19, 19, 19, 16]
    );
    assert_eq!(
        wire::data_rows(identity),
        vec![vec![
            Some(role.to_owned()),
            Some(role.to_owned()),
            Some(role.to_owned()),
            Some("t".to_owned()),
        ]]
    );
    assert!(batches[3..]
        .iter()
        .all(|frames| wire::error_code(frames).is_none()));
}
