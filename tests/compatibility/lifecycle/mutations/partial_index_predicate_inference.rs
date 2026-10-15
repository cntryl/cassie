use super::*;

#[test]
fn should_reject_partial_unique_index_inference_without_its_predicate() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("partial_index_without_inference_predicate").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_partial_target (\
                   id INT PRIMARY KEY, email TEXT, active BOOLEAN, version INT)",
                "CREATE UNIQUE INDEX compat_partial_target_email_active \
                   ON compat_partial_target(email) WHERE active",
                "INSERT INTO compat_partial_target VALUES \
                   (1, 'same@example.test', TRUE, 1)",
            ],
        )
        .await
        .expect("create a partial unique index and seed its predicate-matching row");

        // Act
        let rejected = client
            .execute(
                "INSERT INTO compat_partial_target (id, email, active, version) \
                 VALUES (2, 'same@example.test', TRUE, 2) \
                 ON CONFLICT (email) DO UPDATE SET version = EXCLUDED.version",
                &[],
            )
            .await;
        let sqlstate = rejected
            .as_ref()
            .err()
            .map(|error| db_error(error).code().code().to_owned());
        let stored = client
            .query_one(
                "SELECT id, version FROM compat_partial_target WHERE email = 'same@example.test'",
                &[],
            )
            .await
            .expect("read the row after the invalid conflict-target inference");

        // Assert
        assert_eq!(sqlstate.as_deref(), Some("42P10"));
        assert_eq!(stored.get::<_, i32>(0), 1);
        assert_eq!(stored.get::<_, i32>(1), 1);

        drop(client);
        server.shutdown(connection).await;
    });
}
