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
                 SELECT id, label, CAST(1.5 AS DOUBLE PRECISION) AS measure \
                 FROM compat_ctas_with_data_source \
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
        assert_eq!(
            columns
                .iter()
                .map(|row| (row.get::<_, String>(0), row.get::<_, String>(1)))
                .collect::<Vec<_>>(),
            vec![
                ("id".to_owned(), "integer".to_owned()),
                ("label".to_owned(), "text".to_owned()),
                ("measure".to_owned(), "double precision".to_owned()),
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
