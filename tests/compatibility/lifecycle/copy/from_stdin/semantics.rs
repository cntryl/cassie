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
