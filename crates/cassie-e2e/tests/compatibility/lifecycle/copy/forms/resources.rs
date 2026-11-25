#[test]
fn should_import_copy_with_legacy_using_delimiters() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("issue_846_pre_73_copy_aliases").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE compat_issue846_using_delimiters (id INT, note TEXT)")
            .await
            .expect("create the pre-7.3 COPY target table");

        // Verify standard COPY FROM STDIN before the legacy delimiter form.
        let control = copy_in_frames(
            &server,
            "COPY compat_issue846_using_delimiters FROM STDIN WITH (FORMAT CSV)",
            b"0,baseline\n",
        )
        .await;
        assert_eq!(
            control.first().map(|(tag, _)| *tag),
            Some(b'G'),
            "standard COPY control enters COPY IN: {control:?}"
        );
        assert_eq!(support_pgwire::error_code(&control), None);
        assert_eq!(copy_command(&control).as_deref(), Some("COPY 1"));

        // Act
        let (response_tag, response_payload, error_code, command) = copy_from_stdin(
            &server,
            "COPY compat_issue846_using_delimiters FROM STDIN USING DELIMITERS '|'",
            b"1|plain\n",
        )
        .await;
        let rows = if response_tag == b'G' && error_code.is_none() {
            Some(
                client
                    .query(
                        "SELECT id, note FROM compat_issue846_using_delimiters ORDER BY id",
                        &[],
                    )
                    .await,
            )
        } else {
            None
        };

        // Assert
        drop(client);
        server.shutdown(connection).await;
        assert_eq!(
            response_tag, b'G',
            "PostgreSQL 18.6 USING DELIMITERS should enter COPY IN; payload: {response_payload:?}; \
             SQLSTATE: {error_code:?}"
        );
        assert_eq!(error_code, None);
        assert_eq!(command.as_deref(), Some("COPY 1"));
        let rows = rows
            .expect("query imported rows only after COPY completed")
            .expect("read imported legacy COPY rows");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].get::<_, i32>(0), 0);
        assert_eq!(rows[0].get::<_, String>(1), "baseline");
        assert_eq!(rows[1].get::<_, i32>(0), 1);
        assert_eq!(rows[1].get::<_, String>(1), "plain");
    });
}
