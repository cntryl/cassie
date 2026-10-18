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
