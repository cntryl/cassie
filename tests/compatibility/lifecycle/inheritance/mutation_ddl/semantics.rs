#[test]
fn should_respect_only_parent_during_inheritance_scans() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("table_inheritance_scans").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_inherited_parent (id INT, label TEXT)",
                "CREATE TABLE compat_inherited_child (child_note TEXT) \
                   INHERITS (compat_inherited_parent)",
                "INSERT INTO compat_inherited_parent VALUES (10, 'parent')",
                "INSERT INTO compat_inherited_child VALUES (11, 'child', 'detail')",
            ],
        )
        .await
        .expect("create parent and inherited child fixtures");

        // Act
        let inherited_rows = client
            .query("SELECT id FROM compat_inherited_parent ORDER BY id", &[])
            .await
            .expect("scan parent and inherited descendants");
        let only_parent = client
            .query(
                "SELECT id FROM ONLY compat_inherited_parent ORDER BY id",
                &[],
            )
            .await
            .expect("scan only the inheritance parent");
        let inherited_catalog = client
            .query(
                "SELECT child.relname FROM pg_catalog.pg_inherits AS i \
                 JOIN pg_catalog.pg_class AS parent ON parent.oid = i.inhparent \
                 JOIN pg_catalog.pg_class AS child ON child.oid = i.inhrelid \
                 WHERE parent.relname = 'compat_inherited_parent'",
                &[],
            )
            .await
            .expect("inspect inherited relation dependency");

        // Assert
        assert_eq!(
            inherited_rows
                .iter()
                .map(|row| row.get::<_, i32>(0))
                .collect::<Vec<_>>(),
            vec![10, 11]
        );
        assert_eq!(only_parent.len(), 1);
        assert_eq!(only_parent[0].get::<_, i32>(0), 10);
        assert_eq!(inherited_catalog.len(), 1);
        assert_eq!(
            inherited_catalog[0].get::<_, String>(0),
            "compat_inherited_child"
        );

        drop(client);
        server.shutdown(connection).await;
    });
}
