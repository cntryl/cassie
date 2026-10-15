use super::*;

#[test]
fn should_collect_statistics_during_option_bearing_full_vacuum() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("vacuum_full_analyze_options").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_vacuum_full_options_rows (id INT, label TEXT)",
                "INSERT INTO compat_vacuum_full_options_rows VALUES \
                   (1, 'common'), (2, 'common'), (3, 'common'), \
                   (4, 'common'), (5, 'common'), (6, 'common'), \
                   (7, 'common'), (8, 'common'), (9, 'common'), \
                   (10, 'rare'), (11, 'rare')",
                "DELETE FROM compat_vacuum_full_options_rows WHERE id = 11",
            ],
        )
        .await
        .expect("create a table with one deleted row and skewed text values");

        // Act
        client
            .batch_execute(
                "VACUUM (FULL, ANALYZE, INDEX_CLEANUP AUTO, VERBOSE FALSE, \
                 BUFFER_USAGE_LIMIT '1MB') compat_vacuum_full_options_rows",
            )
            .await
            .expect("rewrite the table and collect statistics with PostgreSQL vacuum options");
        let table_state = client
            .query_one(
                "SELECT last_analyze IS NOT NULL, n_live_tup, n_dead_tup \
                 FROM pg_catalog.pg_stat_user_tables \
                 WHERE schemaname = 'public' \
                   AND relname = 'compat_vacuum_full_options_rows'",
                &[],
            )
            .await
            .expect("read table statistics after the full vacuum");
        let most_common: Option<String> = client
            .query_one(
                "SELECT most_common_vals::text \
                 FROM pg_catalog.pg_stats \
                 WHERE schemaname = 'public' \
                   AND tablename = 'compat_vacuum_full_options_rows' \
                   AND attname = 'label'",
                &[],
            )
            .await
            .expect("read analyzed values after the full vacuum")
            .try_get(0)
            .expect("most-common values");
        let visible_rows: i64 = client
            .query_one("SELECT count(*) FROM compat_vacuum_full_options_rows", &[])
            .await
            .expect("count visible rows after the full vacuum")
            .try_get(0)
            .expect("visible row count");

        // Assert
        assert!(table_state.get::<_, bool>(0));
        assert_eq!(table_state.get::<_, i64>(1), 10);
        assert_eq!(table_state.get::<_, i64>(2), 0);
        assert_eq!(most_common.as_deref(), Some("{common}"));
        assert_eq!(visible_rows, 10);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_refresh_statistics_with_parenthesized_analyze_options() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("analyze_parenthesized_options").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_analyze_options_rows (id INT, label TEXT)",
                "INSERT INTO compat_analyze_options_rows VALUES \
                   (1, 'common'), (2, 'common'), (3, 'common'), \
                   (4, 'common'), (5, 'common'), (6, 'common'), \
                   (7, 'common'), (8, 'common'), (9, 'common'), \
                   (10, 'rare'), (11, 'rare')",
            ],
        )
        .await
        .expect("create rows with a distinguishing text distribution");

        // Act
        client
            .batch_execute(
                "ANALYZE (VERBOSE FALSE, SKIP_LOCKED FALSE, \
                 BUFFER_USAGE_LIMIT '1MB') compat_analyze_options_rows",
            )
            .await
            .expect("analyze the table with parenthesized PostgreSQL options");
        let statistics = client
            .query_one(
                "SELECT last_analyze IS NOT NULL, \
                        pg_catalog.pg_stats.most_common_vals::text \
                 FROM pg_catalog.pg_stat_user_tables \
                 JOIN pg_catalog.pg_stats \
                   ON pg_catalog.pg_stats.schemaname = \
                        pg_catalog.pg_stat_user_tables.schemaname \
                  AND pg_catalog.pg_stats.tablename = \
                        pg_catalog.pg_stat_user_tables.relname \
                  AND pg_catalog.pg_stats.attname = 'label' \
                 WHERE pg_catalog.pg_stat_user_tables.schemaname = 'public' \
                   AND pg_catalog.pg_stat_user_tables.relname = \
                     'compat_analyze_options_rows'",
                &[],
            )
            .await
            .expect("read the analyze timestamp and column distribution");

        // Assert
        assert!(statistics.get::<_, bool>(0));
        assert_eq!(
            statistics.get::<_, Option<String>>(1).as_deref(),
            Some("{common}")
        );

        drop(client);
        server.shutdown(connection).await;
    });
}
