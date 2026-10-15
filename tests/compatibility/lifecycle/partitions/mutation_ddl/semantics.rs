#[test]
fn should_route_inserts_to_the_matching_list_partition() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("list_partition_routing").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_partitioned_rows (id INT, region TEXT) PARTITION BY LIST (region)",
                "CREATE TABLE compat_partitioned_north PARTITION OF compat_partitioned_rows \
                   FOR VALUES IN ('north')",
                "INSERT INTO compat_partitioned_rows VALUES (1, 'north')",
            ],
        )
            .await
            .expect("create partition and inheritance fixtures");

        // Act
        let partition_rows = client
            .query(
                "SELECT id FROM compat_partitioned_rows ORDER BY id",
                &[],
            )
            .await
            .expect("scan partitioned parent");
        let child_rows = client
            .query(
                "SELECT id FROM compat_partitioned_north ORDER BY id",
                &[],
            )
            .await
            .expect("scan the destination partition");

        // Assert
        assert_eq!(partition_rows.len(), 1);
        assert_eq!(partition_rows[0].get::<_, i32>(0), 1);
        assert_eq!(child_rows.len(), 1);
        assert_eq!(child_rows[0].get::<_, i32>(0), 1);

        drop(client);
        server.shutdown(connection).await;
    });
}
