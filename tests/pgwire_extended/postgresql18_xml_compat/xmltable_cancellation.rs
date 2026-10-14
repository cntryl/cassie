//! Pgwire cancellation and recovery while XMLTABLE is producing rows.

use std::time::Duration;

use tokio::io::AsyncWriteExt;

use crate::support_pgwire as wire;
use crate::support_sql_fixture::sql_fixture;

fn current_thread_runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
}

#[test]
fn should_cancel_xmltable_expansion_without_partial_rows_or_stale_state() {
    // Arrange
    let storage = sql_fixture("xmltable-expansion-cancellation", &[]);
    storage
        .cassie
        .startup()
        .expect("start XMLTABLE cancellation fixture");
    let runtime = current_thread_runtime();

    // Act
    let (baseline, cancelled, recovery) = runtime.block_on(async {
        let server = wire::spawn_server(storage.cassie.clone()).await;
        let socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("query connection");
        let (mut reader, mut writer) = tokio::io::split(socket);
        let (process_id, secret_key) =
            wire::complete_startup_with_backend_key(&mut reader, &mut writer).await;

        wire::write_frames(
            &mut writer,
            vec![wire::simple_query_frame(
                "SELECT expanded.value FROM XMLTABLE(\
                    '/root/item' PASSING XMLPARSE(DOCUMENT '<root><item>one</item></root>') \
                    COLUMNS value TEXT PATH 'text()') AS expanded",
            )],
        )
        .await;
        let baseline =
            wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;

        wire::write_frames(
            &mut writer,
            vec![wire::simple_query_frame(
                "SELECT expanded.value FROM XMLTABLE(\
                    '/root/item' PASSING XMLPARSE(DOCUMENT \
                        '<root><item>one</item><item>two</item><item>three</item></root>') \
                    COLUMNS value TEXT PATH 'text()') AS expanded \
                 WHERE pg_sleep_for(INTERVAL '60 seconds') IS NOT NULL",
            )],
        )
        .await;
        tokio::time::sleep(Duration::from_millis(100)).await;
        let mut cancel_socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("cancel connection");
        cancel_socket
            .write_all(&wire::cancel_request_frame(process_id, secret_key))
            .await
            .expect("write cancel request");
        cancel_socket
            .shutdown()
            .await
            .expect("close cancel request");
        let cancelled =
            wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;

        wire::write_frames(&mut writer, vec![wire::simple_query_frame("SELECT 45")]).await;
        let recovery =
            wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;

        drop(reader);
        drop(writer);
        server.stop().await;
        (baseline, cancelled, recovery)
    });
    drop(runtime);
    drop(storage);

    // Assert
    assert_eq!(wire::error_code(&baseline), None);
    assert_eq!(
        wire::data_rows(&baseline),
        vec![vec![Some("one".to_owned())]]
    );
    assert_eq!(wire::error_code(&cancelled).as_deref(), Some("57014"));
    assert!(wire::data_rows(&cancelled).is_empty());
    assert_eq!(cancelled.iter().filter(|(tag, _)| *tag == b'C').count(), 0);
    assert_eq!(cancelled.last().map(|frame| frame.0), Some(b'Z'));
    assert_eq!(wire::error_code(&recovery), None);
    assert_eq!(
        wire::data_rows(&recovery),
        vec![vec![Some("45".to_owned())]]
    );
}
