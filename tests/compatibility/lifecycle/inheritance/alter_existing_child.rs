use super::*;

#[test]
fn should_attach_existing_child_with_pg_inherits_edge() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("inheritance_alter_existing_child").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_attach_parent (id INT, label TEXT)",
                "CREATE TABLE compat_attach_child (id INT, label TEXT, note TEXT)",
                "INSERT INTO compat_attach_parent VALUES (1, 'parent')",
                "INSERT INTO compat_attach_child VALUES (2, 'child', 'retained')",
            ],
        )
        .await
        .expect("create compatible tables and seed rows before attachment");

        // Act
        client
            .batch_execute("ALTER TABLE compat_attach_child INHERIT compat_attach_parent")
            .await
            .expect("attach the pre-existing child to its compatible parent");
        let inherited_rows = client
            .query(
                "SELECT id, label FROM compat_attach_parent ORDER BY id",
                &[],
            )
            .await
            .expect("read parent and newly attached child rows");
        let inheritance_edge = client
            .query(
                "SELECT parent.relname, child.relname, inheritance.inhseqno \
                 FROM pg_catalog.pg_inherits AS inheritance \
                 JOIN pg_catalog.pg_class AS parent \
                   ON parent.oid = inheritance.inhparent \
                 JOIN pg_catalog.pg_class AS child \
                   ON child.oid = inheritance.inhrelid \
                 WHERE parent.relname = 'compat_attach_parent' \
                   AND child.relname = 'compat_attach_child'",
                &[],
            )
            .await
            .expect("inspect the newly published pg_inherits edge");

        // Assert
        assert_eq!(
            inherited_rows
                .iter()
                .map(|row| (row.get::<_, i32>(0), row.get::<_, String>(1)))
                .collect::<Vec<_>>(),
            vec![(1, "parent".to_owned()), (2, "child".to_owned()),]
        );
        assert_eq!(inheritance_edge.len(), 1);
        assert_eq!(
            (
                inheritance_edge[0].get::<_, String>(0),
                inheritance_edge[0].get::<_, String>(1),
                inheritance_edge[0].get::<_, i32>(2),
            ),
            (
                "compat_attach_parent".to_owned(),
                "compat_attach_child".to_owned(),
                1,
            )
        );

        drop(client);
        server.shutdown(connection).await;
    });
}
