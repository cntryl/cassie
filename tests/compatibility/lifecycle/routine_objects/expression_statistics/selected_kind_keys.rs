use super::*;

#[test]
fn should_publish_selected_statistics_catalog_contract() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("extended_statistics_selected_kinds").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_914_selected_stats \
                   (left_value INT, right_value INT, unused_value BOOLEAN)",
                "INSERT INTO compat_914_selected_stats \
                 SELECT value % 4, value % 4, true \
                 FROM generate_series(1, 200) AS generated(value)",
                "CREATE STATISTICS public.compat_914_selected_stats_deps_mcv \
                   (dependencies, mcv) ON left_value, right_value \
                   FROM public.compat_914_selected_stats",
                "ANALYZE public.compat_914_selected_stats",
            ],
        )
        .await
        .expect("collect the requested kinds for only the first two columns");

        // Act
        let statistics = client
            .query_one(
                "SELECT statistics.stxkind::TEXT, statistics.stxkeys::TEXT, \
                        data.stxddependencies IS NOT NULL, \
                        data.stxdndistinct IS NOT NULL, data.stxdmcv IS NOT NULL, \
                        pg_catalog.pg_get_statisticsobjdef(statistics.oid) \
                 FROM pg_catalog.pg_statistic_ext AS statistics \
                 JOIN pg_catalog.pg_statistic_ext_data AS data \
                   ON data.stxoid = statistics.oid \
                 WHERE statistics.stxname = 'compat_914_selected_stats_deps_mcv'",
                &[],
            )
            .await
            .expect("read selected kinds, key attributes, and collected data");

        // Assert
        assert_eq!(statistics.get::<_, String>(0), "{f,m}");
        assert_eq!(statistics.get::<_, String>(1), "1 2");
        assert!(statistics.get::<_, bool>(2));
        assert!(!statistics.get::<_, bool>(3));
        assert!(statistics.get::<_, bool>(4));
        assert_eq!(
            statistics.get::<_, String>(5),
            "CREATE STATISTICS public.compat_914_selected_stats_deps_mcv \
             (dependencies, mcv) ON left_value, right_value \
             FROM compat_914_selected_stats"
        );

        drop(client);
        server.shutdown(connection).await;
    });
}
