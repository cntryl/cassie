use super::*;

#[test]
fn should_reject_csv_header_match_when_column_order_differs() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("copy_header_match_order_mismatch").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE compat_copy_header_match_order (id INT, label TEXT)")
            .await
            .expect("create the COPY HEADER MATCH target");

        // Act
        let (response_tag, response_payload, error_code, command) = super::copy_from_stdin(
            &server,
            "COPY compat_copy_header_match_order (id, label) FROM STDIN \
             WITH (FORMAT CSV, HEADER MATCH)",
            b"label,id\n1,loaded\n",
        )
        .await;
        let rows = client
            .query(
                "SELECT id, label FROM compat_copy_header_match_order ORDER BY id",
                &[],
            )
            .await
            .expect("read rows after a rejected COPY header");

        // Assert
        drop(client);
        server.shutdown(connection).await;
        assert_eq!(
            response_tag, b'G',
            "the accepted HEADER MATCH form should enter COPY IN; response: \
             {response_payload:?}; SQLSTATE: {error_code:?}"
        );
        assert_eq!(error_code.as_deref(), Some("22P04"));
        assert_eq!(command, None);
        assert!(rows.is_empty(), "a mismatched header must publish no rows");
    });
}
