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
#[test]
fn should_keep_persistent_select_into_table_after_creator_disconnects() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("select_into_persistent_table").await;
        let (owner, owner_connection) = server.connect().await;
        let (observer, observer_connection) = server.connect().await;
        owner
            .batch_execute(
                "SELECT 18 AS id, 'persistent'::TEXT AS label \
                 INTO compat_select_into_persistent",
            )
            .await
            .expect("SELECT INTO without TEMP should create a persistent table");

        // Act
        drop(owner);
        owner_connection.abort();
        let _ = owner_connection.await;
        let persistence = observer
            .query_one(
                "SELECT relpersistence::TEXT FROM pg_class \
                 WHERE oid = 'compat_select_into_persistent'::regclass",
                &[],
            )
            .await
            .expect("inspect persistent SELECT INTO relation");
        let rows = observer
            .query(
                "SELECT id, label FROM compat_select_into_persistent ORDER BY id",
                &[],
            )
            .await
            .expect("read SELECT INTO rows from another session");

        // Assert
        assert_eq!(persistence.get::<_, String>(0), "p");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].get::<_, i32>(0), 18);
        assert_eq!(rows[0].get::<_, String>(1), "persistent");

        drop(observer);
        server.shutdown(observer_connection).await;
    });
}
