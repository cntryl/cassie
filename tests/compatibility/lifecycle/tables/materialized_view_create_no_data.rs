use super::*;

#[test]
fn should_create_unpopulated_materialized_view_then_refresh_its_rows() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("materialized_view_create_no_data").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_mv_create_no_data_source \
                   (id INT PRIMARY KEY, label TEXT NOT NULL)",
                "INSERT INTO compat_mv_create_no_data_source VALUES \
                   (1, 'one'), (2, 'two')",
                "CREATE MATERIALIZED VIEW compat_mv_create_no_data \
                   (item_id, item_label) AS \
                   SELECT id, label FROM compat_mv_create_no_data_source \
                   ORDER BY id WITH NO DATA",
            ],
        )
        .await
        .expect("create a materialized view without initially populating it");
        let initially_populated: bool = client
            .query_one(
                "SELECT ispopulated FROM pg_catalog.pg_matviews \
                 WHERE matviewname = 'compat_mv_create_no_data'",
                &[],
            )
            .await
            .expect("inspect the newly created materialized-view state")
            .get(0);

        // Act
        let unpopulated_scan = client
            .query(
                "SELECT item_id FROM compat_mv_create_no_data ORDER BY item_id",
                &[],
            )
            .await
            .expect_err("an unpopulated materialized view cannot be scanned");
        client
            .batch_execute("REFRESH MATERIALIZED VIEW compat_mv_create_no_data WITH DATA")
            .await
            .expect("populate the view from its source rows");
        let populated: bool = client
            .query_one(
                "SELECT ispopulated FROM pg_catalog.pg_matviews \
                 WHERE matviewname = 'compat_mv_create_no_data'",
                &[],
            )
            .await
            .expect("inspect materialized-view state after refresh")
            .get(0);
        let rows = client
            .query(
                "SELECT item_id, item_label FROM compat_mv_create_no_data \
                 ORDER BY item_id",
                &[],
            )
            .await
            .expect("read the refreshed materialized-view rows");

        // Assert
        assert!(!initially_populated);
        assert_eq!(db_error(&unpopulated_scan).code().code(), "55000");
        assert!(populated);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].get::<_, i32>(0), 1);
        assert_eq!(rows[0].get::<_, String>(1), "one");
        assert_eq!(rows[1].get::<_, i32>(0), 2);
        assert_eq!(rows[1].get::<_, String>(1), "two");

        drop(client);
        server.shutdown(connection).await;
    });
}
