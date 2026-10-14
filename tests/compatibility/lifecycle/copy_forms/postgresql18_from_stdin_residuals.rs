use super::*;

fn notice_messages(frames: &[(u8, Vec<u8>)]) -> Vec<String> {
    frames
        .iter()
        .filter(|(tag, _)| *tag == b'N')
        .flat_map(|(_, payload)| {
            let fields = payload.split(|byte| *byte == 0).collect::<Vec<_>>();
            fields
                .windows(2)
                .filter(|pair| pair[0] == b"M")
                .map(|pair| String::from_utf8_lossy(pair[1]).into_owned())
                .collect::<Vec<_>>()
        })
        .collect()
}

async fn copy_from_stdin_with_frames(
    server: &CompatibilityServer,
    sql: &str,
    data: &[u8],
) -> Vec<(u8, Vec<u8>)> {
    let socket = tokio::net::TcpStream::connect(server.addr)
        .await
        .expect("connect raw pgwire COPY client");
    let (mut reader, mut writer) = socket.into_split();
    support_pgwire::complete_startup(&mut reader, &mut writer).await;
    support_pgwire::write_frames(&mut writer, vec![support_pgwire::simple_query_frame(sql)]).await;
    let (response_tag, response_payload) = support_pgwire::read_wire_frame(&mut reader).await;
    if response_tag == b'G' {
        support_pgwire::write_frames(
            &mut writer,
            vec![
                support_pgwire::copy_data_frame(data),
                support_pgwire::copy_done_frame(),
            ],
        )
        .await;
    }
    let mut frames =
        support_pgwire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;
    frames.insert(0, (response_tag, response_payload));
    drop((reader, writer));
    frames
}

#[test]
fn should_report_skipped_copy_rows_with_log_verbosity_verbose() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("copy_log_verbosity_verbose").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE compat_copy_log_verbosity (id INT, amount INT)")
            .await
            .expect("create table for COPY LOG_VERBOSITY");

        // Act
        let frames = copy_from_stdin_with_frames(
            &server,
            "COPY compat_copy_log_verbosity FROM STDIN \
             WITH (FORMAT CSV, ON_ERROR ignore, LOG_VERBOSITY verbose)",
            b"1,10\nbad,20\n3,30\n",
        )
        .await;
        let response_tag = frames.first().map(|(tag, _)| *tag);
        let error_code = support_pgwire::error_code(&frames);
        let command = frames
            .iter()
            .find(|(tag, _)| *tag == b'C')
            .map(|(_, payload)| {
                String::from_utf8_lossy(payload.strip_suffix(&[0]).unwrap_or(payload)).into_owned()
            });
        let notices = notice_messages(&frames);
        let rows = client
            .query(
                "SELECT id, amount FROM compat_copy_log_verbosity ORDER BY id",
                &[],
            )
            .await
            .expect("read rows accepted by COPY ON_ERROR ignore");

        // Assert
        drop(client);
        server.shutdown(connection).await;
        assert_eq!(response_tag, Some(b'G'), "COPY should enter COPY IN: {frames:?}");
        assert_eq!(error_code, None, "COPY LOG_VERBOSITY SQLSTATE: {frames:?}");
        assert_eq!(command.as_deref(), Some("COPY 2"));
        assert_eq!(
            notices,
            vec![
                "skipping row due to data type incompatibility at line 2 for column \"id\": \"bad\"",
                "1 row was skipped due to data type incompatibility",
            ],
            "PostgreSQL 18.6 emits a row detail and skipped-row summary for verbose COPY"
        );
        assert_eq!(rows.len(), 2);
        assert_eq!(
            rows.iter()
                .map(|row| (row.get::<_, i32>(0), row.get::<_, i32>(1)))
                .collect::<Vec<_>>(),
            vec![(1, 10), (3, 30)]
        );
    });
}

#[test]
fn should_abort_copy_from_stdin_atomically_on_malformed_input() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("copy_malformed_atomicity").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE compat_copy_malformed_atomicity (id INT)")
            .await
            .expect("create table for malformed COPY input");

        // Act
        let (response_tag, response_payload, error_code, command) = copy_from_stdin(
            &server,
            "COPY compat_copy_malformed_atomicity FROM STDIN WITH (FORMAT CSV)",
            b"1\nbad\n3\n",
        )
        .await;
        let rows = client
            .query(
                "SELECT id FROM compat_copy_malformed_atomicity ORDER BY id",
                &[],
            )
            .await
            .expect("read target rows after malformed COPY fails");

        // Assert
        drop(client);
        server.shutdown(connection).await;
        assert_eq!(
            response_tag, b'G',
            "valid COPY FROM STDIN should enter COPY IN; payload: {response_payload:?}"
        );
        assert_eq!(error_code.as_deref(), Some("22P02"));
        assert!(
            command.is_none(),
            "failed COPY must not send CommandComplete"
        );
        assert!(
            rows.is_empty(),
            "COPY must roll back rows accepted before the malformed input row"
        );
    });
}
