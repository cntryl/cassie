use super::*;

#[test]
fn should_route_range_upper_bound_rows_to_the_default_partition() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("default_range_partition_routing").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat840_range_parent (id INT, label TEXT) \
                   PARTITION BY RANGE (id)",
                "CREATE TABLE compat840_range_low \
                   PARTITION OF compat840_range_parent \
                   FOR VALUES FROM (MINVALUE) TO (10)",
                "CREATE TABLE compat840_range_default \
                   PARTITION OF compat840_range_parent DEFAULT",
            ],
        )
        .await
        .expect("create a bounded range partition and a default partition");

        // Act
        client
            .batch_execute(
                "INSERT INTO compat840_range_parent VALUES \
                   (1, 'low'), (10, 'boundary'), (11, 'default')",
            )
            .await
            .expect("route rows at and above the explicit range upper bound");
        let parent_rows = client
            .query(
                "SELECT id, label FROM compat840_range_parent ORDER BY id",
                &[],
            )
            .await
            .expect("read all inserted rows through the partitioned parent");
        let default_rows = client
            .query("SELECT id FROM compat840_range_default ORDER BY id", &[])
            .await
            .expect("inspect rows stored in the default partition");

        // Assert
        assert_eq!(
            parent_rows
                .iter()
                .map(|row| (row.get::<_, i32>(0), row.get::<_, String>(1)))
                .collect::<Vec<_>>(),
            vec![
                (1, "low".to_owned()),
                (10, "boundary".to_owned()),
                (11, "default".to_owned()),
            ]
        );
        assert_eq!(
            default_rows
                .iter()
                .map(|row| row.get::<_, i32>(0))
                .collect::<Vec<_>>(),
            vec![10, 11],
            "the range upper bound is exclusive, so 10 belongs in DEFAULT"
        );

        drop(client);
        server.shutdown(connection).await;
    });
}
