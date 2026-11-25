#[test]
fn should_import_postgresql_dump_default_text_copy_data_section() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("issue_846_pg_dump_text_copy").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE compat_issue846_pg_dump_text_copy (payload TEXT)")
            .await
            .expect("create a target for PostgreSQL's default-text COPY section");

        // Act
        // PostgreSQL 18.6 default-text COPY decodes the tab escape and treats \\N as NULL.
        let frames = copy_in_frames(
            &server,
            "COPY compat_issue846_pg_dump_text_copy FROM STDIN",
            b"alpha\\tbeta\n\\N\n",
        )
        .await;
        let error_code = support_pgwire::error_code(&frames);
        let command = copy_command(&frames);
        let rows = client
            .query(
                "SELECT payload FROM compat_issue846_pg_dump_text_copy \
                 ORDER BY payload NULLS LAST",
                &[],
            )
            .await
            .expect("read the imported pg_dump-style text rows");

        // Assert
        drop(client);
        server.shutdown(connection).await;
        assert_eq!(
            frames.first().map(|(tag, _)| *tag),
            Some(b'G'),
            "PostgreSQL 18.6 default-text COPY enters COPY IN: {frames:?}"
        );
        assert_eq!(error_code, None, "pg_dump text COPY SQLSTATE: {frames:?}");
        assert_eq!(command.as_deref(), Some("COPY 2"));
        assert_eq!(
            rows.iter()
                .map(|row| row.get::<_, Option<String>>(0))
                .collect::<Vec<_>>(),
            vec![Some("alpha\tbeta".to_owned()), None]
        );
    });
}
