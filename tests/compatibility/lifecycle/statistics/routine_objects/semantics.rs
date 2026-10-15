#[test]
fn should_hide_rolled_back_extended_statistics_from_other_sessions() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("extended_statistics_rollback").await;
        let (owner, owner_connection) = server.connect().await;
        let (observer, observer_connection) = server.connect().await;
        owner
            .batch_execute(
                "CREATE TABLE compat_stats_rollback_rows (left_value INT, right_value INT)",
            )
            .await
            .expect("create the statistics relation");

        // Act
        owner
            .batch_execute("BEGIN")
            .await
            .expect("begin a statistics transaction");
        owner
            .batch_execute(
                "CREATE STATISTICS compat_stats_rollback (dependencies) \
                 ON left_value, right_value FROM compat_stats_rollback_rows",
            )
            .await
            .expect("create extended statistics transactionally");
        let owner_count: i64 = owner
            .query_one(
                "SELECT count(*) FROM pg_catalog.pg_statistic_ext \
                 WHERE stxname = 'compat_stats_rollback'",
                &[],
            )
            .await
            .expect("read the owner's uncommitted statistics")
            .try_get(0)
            .expect("owner count");
        let observer_before: i64 = observer
            .query_one(
                "SELECT count(*) FROM pg_catalog.pg_statistic_ext \
                 WHERE stxname = 'compat_stats_rollback'",
                &[],
            )
            .await
            .expect("read statistics from an independent session")
            .try_get(0)
            .expect("observer count before rollback");
        owner
            .batch_execute("ROLLBACK")
            .await
            .expect("roll back statistics DDL");
        let observer_after: i64 = observer
            .query_one(
                "SELECT count(*) FROM pg_catalog.pg_statistic_ext \
                 WHERE stxname = 'compat_stats_rollback'",
                &[],
            )
            .await
            .expect("verify rollback removed the catalog row")
            .try_get(0)
            .expect("observer count after rollback");

        // Assert
        assert_eq!(owner_count, 1);
        assert_eq!(observer_before, 0);
        assert_eq!(observer_after, 0);

        drop(owner);
        drop(observer);
        server.shutdown(owner_connection).await;
        observer_connection.abort();
        let _ = observer_connection.await;
    });
}
