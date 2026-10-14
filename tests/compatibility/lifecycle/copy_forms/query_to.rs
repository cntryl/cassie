use super::*;
use std::time::Duration;

#[test]
fn should_export_csv_query_rows_with_header() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("copy_csv_query_to").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_copy_query_rows (id INT PRIMARY KEY, label TEXT)",
                "INSERT INTO compat_copy_query_rows VALUES (1, 'a,b'), (2, NULL)",
            ],
        )
        .await
        .expect("create CSV query source rows");

        let socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("connect raw pgwire COPY client");
        let (mut reader, mut writer) = socket.into_split();
        support_pgwire::complete_startup(&mut reader, &mut writer).await;

        // Act
        support_pgwire::write_frames(
            &mut writer,
            vec![support_pgwire::simple_query_frame(
                "COPY (SELECT id, label FROM compat_copy_query_rows ORDER BY id) \
                 TO STDOUT WITH (FORMAT CSV, HEADER TRUE)",
            )],
        )
        .await;
        let frames =
            support_pgwire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5))
                .await;
        let error_code = support_pgwire::error_code(&frames);
        let response = frames
            .iter()
            .find(|(tag, _)| *tag == b'H')
            .map(|(_, payload)| payload.clone());
        let copy_data = frames
            .iter()
            .filter(|(tag, _)| *tag == b'd')
            .flat_map(|(_, payload)| payload.iter().copied())
            .collect::<Vec<_>>();
        let command = frames
            .iter()
            .find(|(tag, _)| *tag == b'C')
            .map(|(_, payload)| {
                String::from_utf8_lossy(payload.strip_suffix(&[0]).unwrap_or(payload)).into_owned()
            });

        drop((reader, writer, client));
        server.shutdown(connection).await;

        // Assert
        assert_eq!(
            error_code, None,
            "CSV COPY query SQLSTATE; frames: {frames:?}"
        );
        assert_eq!(
            response,
            Some(vec![0, 0, 2, 0, 0, 0, 0]),
            "CSV COPY query should describe two text-format columns"
        );
        assert_eq!(copy_data, b"id,label\n1,\"a,b\"\n2,\n");
        assert_eq!(command.as_deref(), Some("COPY 2"));
        assert!(frames.iter().any(|(tag, _)| *tag == b'c'));
    });
}
