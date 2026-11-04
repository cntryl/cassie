//! Independent projector startup connections share one idempotent DDL result.

use crate::support_pgwire as wire;
use std::time::Duration;

const CREATE: &str = "CREATE TABLE IF NOT EXISTS startup_checkpoint \
    (identity TEXT PRIMARY KEY, cursor TEXT NOT NULL, revision BIGINT NOT NULL)";

fn concurrent_creation(extended: bool, warm: bool) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("current-thread runtime");
    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().expect("memory fixture");
        cassie.startup().expect("startup");
        if warm {
            let session = cassie.create_session("tester", None);
            cassie
                .execute_sql(&session, CREATE, vec![])
                .expect("pre-created table");
        }
        let epoch = cassie.midge.schema_epoch().expect("initial schema epoch");
        let version = cassie.catalog.version();
        let server = wire::spawn_server(cassie.clone()).await;
        let mut connections = Vec::new();
        for _ in 0..12 {
            let connection = tokio::net::TcpStream::connect(server.addr)
                .await
                .expect("connection");
            let (mut reader, mut writer) = connection.into_split();
            // Authenticate before concurrent DDL: authentication failures in the
            // consumer report are a separate, not-yet-isolated observation.
            wire::complete_startup(&mut reader, &mut writer).await;
            connections.push((reader, writer));
        }
        let frames = if extended {
            vec![
                wire::parse_frame("startup", CREATE),
                wire::bind_frame("", "startup", &[]),
                wire::execute_frame(""),
                wire::sync_frame(),
            ]
        } else {
            vec![wire::simple_query_frame(CREATE)]
        };

        // Act
        // Send every independent request before awaiting any query response.
        for (_, writer) in &mut connections {
            wire::write_frames(writer, frames.clone()).await;
        }
        let mut transcripts = Vec::new();
        for (reader, _) in &mut connections {
            transcripts
                .push(wire::read_frames_until_ready_within(reader, Duration::from_secs(10)).await);
        }
        drop(connections);
        server.stop().await;

        // Assert
        for transcript in transcripts {
            assert_eq!(wire::error_code(&transcript), None, "{transcript:?}");
            let commands: Vec<_> = transcript.iter().filter(|(tag, _)| *tag == b'C').collect();
            assert_eq!(commands.len(), 1);
            assert_eq!(commands[0].1, b"CREATE TABLE\0");
            assert_eq!(
                transcript.last().map(|(_, body)| body.as_slice()),
                Some(&b"I"[..])
            );
        }
        assert_eq!(
            cassie
                .catalog
                .get_schema("startup_checkpoint")
                .expect("one table")
                .fields
                .len(),
            3
        );
        if warm {
            assert_eq!(cassie.catalog.version(), version);
            assert_eq!(cassie.midge.schema_epoch().expect("unchanged epoch"), epoch);
        }
    });
}

#[test]
fn should_create_one_table_for_concurrent_simple_startup_sessions() {
    // Arrange
    let extended = false;
    let warm = false;
    // Act
    concurrent_creation(extended, warm);
    // Assert
    // The shared fixture requires CREATE TABLE and ReadyForQuery(I) for all sessions.
}

#[test]
fn should_preserve_existing_table_for_concurrent_simple_startup_sessions() {
    // Arrange
    let extended = false;
    let warm = true;
    // Act
    concurrent_creation(extended, warm);
    // Assert
    // The shared fixture also requires unchanged catalog and schema epochs.
}

#[test]
fn should_create_one_table_for_concurrent_extended_startup_sessions() {
    // Arrange
    let extended = true;
    let warm = false;
    // Act
    concurrent_creation(extended, warm);
    // Assert
    // The shared fixture requires one completion and ReadyForQuery(I) per session.
}

#[test]
fn should_preserve_existing_table_for_concurrent_extended_startup_sessions() {
    // Arrange
    let extended = true;
    let warm = true;
    // Act
    concurrent_creation(extended, warm);
    // Assert
    // The shared fixture requires successful no-ops with no epoch publication.
}
