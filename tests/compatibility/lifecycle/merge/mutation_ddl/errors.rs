#[test]
fn should_reject_merge_that_would_modify_a_target_row_twice() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("merge_cardinality_violation").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_merge_cardinality_target (id INT PRIMARY KEY, label TEXT)",
                "INSERT INTO compat_merge_cardinality_target VALUES (1, 'old')",
            ],
        )
        .await
        .expect("create MERGE cardinality fixture");

        // Act
        let conflict = client
            .execute(
                "MERGE INTO compat_merge_cardinality_target AS target \
                 USING (VALUES (1, 'first'), (1, 'second')) AS source(id, label) \
                 ON target.id = source.id \
                 WHEN MATCHED THEN UPDATE SET label = source.label",
                &[],
            )
            .await
            .expect_err("a second source match must not modify the same target again");

        // Assert
        assert_eq!(db_error(&conflict).code().code(), "21000");
        let stored_label = client
            .query_one(
                "SELECT label FROM compat_merge_cardinality_target WHERE id = 1",
                &[],
            )
            .await
            .expect("read the target after the rejected MERGE")
            .get::<_, String>(0);
        assert_eq!(stored_label, "old", "the failed MERGE is atomic");

        drop(client);
        server.shutdown(connection).await;
    });
}
