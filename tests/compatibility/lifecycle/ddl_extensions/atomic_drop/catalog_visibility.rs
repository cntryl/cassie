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
