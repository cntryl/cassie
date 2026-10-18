//! PostgreSQL 18.6 inheritance contracts tracked by #909.

use super::*;

fn lifecycle_wave3_runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("current-thread Tokio runtime")
}

#[test]
fn should_merge_shared_columns_from_multiple_inheritance_parents() {
    // Arrange
    let runtime = lifecycle_wave3_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("multiple_inheritance_shared_columns").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE compat_wave3_parent_a (shared_id INT, a_value TEXT)")
            .await
            .expect("create the first parent relation");
        client
            .batch_execute("CREATE TABLE compat_wave3_parent_b (shared_id INT, b_value BOOL)")
            .await
            .expect("create the second parent relation");
        client
            .batch_execute(
                "CREATE TABLE compat_wave3_child (child_value BIGINT) \
                 INHERITS (compat_wave3_parent_a, compat_wave3_parent_b)",
            )
            .await
            .expect("create a child with both inheritance parents");
        client
            .batch_execute(
                "INSERT INTO compat_wave3_child \
                 (shared_id, a_value, b_value, child_value) \
                 VALUES (7, 'from-a', true, 99)",
            )
            .await
            .expect("insert a row that uses each inherited and local column");

        // Act
        let inherited_attributes = client
            .query(
                "SELECT attname, attinhcount, attislocal \
                 FROM pg_catalog.pg_attribute \
                 WHERE attrelid = 'compat_wave3_child'::regclass AND attnum > 0 \
                 ORDER BY attnum",
                &[],
            )
            .await
            .expect("inspect the merged inherited-column metadata");
        let rows_read_from_parent_a = client
            .query(
                "SELECT shared_id, a_value \
                 FROM compat_wave3_parent_a",
                &[],
            )
            .await
            .expect("scan the first parent and its child");
        let rows_read_from_parent_b = client
            .query(
                "SELECT shared_id, b_value \
                 FROM compat_wave3_parent_b",
                &[],
            )
            .await
            .expect("scan the second parent and its child");
        let only_parent_a_rows = client
            .query("SELECT shared_id FROM ONLY compat_wave3_parent_a", &[])
            .await
            .expect("exclude inherited rows when scanning ONLY the first parent");

        // Assert
        assert_eq!(
            inherited_attributes
                .iter()
                .map(|row| (
                    row.get::<_, String>(0),
                    row.get::<_, i16>(1),
                    row.get::<_, bool>(2),
                ))
                .collect::<Vec<_>>(),
            vec![
                ("shared_id".to_owned(), 2, false),
                ("a_value".to_owned(), 1, false),
                ("b_value".to_owned(), 1, false),
                ("child_value".to_owned(), 0, true),
            ]
        );
        assert_eq!(rows_read_from_parent_a.len(), 1);
        assert_eq!(rows_read_from_parent_a[0].get::<_, i32>(0), 7);
        assert_eq!(rows_read_from_parent_a[0].get::<_, String>(1), "from-a");
        assert_eq!(rows_read_from_parent_b.len(), 1);
        assert_eq!(rows_read_from_parent_b[0].get::<_, i32>(0), 7);
        assert!(rows_read_from_parent_b[0].get::<_, bool>(1));
        assert!(only_parent_a_rows.is_empty());

        drop(client);
        server.shutdown(connection).await;
    });
}
