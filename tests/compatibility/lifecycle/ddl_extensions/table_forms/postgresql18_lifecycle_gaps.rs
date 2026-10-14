use super::*;

#[test]
fn should_change_table_persistence_modes_without_changing_rows() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("table_persistence_mode_alteration").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE compat_persistence_modes (id INT PRIMARY KEY, label TEXT)")
            .await
            .expect("create a logged table");
        client
            .execute(
                "INSERT INTO compat_persistence_modes VALUES (1, 'retained')",
                &[],
            )
            .await
            .expect("seed the table before changing its persistence mode");

        // Act
        client
            .batch_execute("ALTER TABLE compat_persistence_modes SET UNLOGGED")
            .await
            .expect("switch the table to unlogged persistence");
        let unlogged_persistence: String = client
            .query_one(
                "SELECT relpersistence::TEXT FROM pg_catalog.pg_class \
                 WHERE relname = 'compat_persistence_modes'",
                &[],
            )
            .await
            .expect("read persistence after SET UNLOGGED")
            .get(0);
        client
            .batch_execute("ALTER TABLE compat_persistence_modes SET LOGGED")
            .await
            .expect("switch the table back to logged persistence");
        let logged_persistence: String = client
            .query_one(
                "SELECT relpersistence::TEXT FROM pg_catalog.pg_class \
                 WHERE relname = 'compat_persistence_modes'",
                &[],
            )
            .await
            .expect("read persistence after SET LOGGED")
            .get(0);
        let row = client
            .query_one("SELECT id, label FROM compat_persistence_modes", &[])
            .await
            .expect("read the row after both persistence transitions");

        // Assert
        assert_eq!(unlogged_persistence, "u");
        assert_eq!(logged_persistence, "p");
        assert_eq!(row.get::<_, i32>(0), 1);
        assert_eq!(row.get::<_, String>(1), "retained");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_reject_view_updates_that_violate_a_local_check_option() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("view_local_check_option").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_check_option_rows \
                   (id INT PRIMARY KEY, status TEXT NOT NULL)",
                "INSERT INTO compat_check_option_rows VALUES (1, 'active')",
                "CREATE VIEW compat_active_rows AS \
                   SELECT id, status FROM compat_check_option_rows \
                   WHERE status = 'active' WITH LOCAL CHECK OPTION",
            ],
        )
        .await
        .expect("create a simple auto-updatable view with a local check option");

        // Act
        let rejected = client
            .execute(
                "UPDATE compat_active_rows SET status = 'inactive' WHERE id = 1",
                &[],
            )
            .await
            .expect_err("reject an update that would make its row disappear from the view");
        let sqlstate = db_error(&rejected).code().code().to_owned();
        let row = client
            .query_one(
                "SELECT status FROM compat_check_option_rows WHERE id = 1",
                &[],
            )
            .await
            .expect("read the base row after the rejected view update");

        // Assert
        assert_eq!(sqlstate, "44000");
        assert_eq!(row.get::<_, String>(0), "active");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_replace_a_view_definition_without_changing_its_identity() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("replace_view_definition").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_replace_view_source (id INT PRIMARY KEY, label TEXT)",
                "INSERT INTO compat_replace_view_source VALUES (1, 'original')",
                "CREATE VIEW compat_replace_view AS \
                   SELECT id, label FROM compat_replace_view_source",
                "CREATE VIEW compat_replace_view_dependent AS \
                   SELECT id, label FROM compat_replace_view",
            ],
        )
        .await
        .expect("create a view and a dependent view");
        let original_oid: i64 = client
            .query_one(
                "SELECT oid::BIGINT FROM pg_catalog.pg_class \
                 WHERE relname = 'compat_replace_view'",
                &[],
            )
            .await
            .expect("read the original view identity")
            .get(0);

        // Act
        client
            .batch_execute(
                "CREATE OR REPLACE VIEW compat_replace_view AS \
                 SELECT id, label || '-replacement' AS label \
                 FROM compat_replace_view_source",
            )
            .await
            .expect("replace the view query while preserving its declared columns");
        let replaced_oid: i64 = client
            .query_one(
                "SELECT oid::BIGINT FROM pg_catalog.pg_class \
                 WHERE relname = 'compat_replace_view'",
                &[],
            )
            .await
            .expect("read the view identity after replacement")
            .get(0);
        let dependent_row = client
            .query_one("SELECT id, label FROM compat_replace_view_dependent", &[])
            .await
            .expect("read the dependent view after replacing its dependency");

        // Assert
        assert_eq!(replaced_oid, original_oid);
        assert_eq!(dependent_row.get::<_, i32>(0), 1);
        assert_eq!(dependent_row.get::<_, String>(1), "original-replacement");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_leave_no_ctas_relation_after_query_execution_fails() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("ctas_query_failure_atomicity").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_ctas_failure_source \
                   (id INT PRIMARY KEY, numerator INT NOT NULL, denominator INT NOT NULL)",
                "INSERT INTO compat_ctas_failure_source VALUES \
                   (1, 8, 2), (2, 9, 0)",
            ],
        )
        .await
        .expect("create and seed the source before the failing CTAS statement");

        // Act
        let create_failed = client
            .batch_execute(
                "CREATE TABLE compat_ctas_failure_result AS \
                 SELECT id, numerator / denominator AS quotient \
                 FROM compat_ctas_failure_source ORDER BY id",
            )
            .await
            .expect_err("the source row with a zero denominator must fail CTAS evaluation");
        let target_absent: bool = client
            .query_one(
                "SELECT to_regclass('public.compat_ctas_failure_result') IS NULL",
                &[],
            )
            .await
            .expect("check that failed CTAS did not publish its relation")
            .get(0);
        let source_rows: i64 = client
            .query_one("SELECT count(*) FROM compat_ctas_failure_source", &[])
            .await
            .expect("confirm that source rows remain after failed CTAS")
            .get(0);

        // Assert
        assert_eq!(db_error(&create_failed).code().code(), "22012");
        assert!(
            target_absent,
            "failed CTAS must not leave a catalog relation"
        );
        assert_eq!(source_rows, 2, "failed CTAS must preserve its input rows");

        drop(client);
        server.shutdown(connection).await;
    });
}
