#[test]
fn should_exclude_descendant_rows_when_updating_only_parent() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("inheritance_update_only_scope").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_update_only_parent \
                   (id INT PRIMARY KEY, parent_value TEXT NOT NULL)",
                "CREATE TABLE compat_update_only_child \
                   (id INT PRIMARY KEY, parent_value TEXT NOT NULL, child_note TEXT)",
                "ALTER TABLE compat_update_only_child INHERIT compat_update_only_parent",
                "INSERT INTO compat_update_only_parent VALUES (2, 'parent')",
                "INSERT INTO compat_update_only_child VALUES (1, 'child', 'detail')",
            ],
        )
        .await
        .expect("create matching parent and child relations and attach inheritance");

        // Act
        let updated_rows = client
            .execute(
                "UPDATE ONLY compat_update_only_parent \
                 SET parent_value = 'parent-only' WHERE id IN (1, 2)",
                &[],
            )
            .await
            .expect("update only rows stored in the parent relation");
        let rows = client
            .query(
                "SELECT id, parent_value FROM compat_update_only_parent ORDER BY id",
                &[],
            )
            .await
            .expect("read parent and child rows through the inherited scan");

        // Assert
        assert_eq!(updated_rows, 1);
        assert_eq!(
            rows.iter()
                .map(|row| (row.get::<_, i32>(0), row.get::<_, String>(1)))
                .collect::<Vec<_>>(),
            vec![(1, "child".to_owned()), (2, "parent-only".to_owned())]
        );

        drop(client);
        server.shutdown(connection).await;
    });
}
