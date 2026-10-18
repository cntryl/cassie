#[test]
fn should_analyze_only_partitioned_parent() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("analyze_only_partitioned_parent").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_analyze_only_parent \
                   (id INT, label TEXT) PARTITION BY RANGE (id)",
                "CREATE TABLE compat_analyze_only_child \
                   PARTITION OF compat_analyze_only_parent \
                   FOR VALUES FROM (0) TO (10)",
                "INSERT INTO compat_analyze_only_parent \
                   VALUES (1, 'common'), (2, 'common')",
            ],
        )
        .await
        .expect("create a populated partitioned table");

        // Act
        client
            .batch_execute("ANALYZE ONLY compat_analyze_only_parent (label)")
            .await
            .expect("analyze only the partitioned parent and selected column");
        let analyze_states = client
            .query(
                "SELECT relname, last_analyze IS NOT NULL \
                 FROM pg_catalog.pg_stat_user_tables \
                 WHERE schemaname = 'public' \
                   AND relname IN ('compat_analyze_only_parent', \
                                   'compat_analyze_only_child') \
                 ORDER BY relname",
                &[],
            )
            .await
            .expect("read parent and child analyze timestamps");
        let statistics = client
            .query(
                "SELECT tablename, attname FROM pg_catalog.pg_stats \
                 WHERE schemaname = 'public' \
                   AND tablename IN ('compat_analyze_only_parent', \
                                     'compat_analyze_only_child') \
                 ORDER BY tablename, attname",
                &[],
            )
            .await
            .expect("read parent and child column statistics");

        // Assert
        assert_eq!(analyze_states.len(), 2);
        assert_eq!(
            analyze_states[0].get::<_, String>(0),
            "compat_analyze_only_child"
        );
        assert!(!analyze_states[0].get::<_, bool>(1));
        assert_eq!(
            analyze_states[1].get::<_, String>(0),
            "compat_analyze_only_parent"
        );
        assert!(analyze_states[1].get::<_, bool>(1));
        assert_eq!(statistics.len(), 1);
        assert_eq!(
            statistics[0].get::<_, String>(0),
            "compat_analyze_only_parent"
        );
        assert_eq!(statistics[0].get::<_, String>(1), "label");

        drop(client);
        server.shutdown(connection).await;
    });
}
#[test]
fn should_leave_partitioned_parent_unvacuumed_with_only_option() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("vacuum_only_partitioned_parent").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_vacuum_only_parent \
                   (id INT, label TEXT) PARTITION BY RANGE (id)",
                "CREATE TABLE compat_vacuum_only_child \
                   PARTITION OF compat_vacuum_only_parent \
                   FOR VALUES FROM (0) TO (10)",
                "INSERT INTO compat_vacuum_only_parent VALUES (1, 'row')",
            ],
        )
        .await
        .expect("create a partitioned table with one child row");

        // Act
        client
            .batch_execute("VACUUM ONLY compat_vacuum_only_parent")
            .await
            .expect("accept PostgreSQL 18 VACUUM ONLY for a partitioned parent");
        let vacuumed_relations: i64 = client
            .query_one(
                "SELECT count(*) FROM pg_catalog.pg_stat_user_tables \
                 WHERE schemaname = 'public' \
                   AND relname IN ('compat_vacuum_only_parent', \
                                   'compat_vacuum_only_child') \
                   AND last_vacuum IS NOT NULL",
                &[],
            )
            .await
            .expect("check whether the partitioned parent or child was vacuumed")
            .try_get(0)
            .expect("vacuumed relation count");

        // Assert
        assert_eq!(vacuumed_relations, 0);

        drop(client);
        server.shutdown(connection).await;
    });
}
