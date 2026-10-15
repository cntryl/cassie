#[test]
fn should_create_a_view_with_local_check_option() {
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
            ],
        )
        .await
        .expect("create and seed the base table for a simple view");

        // Act
        let created = client
            .batch_execute(
                "CREATE VIEW compat_active_rows AS \
                   SELECT id, status FROM compat_check_option_rows \
                   WHERE status = 'active' WITH LOCAL CHECK OPTION",
            )
            .await;
        if let Err(error) = &created {
            let database_error = db_error(error);
            panic!(
                "PostgreSQL 18.6 accepts CREATE VIEW ... WITH LOCAL CHECK OPTION; \
                 Cassie returned DbError code={}, message={:?}, position={:?}",
                database_error.code().code(),
                database_error.message(),
                database_error.position()
            );
        }
        created.expect("create a simple view with a local check option");
        let view = client
            .query_one(
                "SELECT check_option, is_updatable \
                 FROM information_schema.views WHERE table_name = 'compat_active_rows'",
                &[],
            )
            .await
            .expect("inspect the local check option and view updatability");

        // Assert
        assert_eq!(view.get::<_, String>(0), "LOCAL");
        assert_eq!(view.get::<_, String>(1), "YES");

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
fn should_materialize_explicit_with_data_ctas_rows() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("ctas_explicit_with_data").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_ctas_with_data_source \
                   (id INT PRIMARY KEY, label TEXT NOT NULL)",
                "INSERT INTO compat_ctas_with_data_source VALUES (1, 'one'), (2, 'two')",
            ],
        )
        .await
        .expect("create and seed the source for explicit WITH DATA CTAS");

        // Act
        let created = client
            .batch_execute(
                "CREATE TABLE compat_ctas_with_data_result AS \
                 SELECT id, label FROM compat_ctas_with_data_source \
                 ORDER BY id WITH DATA",
            )
            .await;
        if let Err(error) = &created {
            let database_error = db_error(error);
            panic!(
                "PostgreSQL 18.6 accepts CREATE TABLE AS ... WITH DATA; Cassie returned DbError code={}, message={:?}, position={:?}",
                database_error.code().code(),
                database_error.message(),
                database_error.position()
            );
        }
        created.expect("materialize the SELECT rows with explicit WITH DATA");
        let columns = client
            .query(
                "SELECT column_name, data_type FROM information_schema.columns \
                 WHERE table_name = 'compat_ctas_with_data_result' \
                 ORDER BY ordinal_position",
                &[],
            )
            .await
            .expect("inspect explicit WITH DATA CTAS output types");
        let rows = client
            .query(
                "SELECT id, label FROM compat_ctas_with_data_result ORDER BY id",
                &[],
            )
            .await
            .expect("read explicitly materialized CTAS rows");

        // Assert
        assert!(
            columns
                .iter()
                .map(|row| (row.get::<_, String>(0), row.get::<_, String>(1)))
                .collect::<Vec<_>>()
                == vec![
                    ("id".to_owned(), "integer".to_owned()),
                    ("label".to_owned(), "text".to_owned()),
                ],
            "CTAS should preserve the SELECT output names and types"
        );
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].get::<_, i32>(0), 1);
        assert_eq!(rows[0].get::<_, String>(1), "one");
        assert_eq!(rows[1].get::<_, i32>(0), 2);
        assert_eq!(rows[1].get::<_, String>(1), "two");

        drop(client);
        server.shutdown(connection).await;
    });
}
#[test]
fn should_create_materialized_view_with_explicit_with_data() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("materialized_view_explicit_with_data").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_mv_with_data_source \
                   (id INT PRIMARY KEY, label TEXT NOT NULL)",
                "INSERT INTO compat_mv_with_data_source VALUES (1, 'one'), (2, 'two')",
            ],
        )
        .await
        .expect("create and seed the source for explicit WITH DATA materialization");

        // Act
        let created = client
            .batch_execute(
                "CREATE MATERIALIZED VIEW compat_mv_with_data_result AS \
                 SELECT id, label FROM compat_mv_with_data_source \
                 ORDER BY id WITH DATA",
            )
            .await;
        if let Err(error) = &created {
            let database_error = db_error(error);
            panic!(
                "PostgreSQL 18.6 accepts CREATE MATERIALIZED VIEW ... WITH DATA; Cassie returned DbError code={}, message={:?}, position={:?}",
                database_error.code().code(),
                database_error.message(),
                database_error.position()
            );
        }
        created.expect("create and populate the materialized view with explicit WITH DATA");
        let populated: bool = client
            .query_one(
                "SELECT ispopulated FROM pg_catalog.pg_matviews \
                 WHERE schemaname = 'public' \
                   AND matviewname = 'compat_mv_with_data_result'",
                &[],
            )
            .await
            .expect("inspect the materialized-view population state")
            .get(0);
        let rows = client
            .query(
                "SELECT id, label FROM compat_mv_with_data_result ORDER BY id",
                &[],
            )
            .await
            .expect("read the populated materialized-view rows");

        // Assert
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
