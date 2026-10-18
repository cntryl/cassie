use super::*;

fn assert_nested_list_partition_routing(
    root_rows: &[tokio_postgres::Row],
    counts: &tokio_postgres::Row,
) {
    let observed_rows = root_rows
        .iter()
        .map(|row| {
            (
                row.get::<_, i32>(0),
                row.get::<_, String>(1),
                row.get::<_, String>(2),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        observed_rows,
        vec![
            (0, "a".to_owned(), "early-a".to_owned()),
            (1, "b".to_owned(), "early-b".to_owned()),
            (100, "a".to_owned(), "late-boundary".to_owned()),
            (101, "z".to_owned(), "late-other".to_owned()),
        ]
    );
    assert_eq!(
        (
            counts.get::<_, i64>(0),
            counts.get::<_, i64>(1),
            counts.get::<_, i64>(2),
            counts.get::<_, i64>(3),
        ),
        (2, 1, 1, 2),
        "the RANGE upper bound 100 belongs to the later partition"
    );
}

#[test]
fn should_route_range_boundary_rows_through_nested_list_partitions() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("multilevel_partition_routing").await;
        let (client, connection) = server.connect().await;
        let setup = execute_statements(
            &client,
            &[
                "CREATE TABLE compat_multilevel_partition_root \
                   (id INT, bucket TEXT, label TEXT) PARTITION BY RANGE (id)",
                "CREATE TABLE compat_multilevel_partition_early \
                   PARTITION OF compat_multilevel_partition_root \
                   FOR VALUES FROM (MINVALUE) TO (100) PARTITION BY LIST (bucket)",
                "CREATE TABLE compat_multilevel_partition_late \
                   PARTITION OF compat_multilevel_partition_root \
                   FOR VALUES FROM (100) TO (MAXVALUE)",
                "CREATE TABLE compat_multilevel_partition_early_a \
                   PARTITION OF compat_multilevel_partition_early FOR VALUES IN ('a')",
                "CREATE TABLE compat_multilevel_partition_early_b \
                   PARTITION OF compat_multilevel_partition_early FOR VALUES IN ('b')",
                "INSERT INTO compat_multilevel_partition_root VALUES \
                   (0, 'a', 'early-a'), (1, 'b', 'early-b'), \
                   (100, 'a', 'late-boundary'), (101, 'z', 'late-other')",
            ],
        )
        .await;

        // Act
        let root_rows = if setup.is_ok() {
            Some(
                client
                    .query(
                        "SELECT id, bucket, label FROM compat_multilevel_partition_root \
                         ORDER BY id",
                        &[],
                    )
                    .await,
            )
        } else {
            None
        };
        let partition_counts = if setup.is_ok() {
            Some(
                client
                    .query_one(
                        "SELECT \
                           (SELECT count(*) FROM compat_multilevel_partition_early), \
                           (SELECT count(*) FROM compat_multilevel_partition_early_a), \
                           (SELECT count(*) FROM compat_multilevel_partition_early_b), \
                           (SELECT count(*) FROM compat_multilevel_partition_late)",
                        &[],
                    )
                    .await,
            )
        } else {
            None
        };

        drop(client);
        server.shutdown(connection).await;

        // Assert
        assert!(
            setup.is_ok(),
            "create nested RANGE/LIST partitions and route rows: {setup:?}"
        );
        let root_rows = root_rows
            .expect("query the partitioned root after setup")
            .expect("read rows through the partitioned root");
        let counts = partition_counts
            .expect("count rows at each level after setup")
            .expect("read nested partition counts");
        assert_nested_list_partition_routing(&root_rows, &counts);
    });
}
