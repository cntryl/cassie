#[test]
fn should_rollback_data_modifying_cte_on_later_error() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("modifying_cte_atomicity").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE compat_cte_mutations (id INT PRIMARY KEY, label TEXT)")
            .await
            .expect("create CTE target");

        // Act
        let returned = client
            .query(
                "WITH inserted AS ( \
                   INSERT INTO compat_cte_mutations VALUES (1, 'created') RETURNING id, label \
                 ) SELECT id, label FROM inserted",
                &[],
            )
            .await
            .expect("read the data-modifying CTE RETURNING relation");
        let failed = client
            .query(
                "WITH inserted AS ( \
                   INSERT INTO compat_cte_mutations VALUES (2, 'rolled-back') RETURNING id \
                 ) SELECT id / 0 FROM inserted",
                &[],
            )
            .await
            .expect_err("later error should fail the complete CTE statement");
        let rows = client
            .query(
                "SELECT id, label FROM compat_cte_mutations ORDER BY id",
                &[],
            )
            .await
            .expect("read rows after failed CTE");

        // Assert
        assert_eq!(returned.len(), 1);
        assert_eq!(returned[0].get::<_, i32>(0), 1);
        assert_eq!(returned[0].get::<_, String>(1), "created");
        assert_eq!(db_error(&failed).code().code(), "22012");
        assert_eq!(rows.len(), 1, "failed CTE must not publish its insert");
        assert_eq!(rows[0].get::<_, i32>(0), 1);

        drop(client);
        server.shutdown(connection).await;
    });
}
