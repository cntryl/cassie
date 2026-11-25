#[test]
fn should_refresh_a_materialized_view_with_no_data() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("materialized_view_lifecycle").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_mv_source (id INT PRIMARY KEY, label TEXT)",
                "INSERT INTO compat_mv_source VALUES (1, 'one'), (2, 'two')",
                "CREATE MATERIALIZED VIEW compat_mv_rows (item_id, item_label) AS \
                   SELECT id, label FROM compat_mv_source WITH NO DATA",
            ],
        )
        .await
        .expect("create materialized view without initial data");

        // Act
        let unpopulated = client
            .query_one("SELECT count(*) FROM compat_mv_rows", &[])
            .await
            .expect_err("a materialized view created WITH NO DATA is not scannable");
        client
            .batch_execute("REFRESH MATERIALIZED VIEW compat_mv_rows")
            .await
            .expect("refresh materialized view");
        let populated = client
            .query(
                "SELECT item_id, item_label FROM compat_mv_rows ORDER BY item_id",
                &[],
            )
            .await
            .expect("read refreshed materialized view");

        // Assert
        assert_eq!(db_error(&unpopulated).code().code(), "55000");
        assert_eq!(populated.len(), 2);
        assert_eq!(populated[0].get::<_, i32>(0), 1);
        assert_eq!(populated[0].get::<_, String>(1), "one");
        assert_eq!(populated[1].get::<_, i32>(0), 2);
        assert_eq!(populated[1].get::<_, String>(1), "two");

        drop(client);
        server.shutdown(connection).await;
    });
}
