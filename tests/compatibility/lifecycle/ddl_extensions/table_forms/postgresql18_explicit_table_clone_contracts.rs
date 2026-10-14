use super::*;

#[test]
fn should_copy_column_comments_only_when_like_comments_are_included() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("like_comments_option_forms").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_like_comments_source (payload TEXT)",
                "COMMENT ON COLUMN compat_like_comments_source.payload \
                 IS 'source comment'",
                "CREATE TABLE compat_like_comments_included \
                 (LIKE compat_like_comments_source INCLUDING COMMENTS)",
                "CREATE TABLE compat_like_comments_excluded \
                 (LIKE compat_like_comments_source EXCLUDING COMMENTS)",
            ],
        )
        .await
        .expect("create LIKE tables with explicit COMMENTS inclusion and exclusion");

        // Act
        let comments = client
            .query(
                "SELECT c.relname, col_description(c.oid, a.attnum) \
                 FROM pg_catalog.pg_class AS c \
                 JOIN pg_catalog.pg_attribute AS a ON a.attrelid = c.oid \
                 WHERE c.relname IN (\
                   'compat_like_comments_included', 'compat_like_comments_excluded') \
                   AND a.attname = 'payload' \
                 ORDER BY c.relname",
                &[],
            )
            .await
            .expect("inspect copied and excluded column comments");

        // Assert
        assert_eq!(comments.len(), 2);
        assert_eq!(
            comments[0].get::<_, String>(0),
            "compat_like_comments_excluded"
        );
        assert_eq!(comments[0].get::<_, Option<String>>(1), None);
        assert_eq!(
            comments[1].get::<_, String>(0),
            "compat_like_comments_included"
        );
        assert_eq!(
            comments[1].get::<_, Option<String>>(1).as_deref(),
            Some("source comment")
        );

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_copy_column_storage_only_when_like_storage_is_included() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("like_storage_option_forms").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_like_storage_source (payload TEXT)",
                "ALTER TABLE compat_like_storage_source \
                 ALTER COLUMN payload SET STORAGE EXTERNAL",
                "CREATE TABLE compat_like_storage_included \
                 (LIKE compat_like_storage_source INCLUDING STORAGE)",
                "CREATE TABLE compat_like_storage_excluded \
                 (LIKE compat_like_storage_source EXCLUDING STORAGE)",
            ],
        )
        .await
        .expect("create LIKE tables with explicit STORAGE inclusion and exclusion");

        // Act
        let storage = client
            .query(
                "SELECT c.relname, a.attstorage::TEXT \
                 FROM pg_catalog.pg_class AS c \
                 JOIN pg_catalog.pg_attribute AS a ON a.attrelid = c.oid \
                 WHERE c.relname IN (\
                   'compat_like_storage_included', 'compat_like_storage_excluded') \
                   AND a.attname = 'payload' \
                 ORDER BY c.relname",
                &[],
            )
            .await
            .expect("inspect included and excluded storage policies");

        // Assert
        assert_eq!(storage.len(), 2);
        assert_eq!(
            storage[0].get::<_, String>(0),
            "compat_like_storage_excluded"
        );
        assert_eq!(storage[0].get::<_, String>(1), "x");
        assert_eq!(
            storage[1].get::<_, String>(0),
            "compat_like_storage_included"
        );
        assert_eq!(storage[1].get::<_, String>(1), "e");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_copy_column_compression_only_when_like_compression_is_included() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("like_compression_option_forms").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_like_compression_source (payload TEXT)",
                "ALTER TABLE compat_like_compression_source \
                 ALTER COLUMN payload SET COMPRESSION pglz",
                "CREATE TABLE compat_like_compression_included \
                 (LIKE compat_like_compression_source INCLUDING COMPRESSION)",
                "CREATE TABLE compat_like_compression_excluded \
                 (LIKE compat_like_compression_source EXCLUDING COMPRESSION)",
            ],
        )
        .await
        .expect("create LIKE tables with explicit COMPRESSION inclusion and exclusion");

        // Act
        let compression = client
            .query(
                "SELECT c.relname, a.attcompression::TEXT \
                 FROM pg_catalog.pg_class AS c \
                 JOIN pg_catalog.pg_attribute AS a ON a.attrelid = c.oid \
                 WHERE c.relname IN (\
                   'compat_like_compression_included', \
                   'compat_like_compression_excluded') \
                   AND a.attname = 'payload' \
                 ORDER BY c.relname",
                &[],
            )
            .await
            .expect("inspect included and excluded compression policies");

        // Assert
        assert_eq!(compression.len(), 2);
        assert_eq!(
            compression[0].get::<_, String>(0),
            "compat_like_compression_excluded"
        );
        assert_eq!(compression[0].get::<_, String>(1), "");
        assert_eq!(
            compression[1].get::<_, String>(0),
            "compat_like_compression_included"
        );
        assert_eq!(compression[1].get::<_, String>(1), "p");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_apply_typed_defaults_using_declared_column_types() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("typed_table_defaults").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute(
                "CREATE TABLE compat_typed_defaults (\
                   amount NUMERIC(8, 3) DEFAULT (12.375::NUMERIC(8, 3)), \
                   small SMALLINT DEFAULT (7::SMALLINT), \
                   payload JSONB DEFAULT ('{\"kind\":\"fresh\"}'::JSONB))",
            )
            .await
            .expect("create columns with explicitly typed defaults");

        // Act
        let inserted = client
            .query_one(
                "INSERT INTO compat_typed_defaults DEFAULT VALUES \
                 RETURNING amount::TEXT, small, payload::TEXT, \
                           pg_typeof(amount)::TEXT, pg_typeof(small)::TEXT, \
                           pg_typeof(payload)::TEXT",
                &[],
            )
            .await
            .expect("apply each typed default through DEFAULT VALUES");
        let metadata = client
            .query(
                "SELECT attname, atttypid::INT, atttypmod, attnotnull \
                 FROM pg_catalog.pg_attribute \
                 WHERE attrelid = 'compat_typed_defaults'::regclass \
                   AND attnum > 0 ORDER BY attnum",
                &[],
            )
            .await
            .expect("inspect declared column OIDs and typmods");

        // Assert
        assert_eq!(inserted.get::<_, String>(0), "12.375");
        assert_eq!(inserted.get::<_, i16>(1), 7);
        assert_eq!(inserted.get::<_, String>(2), "{\"kind\": \"fresh\"}");
        assert_eq!(inserted.get::<_, String>(3), "numeric");
        assert_eq!(inserted.get::<_, String>(4), "smallint");
        assert_eq!(inserted.get::<_, String>(5), "jsonb");
        assert_eq!(metadata.len(), 3);
        assert_eq!(metadata[0].get::<_, String>(0), "amount");
        assert_eq!(metadata[0].get::<_, i32>(1), 1700);
        assert_eq!(metadata[0].get::<_, i32>(2), 524_295);
        assert!(!metadata[0].get::<_, bool>(3));
        assert_eq!(metadata[1].get::<_, String>(0), "small");
        assert_eq!(metadata[1].get::<_, i32>(1), 21);
        assert_eq!(metadata[1].get::<_, i32>(2), -1);
        assert!(!metadata[1].get::<_, bool>(3));
        assert_eq!(metadata[2].get::<_, String>(0), "payload");
        assert_eq!(metadata[2].get::<_, i32>(1), 3802);
        assert_eq!(metadata[2].get::<_, i32>(2), -1);
        assert!(!metadata[2].get::<_, bool>(3));

        drop(client);
        server.shutdown(connection).await;
    });
}
