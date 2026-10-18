use super::*;
use std::time::Duration;
use tokio::io::AsyncWriteExt;

fn assert_cancelled_copy_observations(
    copy_response_tag: u8,
    copy_response_payload: &[u8],
    error_code: Option<&str>,
    frames: &[(u8, Vec<u8>)],
    rows: &[tokio_postgres::Row],
    recovered: &[(u8, Vec<u8>)],
    responded_before_copy_done: bool,
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
        Some((b'Z', b"I".as_slice()))
    );
    assert!(rows.is_empty(), "cancelled COPY must publish no rows");
    assert_eq!(support_pgwire::error_code(recovered), None);
    assert_eq!(
        support_pgwire::data_rows(recovered),
        vec![vec![Some("1".to_owned())]]
    );
    assert!(
        responded_before_copy_done,
        "PostgreSQL 18.6 returns SQLSTATE 57014 and ReadyForQuery before CopyDone; \
         Cassie only responds after the client terminates the COPY stream: {frames:?}"
    );
}

#[test]
fn should_cancel_copy_from_stdin_over_pgwire_without_publishing_rows() {
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
        tokio::time::sleep(Duration::from_millis(50)).await;

        support_pgwire::write_frames(&mut writer, vec![support_pgwire::copy_data_frame(b"2\n")])
            .await;
        let response_before_copy_done = tokio::time::timeout(
            Duration::from_secs(1),
            support_pgwire::read_frames_until_ready(&mut reader),
        )
        .await
        .ok();
        if response_before_copy_done.is_none() {
            // Keep the Cassie session bounded and collect its delayed response
            // after the stream's normal terminator.
            support_pgwire::write_frames(&mut writer, vec![support_pgwire::copy_done_frame()])
                .await;
        }
        let frames = match response_before_copy_done.as_ref() {
            Some(frames) => frames.clone(),
            None => {
                support_pgwire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5))
                    .await
            }
        };
        let mut frames_with_copy_response = frames.clone();
        frames_with_copy_response.insert(0, (copy_response_tag, copy_response_payload.clone()));
        let error_code = support_pgwire::error_code(&frames_with_copy_response);
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
            response_before_copy_done.is_some(),
        );
    });
}
