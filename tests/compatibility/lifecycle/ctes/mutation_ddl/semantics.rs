#[test]
fn should_attach_a_non_modifying_cte_to_update() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("cte_attached_to_update").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_cte_update_target (id INT PRIMARY KEY, label TEXT)",
                "CREATE TABLE compat_cte_update_source (id INT, label TEXT)",
                "INSERT INTO compat_cte_update_target VALUES (1, 'old'), (2, 'keep')",
                "INSERT INTO compat_cte_update_source VALUES (1, 'from-cte')",
            ],
        )
        .await
        .expect("create CTE UPDATE fixtures");

        // Act
        let changed = client
            .query(
                "WITH selected AS (SELECT id, label FROM compat_cte_update_source) \
                 UPDATE compat_cte_update_target AS target \
                 SET label = selected.label \
                 FROM selected WHERE target.id = selected.id \
                 RETURNING target.id, target.label",
                &[],
            )
            .await
            .expect("execute UPDATE with a non-modifying WITH query");

        // Assert
        assert_eq!(changed.len(), 1);
        assert_eq!(changed[0].get::<_, i32>(0), 1);
        assert_eq!(changed[0].get::<_, String>(1), "from-cte");
        let unchanged: String = client
            .query_one(
                "SELECT label FROM compat_cte_update_target WHERE id = 2",
                &[],
            )
            .await
            .expect("read the untouched target row")
            .get(0);
        assert_eq!(unchanged, "keep");

        drop(client);
        server.shutdown(connection).await;
    });
}
