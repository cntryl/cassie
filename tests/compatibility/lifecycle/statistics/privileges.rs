use super::*;

#[test]
fn should_reject_statistics_data_reads_given_an_unprivileged_role_when_extended_statistics_exist() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("statistics_data_permissions").await;
        let (client, connection) = server.connect().await;
        for statement in [
            "CREATE ROLE compat_stats_unprivileged",
            "CREATE TABLE compat_stats_acl_rows (left_value INT, right_value INT)",
            "INSERT INTO compat_stats_acl_rows \
             SELECT value, value % 3 FROM generate_series(1, 30) AS value",
            "CREATE STATISTICS compat_stats_acl (ndistinct) \
             ON left_value, right_value FROM compat_stats_acl_rows",
            "ANALYZE compat_stats_acl_rows",
        ] {
            client
                .batch_execute(statement)
                .await
                .expect("create and populate an extended-statistics object");
        }
        client
            .batch_execute("SET ROLE compat_stats_unprivileged")
            .await
            .expect("switch to a role without statistics-data privileges");

        // Act
        let denied = client
            .query("SELECT * FROM pg_catalog.pg_statistic_ext_data", &[])
            .await;
        client
            .batch_execute("RESET ROLE")
            .await
            .expect("restore the administrative role after the denied read");

        // Assert
        assert_eq!(
            db_error(
                denied
                    .as_ref()
                    .expect_err("statistics data is restricted to its owner")
            )
            .code()
            .code(),
            "42501"
        );
        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_reject_statistics_mutation_given_an_unprivileged_role_when_extended_statistics_exist() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("statistics_data_mutation_permissions").await;
        let (client, connection) = server.connect().await;
        for statement in [
            "CREATE ROLE compat_stats_mutation_unprivileged",
            "CREATE TABLE compat_stats_mutation_rows (left_value INT, right_value INT)",
            "INSERT INTO compat_stats_mutation_rows \
             SELECT value, value % 3 FROM generate_series(1, 30) AS value",
            "CREATE STATISTICS compat_stats_mutation (ndistinct) \
             ON left_value, right_value FROM compat_stats_mutation_rows",
            "ANALYZE compat_stats_mutation_rows",
        ] {
            client
                .batch_execute(statement)
                .await
                .expect("create populated extended statistics");
        }
        let visible_statistics: i64 = client
            .query_one(
                "SELECT count(*)::BIGINT FROM pg_catalog.pg_statistic_ext_data",
                &[],
            )
            .await
            .expect("confirm that ANALYZE published extended-statistics data")
            .get(0);
        client
            .batch_execute("SET ROLE compat_stats_mutation_unprivileged")
            .await
            .expect("switch to the role without extended-statistics privileges");

        // Act
        let mutation = client
            .execute(
                "UPDATE pg_catalog.pg_statistic_ext_data \
                 SET stxdinherit = false",
                &[],
            )
            .await;
        client
            .batch_execute("RESET ROLE")
            .await
            .expect("restore the administrative role after the denied mutation");

        // Assert
        assert_eq!(visible_statistics, 1);
        assert_eq!(
            db_error(
                mutation
                    .as_ref()
                    .expect_err("an unprivileged role cannot mutate statistics data")
            )
            .code()
            .code(),
            "42501"
        );
        drop(client);
        server.shutdown(connection).await;
    });
}
