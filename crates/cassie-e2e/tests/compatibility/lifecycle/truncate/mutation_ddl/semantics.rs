#[test]
fn should_truncate_referencing_rows_with_cascade() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("truncate_cascade_foreign_key").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_truncate_cascade_parent (id INT PRIMARY KEY)",
                "CREATE TABLE compat_truncate_cascade_child (\
                   id INT PRIMARY KEY, parent_id INT REFERENCES compat_truncate_cascade_parent(id))",
                "INSERT INTO compat_truncate_cascade_parent VALUES (1), (2)",
                "INSERT INTO compat_truncate_cascade_child VALUES (10, 1), (20, 2)",
            ],
        )
        .await
        .expect("create parent and referencing rows");

        // Act
        let truncated = client
            .batch_execute("TRUNCATE TABLE compat_truncate_cascade_parent CASCADE")
            .await;
        let remaining = if truncated.is_ok() {
            Some(
                client
                .query_one(
                    "SELECT (SELECT count(*) FROM compat_truncate_cascade_parent), \
                     (SELECT count(*) FROM compat_truncate_cascade_child)",
                    &[],
                )
                .await
                .expect("count rows after cascading truncate"),
            )
        } else {
            None
        };

        // Assert
        let truncate_sqlstate = truncated.as_ref().err().map(|error| db_error(error).code().code());
        drop(client);
        server.shutdown(connection).await;

        assert!(
            truncated.is_ok(),
            "TRUNCATE CASCADE should remove dependent rows; SQLSTATE: {truncate_sqlstate:?}"
        );
        let remaining = remaining.expect("successful TRUNCATE CASCADE should produce row counts");
        assert_eq!(remaining.get::<_, i64>(0), 0);
        assert_eq!(remaining.get::<_, i64>(1), 0);
    });
}
