use super::*;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

async fn cancel_backend(server_addr: std::net::SocketAddr, process_id: i32, secret_key: i32) {
    let mut cancel_socket = tokio::net::TcpStream::connect(server_addr)
        .await
        .expect("connect pgwire cancellation request");
    cancel_socket
        .write_all(&support_pgwire::cancel_request_frame(
            process_id, secret_key,
        ))
        .await
        .expect("send query cancellation request");
    cancel_socket
        .shutdown()
        .await
        .expect("close cancellation request");
    let mut response = [0_u8; 1];
    let close = tokio::time::timeout(Duration::from_secs(5), cancel_socket.read(&mut response))
        .await
        .expect("server should process CancelRequest");
    assert_eq!(close.expect("read CancelRequest close"), 0);
}

fn assert_cancelled_copy_observations(
    copy_response_tag: u8,
    copy_response_payload: &[u8],
    error_code: Option<&str>,
    frames: &[(u8, Vec<u8>)],
    expected_ready_status: u8,
    rows: &[tokio_postgres::Row],
    recovered: &[(u8, Vec<u8>)],
) {
    assert_eq!(
        copy_response_tag, b'G',
        "valid COPY FROM STDIN should enter COPY IN; payload: {copy_response_payload:?}"
    );
    assert_eq!(error_code, Some("57014"));
    assert!(
        frames.iter().all(|(tag, _)| *tag != b'C'),
        "cancelled COPY must not send CommandComplete: {frames:?}"
    );
    assert_eq!(
        frames
            .last()
            .map(|(tag, payload)| (*tag, payload.as_slice())),
        Some((b'Z', std::slice::from_ref(&expected_ready_status)))
    );
    assert!(rows.is_empty(), "cancelled COPY must publish no rows");
    assert_eq!(support_pgwire::error_code(recovered), None);
    assert_eq!(
        support_pgwire::data_rows(recovered),
        vec![vec![Some("1".to_owned())]]
    );
}

#[test]
fn should_cancel_idle_copy_from_stdin_inside_explicit_transaction() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("copy_pgwire_cancel").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE compat_copy_pgwire_cancel (id INT)")
            .await
            .expect("create the COPY target table");

        let socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("connect raw pgwire COPY client");
        let (mut reader, mut writer) = socket.into_split();
        let (process_id, secret_key) =
            support_pgwire::complete_startup_with_backend_key(&mut reader, &mut writer).await;

        support_pgwire::write_frames(
            &mut writer,
            vec![support_pgwire::simple_query_frame("BEGIN")],
        )
        .await;
        let begun =
            support_pgwire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5))
                .await;
        assert_eq!(
            begun
                .last()
                .map(|(tag, payload)| (*tag, payload.as_slice())),
            Some((b'Z', b"T".as_slice()))
        );

        support_pgwire::write_frames(
            &mut writer,
            vec![support_pgwire::simple_query_frame(
                "COPY compat_copy_pgwire_cancel (id) FROM STDIN",
            )],
        )
        .await;
        let (copy_response_tag, copy_response_payload) =
            support_pgwire::read_wire_frame(&mut reader).await;

        // Act
        cancel_backend(server.addr, process_id, secret_key).await;
        let frames =
            support_pgwire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5))
                .await;
        let mut frames_with_copy_response = frames.clone();
        frames_with_copy_response.insert(0, (copy_response_tag, copy_response_payload.clone()));
        let error_code = support_pgwire::error_code(&frames_with_copy_response);
        let transaction_status = frames
            .last()
            .map(|(tag, payload)| (*tag, payload.as_slice()));

        // Send frames that were already in flight when the cancelled COPY ended.
        support_pgwire::write_frames(
            &mut writer,
            vec![
                support_pgwire::copy_data_frame(b"1\n"),
                support_pgwire::copy_done_frame(),
                support_pgwire::simple_query_frame("ROLLBACK"),
            ],
        )
        .await;
        let rolled_back =
            support_pgwire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5))
                .await;
        let rows = client
            .query("SELECT id FROM compat_copy_pgwire_cancel", &[])
            .await
            .expect("read rows after cancelled COPY");

        support_pgwire::write_frames(
            &mut writer,
            vec![support_pgwire::simple_query_frame("SELECT 1")],
        )
        .await;
        let recovered =
            support_pgwire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5))
                .await;

        // Assert
        drop((reader, writer, client));
        server.shutdown(connection).await;
        assert_cancelled_copy_observations(
            copy_response_tag,
            &copy_response_payload,
            error_code.as_deref(),
            &frames,
            b'E',
            &rows,
            &recovered,
        );
        assert_eq!(transaction_status, Some((b'Z', b"E".as_slice())));
        assert_eq!(
            rolled_back
                .last()
                .map(|(tag, payload)| (*tag, payload.as_slice())),
            Some((b'Z', b"I".as_slice()))
        );
    });
}

#[test]
fn should_cancel_idle_copy_from_stdin_inside_implicit_transaction() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("copy_pgwire_cancel_implicit").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE compat_copy_pgwire_cancel_implicit (id INT)")
            .await
            .expect("create the COPY target table");
        let socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("connect raw pgwire COPY client");
        let (mut reader, mut writer) = socket.into_split();
        let (process_id, secret_key) =
            support_pgwire::complete_startup_with_backend_key(&mut reader, &mut writer).await;

        support_pgwire::write_frames(
            &mut writer,
            vec![support_pgwire::simple_query_frame(
                "COPY compat_copy_pgwire_cancel_implicit (id) FROM STDIN",
            )],
        )
        .await;
        let (copy_response_tag, copy_response_payload) =
            support_pgwire::read_wire_frame(&mut reader).await;

        // Act
        cancel_backend(server.addr, process_id, secret_key).await;
        let frames =
            support_pgwire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5))
                .await;
        let mut frames_with_copy_response = frames.clone();
        frames_with_copy_response.insert(0, (copy_response_tag, copy_response_payload.clone()));
        let error_code = support_pgwire::error_code(&frames_with_copy_response);
        support_pgwire::write_frames(
            &mut writer,
            vec![
                support_pgwire::copy_data_frame(b"2\n"),
                support_pgwire::copy_done_frame(),
                support_pgwire::simple_query_frame("SELECT 1"),
            ],
        )
        .await;
        let recovered =
            support_pgwire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5))
                .await;
        let rows = client
            .query("SELECT id FROM compat_copy_pgwire_cancel_implicit", &[])
            .await
            .expect("read rows after cancelled COPY");

        // Assert
        drop((reader, writer, client));
        server.shutdown(connection).await;
        assert_cancelled_copy_observations(
            copy_response_tag,
            &copy_response_payload,
            error_code.as_deref(),
            &frames,
            b'I',
            &rows,
            &recovered,
        );
    });
}

#[test]
fn should_cancel_copy_into_a_transaction_owned_table_and_recover_at_savepoint() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("staged_copy_cancel_recovery").await;
        let (client, connection) = server.connect().await;
        let socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("wire connection");
        let (mut reader, mut writer) = socket.into_split();
        let (process_id, secret_key) =
            support_pgwire::complete_startup_with_backend_key(&mut reader, &mut writer).await;
        for sql in [
            "BEGIN",
            "CREATE TABLE staged_copy_cancelled (id SERIAL PRIMARY KEY, n INT)",
            "SAVEPOINT before_copy",
        ] {
            support_pgwire::write_frames(
                &mut writer,
                vec![support_pgwire::simple_query_frame(sql)],
            )
            .await;
            let frames =
                support_pgwire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5))
                    .await;
            assert_eq!(support_pgwire::error_code(&frames), None);
        }
        support_pgwire::write_frames(
            &mut writer,
            vec![support_pgwire::simple_query_frame(
                "COPY staged_copy_cancelled (n) FROM STDIN",
            )],
        )
        .await;
        let (tag, _) = support_pgwire::read_wire_frame(&mut reader).await;
        assert_eq!(tag, b'G');

        // Act
        cancel_backend(server.addr, process_id, secret_key).await;
        let cancelled =
            support_pgwire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5))
                .await;
        support_pgwire::write_frames(
            &mut writer,
            vec![support_pgwire::simple_query_frame(
                "ROLLBACK TO before_copy",
            )],
        )
        .await;
        let recovered =
            support_pgwire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5))
                .await;
        support_pgwire::write_frames(
            &mut writer,
            vec![support_pgwire::simple_query_frame(
                "SELECT count(*) FROM staged_copy_cancelled",
            )],
        )
        .await;
        let rows =
            support_pgwire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5))
                .await;
        drop((reader, writer, client));
        server.shutdown(connection).await;

        // Assert
        assert_eq!(
            support_pgwire::error_code(&cancelled).as_deref(),
            Some("57014")
        );
        assert!(!cancelled.iter().any(|(tag, _)| *tag == b'C'));
        assert_eq!(support_pgwire::error_code(&recovered), None);
        assert_eq!(
            support_pgwire::data_rows(&rows),
            vec![vec![Some("0".into())]]
        );
    });
}

async fn start_timed_staged_copy() -> (
    tokio::task::JoinHandle<()>,
    tokio::net::tcp::OwnedReadHalf,
    tokio::net::tcp::OwnedWriteHalf,
) {
    let mut config = cassie::config::CassieRuntimeConfig::from_env().expect("runtime config");
    crate::support_sql::test_root_password().clone_into(&mut config.password);
    config.limits.query_timeout_ms = 1_000;
    let cassie = cassie::app::Cassie::new_in_memory_with_config(config.clone()).expect("Cassie");
    cassie.startup().expect("startup");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener");
    let address = listener.local_addr().expect("address");
    let server = tokio::spawn(async move {
        let _ = cassie::pgwire::server::run_with_listener(
            listener,
            std::sync::Arc::new(cassie),
            config,
        )
        .await;
    });
    let socket = tokio::net::TcpStream::connect(address)
        .await
        .expect("connect");
    let (mut reader, mut writer) = socket.into_split();
    support_pgwire::complete_startup_with_backend_key(&mut reader, &mut writer).await;
    for sql in [
        "BEGIN",
        "CREATE TABLE timed_staged_copy (n INT)",
        "SAVEPOINT before_copy",
    ] {
        support_pgwire::write_frames(&mut writer, vec![support_pgwire::simple_query_frame(sql)])
            .await;
        let frames =
            support_pgwire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5))
                .await;
        assert_eq!(support_pgwire::error_code(&frames), None);
    }
    support_pgwire::write_frames(
        &mut writer,
        vec![support_pgwire::simple_query_frame(
            "COPY timed_staged_copy FROM STDIN",
        )],
    )
    .await;
    let (tag, _) = support_pgwire::read_wire_frame(&mut reader).await;
    assert_eq!(tag, b'G');
    (server, reader, writer)
}

#[test]
fn should_recover_after_idle_staged_copy_deadline() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let (server, mut reader, mut writer) = start_timed_staged_copy().await;
        // Act
        let timeout =
            support_pgwire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5))
                .await;
        support_pgwire::write_frames(
            &mut writer,
            vec![support_pgwire::simple_query_frame(
                "ROLLBACK TO before_copy",
            )],
        )
        .await;
        let recovery =
            support_pgwire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5))
                .await;
        support_pgwire::write_frames(
            &mut writer,
            vec![support_pgwire::simple_query_frame(
                "SELECT count(*) FROM timed_staged_copy",
            )],
        )
        .await;
        let rows =
            support_pgwire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5))
                .await;
        server.abort();
        // Assert
        assert_eq!(
            support_pgwire::error_code(&timeout).as_deref(),
            Some("57014")
        );
        assert_eq!(support_pgwire::error_code(&recovery), None);
        assert_eq!(
            support_pgwire::data_rows(&rows),
            vec![vec![Some("0".into())]]
        );
    });
}

#[test]
fn should_close_staged_copy_connection_after_deadline_during_partial_frame() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        for prefix in [&[b'd', 0, 0][..], &[b'd', 0, 0, 0, 8, b'x'][..]] {
            let (server, mut reader, mut writer) = start_timed_staged_copy().await;
            writer.write_all(prefix).await.expect("partial frame");
            // Act
            let mut byte = [0_u8; 1];
            let received = tokio::time::timeout(Duration::from_secs(5), reader.read(&mut byte))
                .await
                .expect("connection closes")
                .expect("read");
            server.abort();
            // Assert
            assert_eq!(
                received, 0,
                "no protocol reply is sent into a partial frame"
            );
        }
    });
}
