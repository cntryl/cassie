use super::*;

#[test]
#[ignore = "Pending #826 column storage and compression metadata contract"]
fn should_preserve_column_storage_compression_metadata() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("column_storage_compression_clauses").await;
        let (client, connection) = server.connect().await;

        // Act
        let created = client
            .batch_execute(
                "CREATE TABLE compat_column_storage_options (\
                   plain_text TEXT STORAGE PLAIN,\
                   external_pglz TEXT STORAGE EXTERNAL COMPRESSION pglz,\
                   extended_lz4 TEXT STORAGE EXTENDED COMPRESSION lz4,\
                   default_compression TEXT COMPRESSION default\
                 )",
            )
            .await;
        let (inserted, metadata, rows) = if created.is_ok() {
            let inserted = client
                .execute(
                    "INSERT INTO compat_column_storage_options VALUES \
                     ('plain', 'pglz', 'lz4', 'default')",
                    &[],
                )
                .await;
            let metadata = if inserted.is_ok() {
                Some(
                    client
                        .query(
                            "SELECT attname, attstorage::TEXT, attcompression::TEXT \
                             FROM pg_catalog.pg_attribute \
                             WHERE attrelid = 'compat_column_storage_options'::regclass \
                               AND attnum > 0 ORDER BY attnum",
                            &[],
                        )
                        .await,
                )
            } else {
                None
            };
            let rows = if inserted.is_ok() {
                Some(
                    client
                        .query("SELECT * FROM compat_column_storage_options", &[])
                        .await,
                )
            } else {
                None
            };
            (Some(inserted), metadata, rows)
        } else {
            (None, None, None)
        };

        // Assert
        let create_sqlstate = created
            .as_ref()
            .err()
            .map(|error| db_error(error).code().code());
        drop(client);
        server.shutdown(connection).await;
        assert!(
            created.is_ok(),
            "CREATE TABLE column STORAGE and COMPRESSION clauses should be accepted; \
             SQLSTATE: {create_sqlstate:?}"
        );
        assert_eq!(
            inserted
                .expect("insert should be attempted after table creation")
                .expect("insert storage-option values"),
            1
        );
        let metadata = metadata
            .expect("catalog inspection should follow a successful insert")
            .expect("read column storage and compression metadata");
        assert_eq!(metadata.len(), 4);
        assert_eq!(metadata[0].get::<_, String>(0), "plain_text");
        assert_eq!(metadata[0].get::<_, String>(1), "p");
        assert_eq!(metadata[0].get::<_, String>(2), "");
        assert_eq!(metadata[1].get::<_, String>(0), "external_pglz");
        assert_eq!(metadata[1].get::<_, String>(1), "e");
        assert_eq!(metadata[1].get::<_, String>(2), "p");
        assert_eq!(metadata[2].get::<_, String>(0), "extended_lz4");
        assert_eq!(metadata[2].get::<_, String>(1), "x");
        assert_eq!(metadata[2].get::<_, String>(2), "l");
        assert_eq!(metadata[3].get::<_, String>(0), "default_compression");
        assert_eq!(metadata[3].get::<_, String>(1), "x");
        assert_eq!(metadata[3].get::<_, String>(2), "");
        let rows = rows
            .expect("row inspection should follow a successful insert")
            .expect("read values stored with column options");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].get::<_, String>(0), "plain");
        assert_eq!(rows[0].get::<_, String>(1), "pglz");
        assert_eq!(rows[0].get::<_, String>(2), "lz4");
        assert_eq!(rows[0].get::<_, String>(3), "default");
    });
}

#[test]
fn should_reject_column_storage_options_without_publishing_the_table() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("column_storage_rejection").await;
        let (client, connection) = server.connect().await;

        // Act
        let (sqlstate, message, relation_count) = rejected_column_option(
            &client,
            "compat_column_storage_rejected",
            "value TEXT STORAGE PLAIN",
        )
        .await;

        // Assert
        assert_eq!(sqlstate, "0A000");
        assert!(message.contains("column STORAGE options are not supported"));
        assert_eq!(relation_count, 0);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_reject_column_compression_options_without_publishing_the_table() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("column_compression_rejection").await;
        let (client, connection) = server.connect().await;

        // Act
        let (sqlstate, message, relation_count) = rejected_column_option(
            &client,
            "compat_column_compression_rejected",
            "value TEXT COMPRESSION pglz",
        )
        .await;

        // Assert
        assert_eq!(sqlstate, "0A000");
        assert!(message.contains("column COMPRESSION options are not supported"));
        assert_eq!(relation_count, 0);

        drop(client);
        server.shutdown(connection).await;
    });
}

async fn rejected_column_option(
    client: &tokio_postgres::Client,
    table: &str,
    definition: &str,
) -> (String, String, i64) {
    let sql = format!("CREATE TABLE {table} ({definition})");
    let error = client
        .batch_execute(&sql)
        .await
        .expect_err("unsupported column options should be rejected");
    let sqlstate = db_error(&error).code().code().to_string();
    let message = db_error(&error).message().to_string();
    let relation_count = client
        .query_one(
            "SELECT count(*) FROM pg_catalog.pg_class WHERE relname = $1::TEXT",
            &[&table],
        )
        .await
        .expect("check rejected DDL did not publish a relation")
        .get(0);
    (sqlstate, message, relation_count)
}

#[test]
fn should_create_table_with_an_explicit_access_method() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("create_table_access_method").await;
        let (client, connection) = server.connect().await;

        // Act
        let created = client
            .batch_execute(
                "CREATE TABLE compat_access_method_table (id INT, label TEXT) USING heap",
            )
            .await;
        let inserted = if created.is_ok() {
            let inserted = client
                .execute(
                    "INSERT INTO compat_access_method_table VALUES (1, 'one')",
                    &[],
                )
                .await;
            Some(inserted)
        } else {
            None
        };

        // Assert
        let create_sqlstate = created
            .as_ref()
            .err()
            .map(|error| db_error(error).code().code());
        drop(client);
        server.shutdown(connection).await;
        assert!(
            created.is_ok(),
            "CREATE TABLE USING heap should create a heap relation; \
             SQLSTATE: {create_sqlstate:?}"
        );
        assert_eq!(
            inserted
                .expect("insert should be attempted after table creation")
                .expect("insert into the explicit heap table"),
            1
        );
    });
}

#[test]
fn should_reject_unsupported_table_access_methods_without_publishing_the_table() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("unsupported_table_access_method").await;
        let (client, connection) = server.connect().await;

        // Act
        let result = client
            .batch_execute("CREATE TABLE compat_unsupported_table_access (id INT) USING columnar")
            .await;
        let error = result.expect_err("unsupported table access methods should be rejected");
        let relation_count: i64 = client
            .query_one(
                "SELECT count(*) FROM pg_catalog.pg_class \
                 WHERE relname = 'compat_unsupported_table_access'",
                &[],
            )
            .await
            .expect("check rejected DDL did not publish a relation")
            .get(0);

        // Assert
        assert_eq!(db_error(&error).code().code(), "0A000");
        assert_eq!(relation_count, 0);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_change_table_access_method_without_changing_rows() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("alter_table_access_method").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_alter_table_access (id INT, label TEXT)",
                "INSERT INTO compat_alter_table_access VALUES (1, 'one')",
            ],
        )
        .await
        .expect("create and seed a table before changing its access method");

        // Act
        let altered = client
            .batch_execute("ALTER TABLE compat_alter_table_access SET ACCESS METHOD heap")
            .await;
        let access_method = if altered.is_ok() {
            Some(
                client
                    .query_one(
                        "SELECT relation.relam::INT, access_method.amname \
                         FROM pg_catalog.pg_class AS relation \
                         JOIN pg_catalog.pg_am AS access_method \
                           ON access_method.oid = relation.relam \
                         WHERE relation.oid = 'compat_alter_table_access'::regclass",
                        &[],
                    )
                    .await,
            )
        } else {
            None
        };
        let rows = if altered.is_ok() {
            Some(
                client
                    .query(
                        "SELECT id, label FROM compat_alter_table_access ORDER BY id",
                        &[],
                    )
                    .await,
            )
        } else {
            None
        };

        // Assert
        let sqlstate = altered
            .as_ref()
            .err()
            .map(|error| db_error(error).code().code());
        drop(client);
        server.shutdown(connection).await;
        assert!(
            altered.is_ok(),
            "PostgreSQL 18.6 accepts ALTER TABLE SET ACCESS METHOD; SQLSTATE: {sqlstate:?}"
        );
        let access_method = access_method
            .expect("inspect the table after access-method change")
            .expect("read the table access method");
        assert_eq!(access_method.get::<_, i32>(0), 2);
        assert_eq!(access_method.get::<_, String>(1), "heap");
        let rows = rows
            .expect("read table rows after access-method change")
            .expect("query rows after access-method change");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].get::<_, i32>(0), 1);
        assert_eq!(rows[0].get::<_, String>(1), "one");
    });
}

#[test]
fn should_create_ctas_with_an_explicit_access_method() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("ctas_access_method").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_access_method_ctas_source (id INT, label TEXT)",
                "INSERT INTO compat_access_method_ctas_source VALUES (1, 'one'), (2, 'two')",
            ],
        )
        .await
        .expect("create and seed the CTAS source");

        // Act
        let created = client
            .batch_execute(
                "CREATE TABLE compat_access_method_ctas_copy USING heap AS \
                 SELECT id, label FROM compat_access_method_ctas_source ORDER BY id",
            )
            .await;
        let rows = if created.is_ok() {
            let rows = client
                .query(
                    "SELECT id, label FROM compat_access_method_ctas_copy ORDER BY id",
                    &[],
                )
                .await;
            Some(rows)
        } else {
            None
        };

        // Assert
        let create_sqlstate = created
            .as_ref()
            .err()
            .map(|error| db_error(error).code().code());
        drop(client);
        server.shutdown(connection).await;
        assert!(
            created.is_ok(),
            "CTAS USING heap should create a heap relation; SQLSTATE: {create_sqlstate:?}"
        );
        let rows = rows
            .expect("read CTAS rows after successful creation")
            .expect("query rows copied by CTAS");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].get::<_, i32>(0), 1);
        assert_eq!(rows[0].get::<_, String>(1), "one");
        assert_eq!(rows[1].get::<_, i32>(0), 2);
        assert_eq!(rows[1].get::<_, String>(1), "two");
    });
}

#[test]
fn should_reject_materialized_view_access_method_until_storage_support_is_implemented() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("materialized_view_access_method").await;
        let (client, connection) = server.connect().await;

        // Act
        let result = client
            .batch_execute(
                "CREATE MATERIALIZED VIEW compat_access_method_mv USING heap AS SELECT 17 AS value",
            )
            .await;
        let error = result.expect_err("materialized-view access methods remain pending under #826");
        let view_count: i64 = client
            .query_one(
                "SELECT count(*) FROM pg_matviews WHERE matviewname = 'compat_access_method_mv'",
                &[],
            )
            .await
            .expect("check that the rejected materialized view was not published")
            .get(0);

        // Assert
        assert_eq!(db_error(&error).code().code(), "0A000");
        assert_eq!(view_count, 0);

        drop(client);
        server.shutdown(connection).await;
    });
}
#[test]
fn should_reject_materialized_view_access_method_changes_without_changing_rows() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("alter_materialized_view_access_method").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_alter_access_method_source (id INT, label TEXT)",
                "INSERT INTO compat_alter_access_method_source VALUES (1, 'one'), (2, 'two')",
                "CREATE MATERIALIZED VIEW compat_alter_access_method_view AS SELECT id, label \
                 FROM compat_alter_access_method_source ORDER BY id",
            ],
        )
        .await
        .expect("create the source and supported materialized view");
        let rows_before = client
            .query(
                "SELECT id, label FROM compat_alter_access_method_view ORDER BY id",
                &[],
            )
            .await
            .expect("read rows before the unsupported storage change");

        // Act
        let result = client
            .batch_execute(
                "ALTER MATERIALIZED VIEW compat_alter_access_method_view SET ACCESS METHOD heap",
            )
            .await;
        let error = result.expect_err("materialized-view access-method changes remain pending");
        let rows_after = client
            .query(
                "SELECT id, label FROM compat_alter_access_method_view ORDER BY id",
                &[],
            )
            .await
            .expect("read rows after the rejected storage change");

        // Assert
        assert_eq!(db_error(&error).code().code(), "0A000");
        assert_eq!(rows_before.len(), 2);
        assert_eq!(rows_after.len(), 2);
        assert_eq!(rows_after[0].get::<_, i32>(0), 1);
        assert_eq!(rows_after[0].get::<_, String>(1), "one");
        assert_eq!(rows_after[1].get::<_, i32>(0), 2);
        assert_eq!(rows_after[1].get::<_, String>(1), "two");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
#[ignore = "Pending #826 materialized-view storage contract"]
fn should_create_materialized_view_with_an_explicit_access_method() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("materialized_view_access_method").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_access_method_mv_source (id INT, label TEXT)",
                "INSERT INTO compat_access_method_mv_source VALUES (1, 'one'), (2, 'two')",
            ],
        )
        .await
        .expect("create and seed the materialized-view source");

        // Act
        let created = client
            .batch_execute(
                "CREATE MATERIALIZED VIEW compat_access_method_mv USING heap AS \
                 SELECT id, label FROM compat_access_method_mv_source ORDER BY id",
            )
            .await;
        let rows = if created.is_ok() {
            let rows = client
                .query(
                    "SELECT id, label FROM compat_access_method_mv ORDER BY id",
                    &[],
                )
                .await;
            Some(rows)
        } else {
            None
        };

        // Assert
        let create_sqlstate = created
            .as_ref()
            .err()
            .map(|error| db_error(error).code().code());
        drop(client);
        server.shutdown(connection).await;
        assert!(
            created.is_ok(),
            "CREATE MATERIALIZED VIEW USING heap should create a heap relation; \
             SQLSTATE: {create_sqlstate:?}"
        );
        let rows = rows
            .expect("read materialized-view rows after successful creation")
            .expect("query rows materialized from the source");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].get::<_, i32>(0), 1);
        assert_eq!(rows[0].get::<_, String>(1), "one");
        assert_eq!(rows[1].get::<_, i32>(0), 2);
        assert_eq!(rows[1].get::<_, String>(1), "two");
    });
}

#[test]
#[ignore = "Pending #826 materialized-view storage contract"]
fn should_set_materialized_view_access_method_without_changing_rows() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("alter_materialized_view_access_method").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_alter_access_method_source (id INT, label TEXT)",
                "INSERT INTO compat_alter_access_method_source VALUES \
                   (1, 'one'), (2, 'two')",
                "CREATE MATERIALIZED VIEW compat_alter_access_method_view USING heap AS \
                   SELECT id, label FROM compat_alter_access_method_source ORDER BY id",
            ],
        )
        .await
        .expect("create the source and heap-backed materialized view");

        // Act
        let altered = client
            .batch_execute(
                "ALTER MATERIALIZED VIEW compat_alter_access_method_view \
                 SET ACCESS METHOD heap",
            )
            .await;
        let access_method = if altered.is_ok() {
            Some(
                client
                    .query_one(
                        "SELECT relation.relam::INT, access_method.amname \
                         FROM pg_catalog.pg_class AS relation \
                         JOIN pg_catalog.pg_am AS access_method \
                           ON access_method.oid = relation.relam \
                         WHERE relation.oid = \
                           'compat_alter_access_method_view'::regclass",
                        &[],
                    )
                    .await,
            )
        } else {
            None
        };
        let rows = if altered.is_ok() {
            Some(
                client
                    .query(
                        "SELECT id, label FROM compat_alter_access_method_view ORDER BY id",
                        &[],
                    )
                    .await,
            )
        } else {
            None
        };

        // Assert
        let sqlstate = altered
            .as_ref()
            .err()
            .map(|error| db_error(error).code().code());
        drop(client);
        server.shutdown(connection).await;
        assert!(
            altered.is_ok(),
            "PostgreSQL 18.6 accepts ALTER MATERIALIZED VIEW SET ACCESS METHOD; \
             SQLSTATE: {sqlstate:?}"
        );
        let access_method = access_method
            .expect("inspect the materialized view after access-method change")
            .expect("read the materialized-view access method");
        assert_eq!(access_method.get::<_, i32>(0), 2);
        assert_eq!(access_method.get::<_, String>(1), "heap");
        let rows = rows
            .expect("read materialized-view rows after access-method change")
            .expect("query rows after access-method change");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].get::<_, i32>(0), 1);
        assert_eq!(rows[0].get::<_, String>(1), "one");
        assert_eq!(rows[1].get::<_, i32>(0), 2);
        assert_eq!(rows[1].get::<_, String>(1), "two");
    });
}
