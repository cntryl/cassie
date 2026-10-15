use super::*;

#[path = "selected_kind_keys.rs"]
mod selected_kind_keys;

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

#[test]
fn should_restore_analyzed_expression_statistics_after_restart() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        support::use_local_storage();
        let path = support::data_dir("expression-statistics-restart");
        let _directory = DataDirectory(path.clone());
        let expected_catalog = vec![vec![
            cassie::types::Value::String("compat_lower_category_stats".into()),
            cassie::types::Value::String("compat_expression_stats".into()),
            cassie::types::Value::String("{e}".into()),
            cassie::types::Value::Int64(250),
            cassie::types::Value::Bool(true),
        ]];

        {
            let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie instance");
            cassie.startup().expect("start Cassie instance");
            let session = cassie.create_session("tester", None);
            for sql in [
                "CREATE TABLE compat_expression_stats (category TEXT)",
                "INSERT INTO compat_expression_stats VALUES ('A'), ('a'), ('b')",
                "CREATE STATISTICS compat_lower_category_stats \
                   ON (lower(category)) FROM compat_expression_stats",
                "ALTER STATISTICS compat_lower_category_stats SET STATISTICS 250",
                "ANALYZE compat_expression_stats",
            ] {
                cassie
                    .execute_sql(&session, sql, vec![])
                    .unwrap_or_else(|error| panic!("could not execute {sql}: {error}"));
            }
            drop(session);
            drop(cassie);
        }

        // Act
        let cassie = Cassie::new_with_data_dir(&path).expect("reopen Cassie instance");
        cassie.startup().expect("recover expression statistics");
        let session = cassie.create_session("tester", None);
        let catalog = cassie
            .execute_sql(
                &session,
                "SELECT s.stxname, relation.relname, s.stxkind::TEXT, \
                        s.stxstattarget, s.stxexprs IS NOT NULL \
                 FROM pg_catalog.pg_statistic_ext AS s \
                 JOIN pg_catalog.pg_class AS relation ON relation.oid = s.stxrelid \
                 WHERE s.stxname = 'compat_lower_category_stats'",
                vec![],
            )
            .expect("read persisted expression-statistics definition")
            .rows()
            .to_vec();
        let expression_data = cassie
            .execute_sql(
                &session,
                "SELECT data.stxdexpr IS NOT NULL \
                 FROM pg_catalog.pg_statistic_ext_data AS data \
                 JOIN pg_catalog.pg_statistic_ext AS statistics \
                   ON statistics.oid = data.stxoid \
                 WHERE statistics.stxname = 'compat_lower_category_stats'",
                vec![],
            )
            .expect("read persisted expression-statistics data")
            .rows()
            .to_vec();
        let analyzed_rows = cassie
            .execute_sql(
                &session,
                "SELECT count(*) FROM compat_expression_stats",
                vec![],
            )
            .expect("read the persisted analyzed relation")
            .rows()
            .to_vec();

        // Assert
        assert_eq!(catalog, expected_catalog);
        assert_eq!(
            expression_data,
            vec![vec![cassie::types::Value::Bool(true)]]
        );
        assert_eq!(analyzed_rows, vec![vec![cassie::types::Value::Int64(3)]]);
    });
}

#[test]
fn should_use_extended_statistics_to_improve_correlated_filter_estimates() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("extended_statistics_planner_estimate").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_correlated_stats (left_value TEXT, right_value TEXT)",
                "INSERT INTO compat_correlated_stats \
                   SELECT (value % 10)::TEXT, (value % 10)::TEXT \
                   FROM generate_series(1, 1000) AS generated(value)",
                "ANALYZE compat_correlated_stats",
            ],
        )
        .await
        .expect("create and analyze the correlated-column fixture");
        let query = "SELECT * FROM compat_correlated_stats \
                     WHERE left_value = '1' AND right_value = '1'";
        let before = client
            .query_one(&format!("EXPLAIN {query}"), &[])
            .await
            .expect("explain the correlated filter before extended statistics");
        let before_plan: String = before.try_get(0).expect("textual query plan");
        let before_estimate = scan_estimate(&before_plan);
        let actual_rows: i64 = client
            .query_one(
                "SELECT count(*) FROM compat_correlated_stats \
                 WHERE left_value = '1' AND right_value = '1'",
                &[],
            )
            .await
            .expect("count the rows selected by the correlated filter")
            .try_get(0)
            .expect("actual row count");
        client
            .batch_execute(
                "CREATE STATISTICS compat_correlated_stats_all \
                   (dependencies, ndistinct, mcv) \
                   ON left_value, right_value FROM compat_correlated_stats; \
                 ANALYZE compat_correlated_stats",
            )
            .await
            .expect("collect all three extended-statistics kinds");

        // Act
        let after = client
            .query_one(&format!("EXPLAIN {query}"), &[])
            .await
            .expect("explain the correlated filter with extended statistics");
        let after_plan: String = after.try_get(0).expect("textual query plan");
        let after_estimate = scan_estimate(&after_plan);
        let statistics = client
            .query_one(
                "SELECT data.stxddependencies IS NOT NULL, \
                        data.stxdndistinct IS NOT NULL, data.stxdmcv IS NOT NULL \
                 FROM pg_catalog.pg_statistic_ext AS statistics \
                 JOIN pg_catalog.pg_statistic_ext_data AS data \
                   ON data.stxoid = statistics.oid \
                 WHERE statistics.stxname = 'compat_correlated_stats_all'",
                &[],
            )
            .await
            .expect("verify all collected statistics are present");
        let actual_rows = u64::try_from(actual_rows).expect("row count is non-negative");

        // Assert
        // PostgreSQL 18.6 estimates 10 rows before and 100 after statistics for
        // this fixture; compare accuracy rather than requiring an identical plan.
        assert_eq!(actual_rows, 100);
        assert!(before_estimate > 0, "plan={before_plan}");
        assert!(after_estimate > 0, "plan={after_plan}");
        assert!(
            u64::abs_diff(after_estimate, actual_rows as u64)
                < u64::abs_diff(before_estimate, actual_rows as u64),
            "extended statistics should improve the estimate from {before_estimate} toward {actual_rows}, got {after_estimate}; before={before_plan}; after={after_plan}"
        );
        assert!(statistics.get::<_, bool>(0));
        assert!(statistics.get::<_, bool>(1));
        assert!(statistics.get::<_, bool>(2));

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_refresh_or_invalidate_extended_statistics_after_schema_changes() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("extended_statistics_schema_invalidation").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_stats_schema_change (left_value TEXT, right_value TEXT)",
                "INSERT INTO compat_stats_schema_change VALUES \
                   ('a', 'a'), ('a', 'a'), ('b', 'b'), ('b', 'b')",
                "CREATE STATISTICS compat_stats_schema_change_all \
                   (dependencies, ndistinct, mcv) \
                   ON left_value, right_value FROM compat_stats_schema_change",
                "ANALYZE compat_stats_schema_change",
            ],
        )
        .await
        .expect("create a relation and collect its extended statistics");

        // Act
        client
            .batch_execute(
                "ALTER TABLE compat_stats_schema_change \
                   RENAME COLUMN left_value TO renamed_value",
            )
            .await
            .expect("rename a column used by the statistics object");
        let after_rename = client
            .query_one(
                "SELECT pg_catalog.pg_get_statisticsobjdef(statistics.oid), \
                        data.stxddependencies IS NOT NULL \
                 FROM pg_catalog.pg_statistic_ext AS statistics \
                 JOIN pg_catalog.pg_statistic_ext_data AS data \
                   ON data.stxoid = statistics.oid \
                 WHERE statistics.stxname = 'compat_stats_schema_change_all'",
                &[],
            )
            .await
            .expect("read the renamed statistics definition and data");
        let renamed_definition: String = after_rename.try_get(0).expect("statistics definition");
        client
            .batch_execute(
                "ALTER TABLE compat_stats_schema_change \
                   ALTER COLUMN renamed_value TYPE VARCHAR(32)",
            )
            .await
            .expect("change the type used by the statistics object");
        let invalidated = client
            .query_one(
                "SELECT \
                   count(statistics.oid), \
                   (SELECT count(*) FROM pg_catalog.pg_statistic_ext_data AS data \
                    JOIN pg_catalog.pg_statistic_ext AS statistics \
                      ON statistics.oid = data.stxoid \
                    WHERE statistics.stxname = 'compat_stats_schema_change_all'), \
                   pg_catalog.pg_get_statisticsobjdef(statistics.oid) \
                 FROM pg_catalog.pg_statistic_ext AS statistics \
                 WHERE statistics.stxname = 'compat_stats_schema_change_all' \
                 GROUP BY statistics.oid",
                &[],
            )
            .await
            .expect("verify type change cleared stale data but retained the definition");
        client
            .batch_execute("ANALYZE compat_stats_schema_change")
            .await
            .expect("rebuild extended statistics after the schema change");
        let recollected = client
            .query_one(
                "SELECT data.stxddependencies IS NOT NULL, \
                        data.stxdndistinct IS NOT NULL, data.stxdmcv IS NOT NULL \
                 FROM pg_catalog.pg_statistic_ext AS statistics \
                 JOIN pg_catalog.pg_statistic_ext_data AS data \
                   ON data.stxoid = statistics.oid \
                 WHERE statistics.stxname = 'compat_stats_schema_change_all'",
                &[],
            )
            .await
            .expect("read the re-collected statistics for the new column type");

        // Assert
        assert!(
            renamed_definition.contains("renamed_value"),
            "{renamed_definition}"
        );
        assert!(after_rename.get::<_, bool>(1));
        assert_eq!(invalidated.get::<_, i64>(0), 1);
        assert_eq!(invalidated.get::<_, i64>(1), 0);
        assert!(invalidated.get::<_, String>(2).contains("renamed_value"));
        assert!(recollected.get::<_, bool>(0));
        assert!(recollected.get::<_, bool>(1));
        assert!(recollected.get::<_, bool>(2));

        drop(client);
        server.shutdown(connection).await;
    });
}

fn scan_estimate(plan: &str) -> u64 {
    plan.split_once("estimates=scan:")
        .and_then(|(_, estimates)| estimates.split_whitespace().next())
        .and_then(|estimate| estimate.parse().ok())
        .unwrap_or_else(|| panic!("query plan did not expose a scan estimate: {plan}"))
}
