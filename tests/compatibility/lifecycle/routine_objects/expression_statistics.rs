use super::*;

#[test]
fn should_collect_expression_statistics_with_selected_target() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("expression_statistics").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_expression_stats (category TEXT)",
                "INSERT INTO compat_expression_stats VALUES ('A'), ('a'), ('b')",
                "CREATE STATISTICS compat_lower_category_stats \
                   ON (lower(category)) FROM compat_expression_stats",
                "ALTER STATISTICS compat_lower_category_stats SET STATISTICS 250",
                "ANALYZE compat_expression_stats",
            ],
        )
        .await
        .expect("collect and alter statistics for a scalar expression");

        // Act
        let catalog = client
            .query_one(
                "SELECT s.stxstattarget, pg_catalog.pg_get_statisticsobjdef(s.oid), \
                        count(d.stxoid) \
                 FROM pg_catalog.pg_statistic_ext AS s \
                 LEFT JOIN pg_catalog.pg_statistic_ext_data AS d ON d.stxoid = s.oid \
                 WHERE s.stxname = 'compat_lower_category_stats' \
                 GROUP BY s.oid, s.stxstattarget",
                &[],
            )
            .await
            .expect("inspect expression statistics and collected data");
        let definition: String = catalog.try_get(1).expect("statistics definition");

        // Assert
        assert_eq!(catalog.get::<_, i32>(0), 250);
        assert!(definition.contains("lower(category)"), "{definition}");
        assert_eq!(catalog.get::<_, i64>(2), 1);

        drop(client);
        server.shutdown(connection).await;
    });
}
