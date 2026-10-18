use super::*;

#[test]
fn should_rollback_copy_rows_after_constraint_violation() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("copy_check_constraint_atomicity").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute(
                "CREATE TABLE compat_copy_check_constraint (\
                   id INT PRIMARY KEY, amount INT, \
                   CONSTRAINT compat_copy_check_amount CHECK (amount >= 0))",
            )
            .await
            .expect("create a COPY target with a named CHECK constraint");

        // Act
        let (response_tag, _response_payload, error_code, command) = super::copy_from_stdin(
            &server,
            "COPY compat_copy_check_constraint (id, amount) FROM STDIN \
             WITH (FORMAT CSV)",
            b"1,10\n2,-5\n",
        )
        .await;
        let remaining = client
            .query(
                "SELECT id, amount FROM compat_copy_check_constraint ORDER BY id",
                &[],
            )
            .await
            .expect("read COPY target after rejected constrained row");

        // Assert
        assert_eq!(response_tag, b'G');
        assert_eq!(error_code.as_deref(), Some("23514"));
        assert_eq!(command, None);
        assert!(
            remaining.is_empty(),
            "failed COPY must roll back earlier rows"
        );

        drop(client);
        server.shutdown(connection).await;
    });
}
