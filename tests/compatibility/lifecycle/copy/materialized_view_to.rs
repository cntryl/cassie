use super::*;

#[test]
fn should_export_the_materialized_snapshot_from_a_populated_view() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("copy_materialized_view_to").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute(
                "CREATE TABLE compat_copy_mv_source (id INT PRIMARY KEY, label TEXT); \
                 INSERT INTO compat_copy_mv_source VALUES (1, 'snapshot'); \
                 CREATE MATERIALIZED VIEW compat_copy_mv AS \
                   SELECT id, label FROM compat_copy_mv_source; \
                 UPDATE compat_copy_mv_source SET label = 'current' WHERE id = 1",
            )
            .await
            .expect("create a populated materialized view and change its source row");

        let socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("connect raw pgwire COPY client");
        let (mut reader, mut writer) = socket.into_split();
        support_pgwire::complete_startup(&mut reader, &mut writer).await;

        // Act
        support_pgwire::write_frames(
            &mut writer,
            vec![support_pgwire::simple_query_frame(
                "COPY compat_copy_mv TO STDOUT WITH (FORMAT CSV, HEADER)",
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
            "COPY materialized view SQLSTATE; frames: {frames:?}"
        );
        assert_eq!(
            response,
            Some(vec![0, 0, 2, 0, 0, 0, 0]),
            "COPY should describe the two materialized-view columns in text format"
        );
        assert_eq!(
            copy_data, b"id,label\n1,snapshot\n",
            "COPY must export the materialized snapshot, not its changed source row"
        );
        assert_eq!(command.as_deref(), Some("COPY 1"));
        assert!(frames.iter().any(|(tag, _)| *tag == b'c'));
    });
}

#[test]
fn should_force_quote_every_csv_copy_output_column() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("copy_force_quote_star").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute(
                "CREATE TABLE compat_copy_force_quote_star (id INT, label TEXT); \
                 INSERT INTO compat_copy_force_quote_star VALUES (1, 'plain')",
            )
            .await
            .expect("create the CSV COPY source row");

        let socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("connect raw pgwire COPY client");
        let (mut reader, mut writer) = socket.into_split();
        support_pgwire::complete_startup(&mut reader, &mut writer).await;

        // Act
        support_pgwire::write_frames(
            &mut writer,
            vec![support_pgwire::simple_query_frame(
                "COPY compat_copy_force_quote_star TO STDOUT \
                 WITH (FORMAT CSV, FORCE_QUOTE *)",
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
            "COPY FORCE_QUOTE * SQLSTATE; frames: {frames:?}"
        );
        assert_eq!(
            response,
            Some(vec![0, 0, 2, 0, 0, 0, 0]),
            "CSV COPY should describe both output columns in text format"
        );
        assert_eq!(copy_data, b"\"1\",\"plain\"\n");
        assert_eq!(command.as_deref(), Some("COPY 1"));
        assert!(frames.iter().any(|(tag, _)| *tag == b'c'));
    });
}
