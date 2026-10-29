use super::*;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn assert_cancelled_copy_observations(
    copy_response_tag: u8,
    copy_response_payload: &[u8],
    error_code: Option<&str>,
    frames: &[(u8, Vec<u8>)],
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
        Some((b'Z', b"E".as_slice()))
    );
    assert!(rows.is_empty(), "cancelled COPY must publish no rows");
    assert_eq!(support_pgwire::error_code(recovered), None);
    assert_eq!(
        support_pgwire::data_rows(recovered),
        vec![vec![Some("1".to_owned())]]
    );
}

#[test]
fn should_cancel_idle_copy_from_stdin_without_followup_frame_and_recover_after_late_frames() {
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
        let begun = support_pgwire::read_frames_until_ready_within(
            &mut reader,
            Duration::from_secs(5),
        )
        .await;
        assert_eq!(
            begun.last().map(|(tag, payload)| (*tag, payload.as_slice())),
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
        support_pgwire::write_frames(&mut writer, vec![support_pgwire::copy_data_frame(b"1\n")])
            .await;
        let mut cancel_socket = tokio::net::TcpStream::connect(server.addr)
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
        let mut cancel_response = [0_u8; 1];
        let cancel_close = tokio::time::timeout(
            Duration::from_secs(5),
            cancel_socket.read(&mut cancel_response),
        )
        .await
        .expect("server should process CancelRequest");
        assert_eq!(cancel_close.expect("read CancelRequest close"), 0);
        let frames = support_pgwire::read_frames_until_ready_within(
            &mut reader,
            Duration::from_secs(5),
        )
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
        let rolled_back = support_pgwire::read_frames_until_ready_within(
            &mut reader,
            Duration::from_secs(5),
        )
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
            &rows,
            &recovered,
        );
        assert_eq!(transaction_status, Some((b'Z', b"E".as_slice())));
        assert_eq!(
            rolled_back.last().map(|(tag, payload)| (*tag, payload.as_slice())),
            Some((b'Z', b"I".as_slice()))
        );
    });
}
