#[test]
fn should_export_declared_columns_for_quoted_schema_qualified_table_copy() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("copy_quoted_schema_table_to").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE SCHEMA \"CopySchema\"",
                "CREATE TABLE \"CopySchema\".\"MixedTable\" (\"select\" TEXT, value INT)",
                "INSERT INTO \"CopySchema\".\"MixedTable\" VALUES ('with,comma', 7)",
            ],
        )
        .await
        .expect("create the quoted schema-qualified COPY source row");

        // Act
        let frames = copy_to_stdout(
            &server,
            "COPY \"CopySchema\".\"MixedTable\" TO STDOUT WITH (FORMAT CSV, HEADER)",
        )
        .await;
        let error_code = support_pgwire::error_code(&frames);
        let payload = copy_payload(&frames);
        let command = copy_command(&frames);

        // Assert
        drop(client);
        server.shutdown(connection).await;
        assert_eq!(error_code, None, "quoted table COPY SQLSTATE; frames: {frames:?}");
        assert_eq!(payload, b"select,value\n\"with,comma\",7\n");
        assert_eq!(command.as_deref(), Some("COPY 1"));
    });
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
