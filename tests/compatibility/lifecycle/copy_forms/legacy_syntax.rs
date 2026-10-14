use super::*;

#[test]
fn should_import_csv_with_legacy_unparenthesized_header_syntax() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("copy_legacy_csv_header").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE compat_copy_legacy_header (id INT, label TEXT)")
            .await
            .expect("create the table before testing the legacy COPY syntax");

        // Act
        let (response_tag, response_payload, error_code, command) = super::copy_from_stdin(
            &server,
            "COPY compat_copy_legacy_header (id, label) FROM STDIN WITH CSV HEADER",
            b"id,label\n1,alpha\n",
        )
        .await;
        let rows = if response_tag == b'G' && error_code.is_none() {
            Some(
                client
                    .query(
                        "SELECT id, label FROM compat_copy_legacy_header ORDER BY id",
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
            "legacy WITH CSV HEADER must enter COPY IN; response: {response_payload:?}; \
             SQLSTATE: {error_code:?}"
        );
        assert_eq!(error_code, None);
        assert_eq!(command.as_deref(), Some("COPY 1"));
        let rows = rows
            .expect("query the rows only after COPY completed")
            .expect("read imported rows");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].get::<_, i32>(0), 1);
        assert_eq!(rows[0].get::<_, String>(1), "alpha");
    });
}
