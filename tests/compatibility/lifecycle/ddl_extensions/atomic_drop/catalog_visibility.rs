use super::*;

#[test]
fn should_publish_transactional_table_catalog_rows_after_commit() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("transactional_table_catalog_visibility").await;
        let (owner, owner_connection) = server.connect().await;
        let (observer, observer_connection) = server.connect().await;

        // Act
        owner
            .batch_execute("BEGIN")
            .await
            .expect("begin a transaction that will create a table");
        owner
            .batch_execute("CREATE TABLE compat_catalog_visibility_rows (id INT PRIMARY KEY)")
            .await
            .expect("create a table before committing its catalog row");
        owner
            .execute(
                "INSERT INTO compat_catalog_visibility_rows VALUES (18)",
                &[],
            )
            .await
            .expect("write to the transaction-local table");
        let owner_count: i64 = owner
            .query_one(
                "SELECT count(*) FROM pg_catalog.pg_class \
                 WHERE relname = 'compat_catalog_visibility_rows'",
                &[],
            )
            .await
            .expect("read the table catalog entry in its creating transaction")
            .try_get(0)
            .expect("owner catalog count");
        let observer_before_commit: i64 = observer
            .query_one(
                "SELECT count(*) FROM pg_catalog.pg_class \
                 WHERE relname = 'compat_catalog_visibility_rows'",
                &[],
            )
            .await
            .expect("read the table catalog from an independent session")
            .try_get(0)
            .expect("observer catalog count before commit");
        owner
            .batch_execute("COMMIT")
            .await
            .expect("publish the table and its row at commit");
        let observer_after_commit: i64 = observer
            .query_one(
                "SELECT count(*) FROM pg_catalog.pg_class \
                 WHERE relname = 'compat_catalog_visibility_rows'",
                &[],
            )
            .await
            .expect("read the committed table catalog entry")
            .try_get(0)
            .expect("observer catalog count after commit");
        let committed_row = observer
            .query_one("SELECT id FROM compat_catalog_visibility_rows", &[])
            .await
            .expect("read the row after its table was committed");

        // Assert
        assert_eq!(owner_count, 1);
        assert_eq!(observer_before_commit, 0);
        assert_eq!(observer_after_commit, 1);
        assert_eq!(committed_row.get::<_, i32>(0), 18);

        drop((owner, observer));
        server.shutdown(owner_connection).await;
        observer_connection.abort();
        let _ = observer_connection.await;
    });
}

#[test]
fn should_hide_uncommitted_alter_from_other_sessions() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("transactional_alter_catalog_visibility").await;
        let (owner, owner_connection) = server.connect().await;
        let (observer, observer_connection) = server.connect().await;
        execute_statements(
            &owner,
            &[
                "CREATE TABLE compat_alter_catalog_visibility (id INT PRIMARY KEY, value TEXT)",
                "INSERT INTO compat_alter_catalog_visibility VALUES (1, 'before')",
            ],
        )
        .await
        .expect("create and seed the table before the transaction");
        owner
            .batch_execute("BEGIN")
            .await
            .expect("begin the transactional ALTER");

        // Act
        let alter = owner
            .batch_execute("ALTER TABLE compat_alter_catalog_visibility ADD COLUMN note TEXT")
            .await;
        let alter_sqlstate = alter
            .as_ref()
            .err()
            .map(|error| db_error(error).code().code().to_owned());
        let observations = if alter.is_ok() {
            owner
                .batch_execute(
                    "UPDATE compat_alter_catalog_visibility SET note = 'staged' WHERE id = 1",
                )
                .await
                .expect("write through the transaction-local column");
            let owner_columns: i64 = owner
                .query_one(
                    "SELECT count(*) FROM pg_catalog.pg_attribute \
                     WHERE attrelid = 'compat_alter_catalog_visibility'::regclass \
                       AND attname = 'note' AND NOT attisdropped",
                    &[],
                )
                .await
                .expect("inspect the ALTER in its creating transaction")
                .try_get(0)
                .expect("owner column count");
            let observer_columns: i64 = observer
                .query_one(
                    "SELECT count(*) FROM pg_catalog.pg_attribute \
                     WHERE attrelid = 'compat_alter_catalog_visibility'::regclass \
                       AND attname = 'note' AND NOT attisdropped",
                    &[],
                )
                .await
                .expect("inspect the catalog from an independent session")
                .try_get(0)
                .expect("observer column count before commit");
            owner
                .batch_execute("ROLLBACK")
                .await
                .expect("roll back the column and its staged data");
            let observer_columns_after_rollback: i64 = observer
                .query_one(
                    "SELECT count(*) FROM pg_catalog.pg_attribute \
                     WHERE attrelid = 'compat_alter_catalog_visibility'::regclass \
                       AND attname = 'note' AND NOT attisdropped",
                    &[],
                )
                .await
                .expect("inspect the catalog after rollback")
                .try_get(0)
                .expect("observer column count after rollback");
            Some((
                owner_columns,
                observer_columns,
                observer_columns_after_rollback,
            ))
        } else {
            owner
                .batch_execute("ROLLBACK")
                .await
                .expect("recover after ALTER TABLE was rejected");
            None
        };
        let committed_value: String = observer
            .query_one(
                "SELECT value FROM compat_alter_catalog_visibility WHERE id = 1",
                &[],
            )
            .await
            .expect("read the original row after rollback")
            .try_get(0)
            .expect("original row value");

        // Assert
        drop((owner, observer));
        server.shutdown(owner_connection).await;
        observer_connection.abort();
        let _ = observer_connection.await;
        assert!(
            alter.is_ok(),
            "PostgreSQL 18.6 permits ALTER TABLE in a transaction; got SQLSTATE {alter_sqlstate:?}"
        );
        let (owner_columns, observer_columns, observer_columns_after_rollback) =
            observations.expect("transactional ALTER visibility observations");
        assert_eq!(owner_columns, 1);
        assert_eq!(observer_columns, 0);
        assert_eq!(observer_columns_after_rollback, 0);
        assert_eq!(committed_value, "before");
    });
}

#[test]
fn should_hide_uncommitted_table_drop_from_other_sessions_until_rollback() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("transactional_drop_catalog_visibility").await;
        let (owner, owner_connection) = server.connect().await;
        let (observer, observer_connection) = server.connect().await;
        execute_statements(
            &owner,
            &[
                "CREATE TABLE compat_drop_catalog_visibility \
                 (id INT PRIMARY KEY, value TEXT NOT NULL)",
                "INSERT INTO compat_drop_catalog_visibility VALUES (1, 'retained')",
            ],
        )
        .await
        .expect("create and seed the table before the transactional DROP");
        owner
            .batch_execute("BEGIN")
            .await
            .expect("begin the transaction that will drop the table");

        // Act
        owner
            .batch_execute("DROP TABLE compat_drop_catalog_visibility")
            .await
            .expect("drop the table inside the open transaction");
        let observer_relations: i64 = observer
            .query_one(
                "SELECT count(*) FROM pg_catalog.pg_class \
                 WHERE relname = 'compat_drop_catalog_visibility' AND relkind = 'r'",
                &[],
            )
            .await
            .expect("inspect table catalog visibility from another session")
            .try_get(0)
            .expect("observer relation count before rollback");
        owner
            .batch_execute("ROLLBACK")
            .await
            .expect("roll back the uncommitted table drop");
        let restored_row = observer
            .query_one(
                "SELECT id, value FROM compat_drop_catalog_visibility ORDER BY id",
                &[],
            )
            .await
            .expect("read the table after rollback restores it");

        // Assert
        assert_eq!(observer_relations, 1);
        assert_eq!(restored_row.get::<_, i32>(0), 1);
        assert_eq!(restored_row.get::<_, String>(1), "retained");

        drop((owner, observer));
        server.shutdown(owner_connection).await;
        observer_connection.abort();
        let _ = observer_connection.await;
    });
}
