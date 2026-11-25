//! Finite Parse metadata admission and failed-publication recovery.

use std::sync::Arc;
use std::time::Duration;

use super::support_pgwire as wire;
use super::support_sql_fixture::sql_fixture_with_config;
use cassie::config::CassieRuntimeConfig;

#[test]
fn should_reject_parse_metadata_before_publication_when_shared_budget_is_exceeded() {
    // Arrange
    let mut config = crate::support_sql::with_test_root_password(
        CassieRuntimeConfig::from_env().expect("runtime config"),
    );
    let budget_bytes = 8_192;
    config.limits.query_memory_budget_bytes = budget_bytes;
    let fixture = sql_fixture_with_config("parse-metadata-budget", &[], config.clone());
    fixture.cassie.startup().expect("startup");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let statement = "metadata_budget";
    let parameter_types = vec![23; 4_096];
    let password = config.password.clone();

    // Act
    let (first, ignored, reparsed) = runtime.block_on(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let address = listener.local_addr().expect("listener address");
        let server = tokio::spawn(cassie::pgwire::server::run_with_listener(
            listener,
            Arc::new(fixture.cassie.clone()),
            config,
        ));
        let socket = tokio::net::TcpStream::connect(address)
            .await
            .expect("connect pgwire");
        let (mut reader, mut writer) = tokio::io::split(socket);
        wire::complete_startup_with_password(&mut reader, &mut writer, &password).await;

        wire::write_frames(
            &mut writer,
            vec![
                wire::parse_frame_with_types(
                    statement,
                    "SELECT CAST($4096 AS INT)",
                    &parameter_types,
                ),
                wire::flush_frame(),
            ],
        )
        .await;
        let first =
            tokio::time::timeout(Duration::from_secs(5), wire::read_wire_frame(&mut reader))
                .await
                .expect("Parse+Flush must reply before any Sync");

        // A rejected Parse must ignore this Describe until Sync, without
        // publishing either parameter or result metadata.
        wire::write_frames(
            &mut writer,
            vec![
                wire::describe_statement_frame(statement),
                wire::sync_frame(),
            ],
        )
        .await;
        let ignored =
            wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;

        // Reusing the exact name proves failed Parse published no statement.
        // Parse+Close stays within this budget without planning or execution.
        wire::write_frames(
            &mut writer,
            vec![
                wire::parse_frame_with_types(statement, "SELECT CAST($1 AS INT)", &[23]),
                wire::close_statement_frame(statement),
                wire::sync_frame(),
            ],
        )
        .await;
        let reparsed =
            wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;
        drop((reader, writer));
        server.abort();
        let _ = server.await;
        (first, ignored, reparsed)
    });
    // Release all server/connection tasks and store owners even on expected RED.
    drop(runtime);
    drop(fixture);

    // Assert
    assert_eq!(
        wire::error_code(std::slice::from_ref(&first)).as_deref(),
        Some("53200"),
        "first Parse+Flush reply before Sync: {first:?}"
    );
    let first_message = wire::parse_error_fields(&first.1)
        .into_iter()
        .find_map(|(field, message)| (field == 'M').then_some(message))
        .expect("over-budget Parse includes an ErrorResponse message");
    let budget_suffix = format!(" > {budget_bytes}");
    assert!(
        first_message.ends_with(budget_suffix.as_str()),
        "4096 parameter OIDs must exceed the configured metadata budget: {first_message}"
    );
    assert_eq!(first.0, b'E');
    assert_eq!(ignored, vec![(b'Z', vec![b'I'])]);
    assert_eq!(
        reparsed,
        vec![(b'1', vec![]), (b'3', vec![]), (b'Z', vec![b'I'])]
    );
}
