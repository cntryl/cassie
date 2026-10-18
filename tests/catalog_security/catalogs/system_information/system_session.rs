#[test]
fn should_observe_the_current_backend_in_pg_stat_activity() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let (server, client, connection, path) =
            start_pgwire_fixture("issue-817-current-pg-stat-activity").await;

        // Act
        let row = client
            .query_one(
                "SELECT count(*)::bigint, \
                        bool_and(pid = pg_backend_pid()), \
                        bool_and(datname = current_database()), \
                        bool_and(usename = current_user), \
                        bool_and(backend_type = 'client backend'), \
                        bool_and(state = 'active'), \
                        bool_and(query IS NOT NULL) \
                 FROM pg_catalog.pg_stat_activity \
                 WHERE pid = pg_backend_pid()",
                &[],
            )
            .await
            .expect("read the activity row belonging to this connection");

        // Assert
        assert_eq!(row.get::<_, i64>(0), 1);
        assert_eq!(
            (1..7)
                .map(|index| row.get::<_, bool>(index))
                .collect::<Vec<_>>(),
            vec![true; 6]
        );

        stop_pgwire_fixture(server, client, connection, path).await;
    });
}
