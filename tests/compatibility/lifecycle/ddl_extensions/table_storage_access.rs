use super::*;

#[test]
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
