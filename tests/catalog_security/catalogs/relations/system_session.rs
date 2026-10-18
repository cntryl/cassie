#[test]
fn should_observe_the_current_relation_lock_in_pg_locks() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let (server, client, connection, path) =
            start_pgwire_fixture("issue-817-current-pg-locks").await;
        client
            .batch_execute("CREATE TABLE issue_817_lock_observation (id INTEGER)")
            .await
            .expect("create the relation whose lock is observed");
        client
            .batch_execute("BEGIN")
            .await
            .expect("begin the lock observation transaction");
        client
            .batch_execute("LOCK TABLE issue_817_lock_observation IN ACCESS SHARE MODE")
            .await
            .expect("hold a relation lock for the catalog observation");

        // Act
        let row = client
            .query_one(
                "SELECT count(*)::bigint, \
                        bool_and(locktype = 'relation'), \
                        bool_and(relation = 'issue_817_lock_observation'::regclass), \
                        bool_and(pid = pg_backend_pid()), \
                        bool_and(mode = 'AccessShareLock'), \
                        bool_and(granted) \
                 FROM pg_catalog.pg_locks \
                 WHERE pid = pg_backend_pid() \
                   AND relation = 'issue_817_lock_observation'::regclass",
                &[],
            )
            .await
            .expect("read this connection's granted relation lock");

        // Assert
        assert_eq!(row.get::<_, i64>(0), 1);
        assert_eq!(
            (1..6)
                .map(|index| row.get::<_, bool>(index))
                .collect::<Vec<_>>(),
            vec![true; 5]
        );

        client
            .batch_execute("ROLLBACK; DROP TABLE issue_817_lock_observation")
            .await
            .expect("release the lock and remove the fixture table");
        stop_pgwire_fixture(server, client, connection, path).await;
    });
}
