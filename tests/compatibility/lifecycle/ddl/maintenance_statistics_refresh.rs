use super::*;

async fn estimated_rows(client: &tokio_postgres::Client) -> (u32, f32) {
    let row = client
        .query_one(
            "SELECT reltuples FROM pg_catalog.pg_class \
             WHERE oid = 'compat_analyze_reestimate_rows'::regclass",
            &[],
        )
        .await
        .expect("read the table's PostgreSQL row estimate");
    let type_oid = row.columns()[0].type_().oid();
    let estimate = row.try_get(0).expect("decode the real-valued row estimate");
    (type_oid, estimate)
}

#[test]
fn should_refresh_pg_class_row_estimate_after_analyze() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("analyze_row_estimate_refresh").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_analyze_reestimate_rows \
                   (id INT PRIMARY KEY, sample TEXT)",
                "INSERT INTO compat_analyze_reestimate_rows VALUES \
                   (1, 'before'), (2, 'before'), (3, 'before'), (4, 'before')",
            ],
        )
        .await
        .expect("create a four-row table before collecting statistics");

        // Act
        client
            .batch_execute("ANALYZE compat_analyze_reestimate_rows")
            .await
            .expect("collect the initial table statistics");
        let initial_estimate = estimated_rows(&client).await;
        client
            .batch_execute(
                "INSERT INTO compat_analyze_reestimate_rows VALUES \
                   (5, 'after'), (6, 'after'), (7, 'after')",
            )
            .await
            .expect("grow the table after the initial statistics collection");
        let stale_estimate = estimated_rows(&client).await;
        client
            .batch_execute("ANALYZE compat_analyze_reestimate_rows")
            .await
            .expect("refresh the row estimate after table growth");
        let refreshed_estimate = estimated_rows(&client).await;
        let visible_rows: i64 = client
            .query_one("SELECT count(*) FROM compat_analyze_reestimate_rows", &[])
            .await
            .expect("count rows after refreshing statistics")
            .try_get(0)
            .expect("visible row count");

        // Assert
        assert_eq!(initial_estimate.0, 700, "pg_class.reltuples has OID REAL");
        assert_eq!(initial_estimate.1, 4.0);
        assert_eq!(stale_estimate, initial_estimate);
        assert_eq!(refreshed_estimate, (700, 7.0));
        assert_eq!(visible_rows, 7);

        drop(client);
        server.shutdown(connection).await;
    });
}
