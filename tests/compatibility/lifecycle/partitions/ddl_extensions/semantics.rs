#[test]
fn should_route_rows_to_the_matching_range_partition() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("range_partition_routing").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_range_parent (id INT, label TEXT) PARTITION BY RANGE (id)",
                "CREATE TABLE compat_range_low PARTITION OF compat_range_parent \
                   FOR VALUES FROM (MINVALUE) TO (10)",
                "CREATE TABLE compat_range_high PARTITION OF compat_range_parent \
                   FOR VALUES FROM (10) TO (MAXVALUE)",
                "INSERT INTO compat_range_parent VALUES (1, 'low'), (10, 'high')",
            ],
        )
        .await
        .expect("create range partitions and route boundary rows");

        // Act
        let rows = client
            .query("SELECT id, label FROM compat_range_parent ORDER BY id", &[])
            .await
            .expect("read routed rows through the partitioned parent");

        // Assert
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].get::<_, i32>(0), 1);
        assert_eq!(rows[0].get::<_, String>(1), "low");
        assert_eq!(rows[1].get::<_, i32>(0), 10);
        assert_eq!(rows[1].get::<_, String>(1), "high");

        drop(client);
        server.shutdown(connection).await;
    });
}
#[test]
fn should_route_rows_to_the_matching_hash_partition() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("hash_partition_routing").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_hash_parent (id INT, label TEXT) PARTITION BY HASH (id)",
                "CREATE TABLE compat_hash_even PARTITION OF compat_hash_parent \
                   FOR VALUES WITH (MODULUS 2, REMAINDER 0)",
                "CREATE TABLE compat_hash_odd PARTITION OF compat_hash_parent \
                   FOR VALUES WITH (MODULUS 2, REMAINDER 1)",
                "INSERT INTO compat_hash_parent VALUES \
                   (1, 'one'), (2, 'two'), (3, 'three'), (4, 'four')",
            ],
        )
        .await
        .expect("create hash partitions and route rows");

        // Act
        let rows = client
            .query("SELECT id, label FROM compat_hash_parent ORDER BY id", &[])
            .await
            .expect("read routed rows through the partitioned parent");

        // Assert
        assert_eq!(
            rows.iter()
                .map(|row| (row.get::<_, i32>(0), row.get::<_, String>(1)))
                .collect::<Vec<_>>(),
            vec![
                (1, "one".to_string()),
                (2, "two".to_string()),
                (3, "three".to_string()),
                (4, "four".to_string())
            ]
        );

        drop(client);
        server.shutdown(connection).await;
    });
}
#[test]
fn should_route_unlisted_values_to_a_default_list_partition() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("default_list_partition_routing").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_default_parent (id INT, region TEXT) \
                   PARTITION BY LIST (region)",
                "CREATE TABLE compat_default_east PARTITION OF compat_default_parent \
                   FOR VALUES IN ('east')",
                "CREATE TABLE compat_default_fallback PARTITION OF compat_default_parent DEFAULT",
            ],
        )
        .await
        .expect("create explicit and default partitions");

        // Act
        client
            .batch_execute("INSERT INTO compat_default_parent VALUES (1, 'east'), (2, 'west')")
            .await
            .expect("route explicit and unlisted values");
        let parent_rows = client
            .query(
                "SELECT id, region FROM compat_default_parent ORDER BY id",
                &[],
            )
            .await
            .expect("read rows through the partitioned parent");
        let fallback_count: i64 = client
            .query_one("SELECT count(*) FROM compat_default_fallback", &[])
            .await
            .expect("count rows routed to the default partition")
            .try_get(0)
            .expect("fallback row count");

        // Assert
        assert_eq!(
            parent_rows
                .iter()
                .map(|row| (row.get::<_, i32>(0), row.get::<_, String>(1)))
                .collect::<Vec<_>>(),
            vec![(1, "east".to_string()), (2, "west".to_string())]
        );
        assert_eq!(fallback_count, 1);

        drop(client);
        server.shutdown(connection).await;
    });
}
#[test]
fn should_set_partition_pruning_compatibility_setting() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("partition_pruning_setting").await;
        let (client, connection) = server.connect().await;

        // Act
        client
            .batch_execute("SET enable_partition_pruning = off")
            .await
            .expect("accept the PostgreSQL partition-pruning setting");
        let setting: String = client
            .query_one("SHOW enable_partition_pruning", &[])
            .await
            .expect("read the configured partition-pruning setting")
            .try_get(0)
            .expect("partition-pruning setting");

        // Assert
        assert_eq!(setting, "off");

        drop(client);
        server.shutdown(connection).await;
    });
}
