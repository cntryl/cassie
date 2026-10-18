use super::*;

#[test]
fn should_force_null_to_match_quoted_csv_null_markers() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("copy_force_null_csv_marker").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute(
                "CREATE TABLE compat_copy_force_null \
                 (id INT, ordinary TEXT, forced TEXT)",
            )
            .await
            .expect("create a CSV COPY target with selected FORCE_NULL column");

        // Act
        let (response_tag, response_payload, error_code, command) = super::copy_from_stdin(
            &server,
            "COPY compat_copy_force_null FROM STDIN \
             WITH (FORMAT CSV, NULL 'N', FORCE_NULL (forced))",
            b"1,\"N\",\"N\"\n2,N,N\n3,\"\",\"\"\n",
        )
        .await;
        let rows = if response_tag == b'G' && error_code.is_none() {
            Some(
                client
                    .query(
                        "SELECT id, ordinary, forced FROM compat_copy_force_null ORDER BY id",
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
            "CSV COPY with FORCE_NULL should enter COPY IN; payload: {response_payload:?}; \
             SQLSTATE: {error_code:?}"
        );
        assert_eq!(error_code, None);
        assert_eq!(command.as_deref(), Some("COPY 3"));
        let rows = rows
            .expect("query imported rows only after COPY completed")
            .expect("read imported FORCE_NULL CSV rows");
        assert_eq!(rows.len(), 3);
        assert_eq!(
            rows.iter()
                .map(|row| (
                    row.get::<_, i32>(0),
                    row.get::<_, Option<String>>(1),
                    row.get::<_, Option<String>>(2),
                ))
                .collect::<Vec<_>>(),
            vec![
                (1, Some("N".to_owned()), None),
                (2, None, None),
                (3, Some(String::new()), Some(String::new())),
            ]
        );
    });
}
