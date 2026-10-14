use super::*;

async fn copy_to_stdout(server: &CompatibilityServer, sql: &str) -> Vec<(u8, Vec<u8>)> {
    let socket = tokio::net::TcpStream::connect(server.addr)
        .await
        .expect("connect raw pgwire COPY client");
    let (mut reader, mut writer) = socket.into_split();
    support_pgwire::complete_startup(&mut reader, &mut writer).await;
    support_pgwire::write_frames(&mut writer, vec![support_pgwire::simple_query_frame(sql)]).await;
    let frames =
        support_pgwire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;
    drop((reader, writer));
    frames
}

fn copy_payload(frames: &[(u8, Vec<u8>)]) -> Vec<u8> {
    frames
        .iter()
        .filter(|(tag, _)| *tag == b'd')
        .flat_map(|(_, payload)| payload.iter().copied())
        .collect()
}

fn copy_command(frames: &[(u8, Vec<u8>)]) -> Option<String> {
    frames
        .iter()
        .find(|(tag, _)| *tag == b'C')
        .map(|(_, payload)| {
            String::from_utf8_lossy(payload.strip_suffix(&[0]).unwrap_or(payload)).into_owned()
        })
}

#[test]
fn should_export_default_text_table_copy_escaped_rows() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("copy_text_table_to_wave4").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_copy_text_table_to (id INT, payload TEXT)",
                "INSERT INTO compat_copy_text_table_to VALUES \
                 (1, 'with\\slash'), (2, NULL), (3, '')",
            ],
        )
        .await
        .expect("create the default-text COPY source rows");

        // Act
        let frames = copy_to_stdout(
            &server,
            "COPY compat_copy_text_table_to (payload, id) TO STDOUT",
        )
        .await;
        let error_code = support_pgwire::error_code(&frames);
        let response = frames
            .iter()
            .find(|(tag, _)| *tag == b'H')
            .map(|(_, payload)| payload.clone());
        let payload = copy_payload(&frames);
        let command = copy_command(&frames);
        let completed = frames.iter().any(|(tag, _)| *tag == b'c');

        // Assert
        drop(client);
        server.shutdown(connection).await;
        assert_eq!(
            error_code, None,
            "default-text table COPY TO SQLSTATE; frames: {frames:?}"
        );
        assert_eq!(response, Some(vec![0, 0, 2, 0, 0, 0, 0]));
        assert_eq!(payload, b"with\\\\slash\t1\n\\N\t2\n\t3\n");
        assert_eq!(command.as_deref(), Some("COPY 3"));
        assert!(completed, "table COPY TO should send CopyDone");
    });
}

#[test]
fn should_export_csv_table_copy_with_selected_column_options() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("copy_csv_table_to_wave4").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_copy_csv_table_to (id INT, payload TEXT)",
                "INSERT INTO compat_copy_csv_table_to VALUES \
                 (1, 'a|b'), (2, NULL), (3, 'say \"ok\"')",
            ],
        )
        .await
        .expect("create the CSV COPY source rows");

        // Act
        let frames = copy_to_stdout(
            &server,
            "COPY compat_copy_csv_table_to (payload, id) TO STDOUT \
             WITH (FORMAT CSV, HEADER, DELIMITER '|', NULL 'NULL', FORCE_QUOTE (payload))",
        )
        .await;
        let error_code = support_pgwire::error_code(&frames);
        let response = frames
            .iter()
            .find(|(tag, _)| *tag == b'H')
            .map(|(_, payload)| payload.clone());
        let payload = copy_payload(&frames);
        let command = copy_command(&frames);
        let completed = frames.iter().any(|(tag, _)| *tag == b'c');

        // Assert
        drop(client);
        server.shutdown(connection).await;
        assert_eq!(
            error_code, None,
            "CSV table COPY TO SQLSTATE; frames: {frames:?}"
        );
        assert_eq!(response, Some(vec![0, 0, 2, 0, 0, 0, 0]));
        let mut lines = String::from_utf8(payload)
            .expect("PostgreSQL CSV COPY output is UTF-8")
            .lines()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let header = lines.remove(0);
        lines.sort_unstable();
        assert_eq!(header, "payload|id");
        assert_eq!(lines, vec!["\"a|b\"|1", "\"say \"\"ok\"\"\"|3", "NULL|2"]);
        assert_eq!(command.as_deref(), Some("COPY 3"));
        assert!(completed, "CSV table COPY TO should send CopyDone");
    });
}

#[test]
fn should_frame_binary_query_copy_scalar_null_fields() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("copy_binary_query_to_wave4").await;
        let (client, connection) = server.connect().await;

        // Act
        let frames = copy_to_stdout(
            &server,
            "COPY (SELECT 42::INT4 AS n, NULL::TEXT AS absent, 'ok'::TEXT AS payload) \
             TO STDOUT WITH (FORMAT BINARY)",
        )
        .await;
        let error_code = support_pgwire::error_code(&frames);
        let response = frames
            .iter()
            .find(|(tag, _)| *tag == b'H')
            .map(|(_, payload)| payload.clone());
        let payload = copy_payload(&frames);
        let command = copy_command(&frames);
        let completed = frames.iter().any(|(tag, _)| *tag == b'c');

        // Assert
        drop(client);
        server.shutdown(connection).await;
        assert_eq!(
            error_code, None,
            "binary query COPY TO SQLSTATE; frames: {frames:?}"
        );
        assert_eq!(response, Some(vec![1, 0, 3, 0, 1, 0, 1, 0, 1]));
        let mut expected = b"PGCOPY\n\xff\r\n\0".to_vec();
        expected.extend_from_slice(&0_i32.to_be_bytes()); // flags
        expected.extend_from_slice(&0_i32.to_be_bytes()); // extension length
        expected.extend_from_slice(&3_i16.to_be_bytes()); // field count
        expected.extend_from_slice(&4_i32.to_be_bytes()); // int4 field length
        expected.extend_from_slice(&42_i32.to_be_bytes());
        expected.extend_from_slice(&(-1_i32).to_be_bytes()); // NULL field length
        expected.extend_from_slice(&2_i32.to_be_bytes()); // text field length
        expected.extend_from_slice(b"ok");
        expected.extend_from_slice(&(-1_i16).to_be_bytes()); // end of rows
        assert_eq!(payload, expected);
        assert_eq!(command.as_deref(), Some("COPY 1"));
        assert!(completed, "binary query COPY TO should send CopyDone");
    });
}
