use super::*;

#[test]
fn should_create_table_as_an_existing_table_with_column_names() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("ctas_as_table_column_names").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_ctas_table_source (id INT, label TEXT NOT NULL)",
                "INSERT INTO compat_ctas_table_source VALUES (1, 'one'), (2, 'two')",
            ],
        )
        .await
        .expect("create and seed a CTAS table source");

        // Act
        let created = client
            .batch_execute(
                "CREATE TABLE compat_ctas_table_copy (copy_id, copy_label) \
                 AS TABLE compat_ctas_table_source",
            )
            .await;
        let copied_rows = if created.is_ok() {
            Some(
                client
                    .query("SELECT * FROM compat_ctas_table_copy ORDER BY copy_id", &[])
                    .await,
            )
        } else {
            None
        };

        // Assert
        drop(client);
        server.shutdown(connection).await;
        assert!(
            created.is_ok(),
            "CREATE TABLE AS TABLE with an explicit column-name list should succeed: {created:?}"
        );
        let copied_rows = copied_rows
            .expect("successful CTAS should be queryable")
            .expect("read copied table rows");
        assert_eq!(copied_rows.len(), 2);
        assert_eq!(copied_rows[0].columns()[0].name(), "copy_id");
        assert_eq!(copied_rows[0].columns()[0].type_().oid(), 23);
        assert_eq!(copied_rows[0].columns()[1].name(), "copy_label");
        assert_eq!(copied_rows[0].columns()[1].type_().oid(), 25);
        assert_eq!(copied_rows[0].get::<_, i32>(0), 1);
        assert_eq!(copied_rows[0].get::<_, String>(1), "one");
        assert_eq!(copied_rows[1].get::<_, i32>(0), 2);
        assert_eq!(copied_rows[1].get::<_, String>(1), "two");
    });
}

#[test]
fn should_preserve_unlogged_table_persistence_identity() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("unlogged_table_persistence").await;
        let (client, connection) = server.connect().await;
        let created = client
            .batch_execute(
                "CREATE UNLOGGED TABLE compat_unlogged_rows \
                 (id INT PRIMARY KEY, label TEXT)",
            )
            .await;

        // Act
        let (inserted, persistence, rows) = if created.is_ok() {
            let inserted = client
                .execute(
                    "INSERT INTO compat_unlogged_rows VALUES (1, 'ephemeral')",
                    &[],
                )
                .await;
            let persistence = if inserted.is_ok() {
                Some(
                    client
                        .query_one(
                            "SELECT relpersistence::TEXT FROM pg_catalog.pg_class \
                             WHERE relname = 'compat_unlogged_rows'",
                            &[],
                        )
                        .await,
                )
            } else {
                None
            };
            let rows = if inserted.is_ok() {
                Some(
                    client
                        .query(
                            "SELECT id, label FROM compat_unlogged_rows ORDER BY id",
                            &[],
                        )
                        .await,
                )
            } else {
                None
            };
            (Some(inserted), persistence, rows)
        } else {
            (None, None, None)
        };

        // Assert
        drop(client);
        server.shutdown(connection).await;
        assert!(
            created.is_ok(),
            "CREATE UNLOGGED TABLE should retain PostgreSQL persistence semantics: {created:?}"
        );
        assert_eq!(
            inserted
                .expect("insert should be attempted after table creation")
                .expect("insert into unlogged table"),
            1
        );
        assert_eq!(
            persistence
                .expect("read relpersistence after inserting")
                .expect("read unlogged table persistence")
                .try_get::<_, String>(0)
                .expect("relpersistence text"),
            "u"
        );
        let rows = rows
            .expect("read rows after inserting")
            .expect("read rows from unlogged table");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].get::<_, i32>(0), 1);
        assert_eq!(rows[0].get::<_, String>(1), "ephemeral");
    });
}
