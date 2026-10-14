use super::*;

#[test]
fn should_report_descending_btree_key_order() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("descending_index_catalog").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_descending_index_rows (id INT, score INT)",
                "INSERT INTO compat_descending_index_rows VALUES (1, 10), (2, 20)",
                "CREATE INDEX compat_descending_index_idx \
                   ON compat_descending_index_rows USING btree (score DESC)",
            ],
        )
        .await
        .expect("create a descending btree key");

        // Act
        let index_definition: String = client
            .query_one(
                "SELECT indexdef FROM pg_catalog.pg_indexes \
                 WHERE tablename = 'compat_descending_index_rows' \
                   AND indexname = 'compat_descending_index_idx'",
                &[],
            )
            .await
            .expect("read the PostgreSQL index definition")
            .try_get(0)
            .expect("index definition");

        // Assert
        assert!(
            index_definition.contains("USING btree (score DESC)"),
            "PostgreSQL reports the method and key ordering; got {index_definition:?}"
        );

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_publish_hash_index_method_in_catalog_definition() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("hash_index_catalog").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_hash_index_rows (id INT)",
                "INSERT INTO compat_hash_index_rows VALUES (1), (2)",
                "CREATE INDEX compat_hash_index_idx \
                   ON compat_hash_index_rows USING hash (id)",
            ],
        )
        .await
        .expect("create an index using the PostgreSQL hash method");

        // Act
        let definition: String = client
            .query_one(
                "SELECT indexdef FROM pg_catalog.pg_indexes \
                 WHERE tablename = 'compat_hash_index_rows' \
                   AND indexname = 'compat_hash_index_idx'",
                &[],
            )
            .await
            .expect("read the hash-index definition")
            .try_get(0)
            .expect("index definition");

        // Assert
        assert!(definition.contains("USING hash (id)"), "{definition:?}");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_rebuild_an_existing_index_with_reindex() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("reindex_existing_index").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_reindex_rows (id INT, score INT)",
                "INSERT INTO compat_reindex_rows VALUES (1, 20), (2, 10)",
                "CREATE INDEX compat_reindex_score_idx \
                   ON compat_reindex_rows (score)",
            ],
        )
        .await
        .expect("create a table and its index");

        // Act
        client
            .batch_execute("REINDEX INDEX compat_reindex_score_idx")
            .await
            .expect("rebuild the existing index");
        let ordered = client
            .query("SELECT score FROM compat_reindex_rows ORDER BY score", &[])
            .await
            .expect("read table rows after rebuilding the index");

        // Assert
        assert_eq!(
            ordered
                .iter()
                .map(|row| row.get::<_, i32>(0))
                .collect::<Vec<_>>(),
            vec![10, 20]
        );

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_roundtrip_index_comments_through_catalog() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("index_comments").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_index_comment_rows (id INT)",
                "CREATE INDEX compat_index_comment_idx \
                   ON compat_index_comment_rows (id)",
            ],
        )
        .await
        .expect("create an index to annotate");

        // Act
        client
            .batch_execute("COMMENT ON INDEX compat_index_comment_idx IS 'index note'")
            .await
            .expect("store an object comment on the index");
        let comment: Option<String> = client
            .query_one(
                "SELECT obj_description('compat_index_comment_idx'::regclass, 'pg_class')",
                &[],
            )
            .await
            .expect("read the index comment from PostgreSQL object metadata")
            .try_get(0)
            .expect("index comment");

        // Assert
        assert_eq!(comment.as_deref(), Some("index note"));

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_drop_index_concurrently_outside_a_transaction_block() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("drop_index_concurrently").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_concurrent_drop_rows (id INT)",
                "CREATE INDEX compat_concurrent_drop_idx \
                   ON compat_concurrent_drop_rows (id)",
            ],
        )
        .await
        .expect("create a relation and an index to remove");

        // Act
        client
            .batch_execute("DROP INDEX CONCURRENTLY compat_concurrent_drop_idx")
            .await
            .expect("drop the index concurrently in autocommit mode");
        let remaining: i64 = client
            .query_one(
                "SELECT count(*) FROM pg_catalog.pg_indexes \
                 WHERE schemaname = 'public' \
                   AND indexname = 'compat_concurrent_drop_idx'",
                &[],
            )
            .await
            .expect("inspect the index catalog after concurrent removal")
            .try_get(0)
            .expect("matching index count");

        // Assert
        assert_eq!(remaining, 0);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_publish_table_statistics_after_analyze() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("analyze_table_statistics").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_analyze_rows (label TEXT)",
                "INSERT INTO compat_analyze_rows VALUES \
                   ('common'), ('common'), ('common'), ('common'), ('common'), \
                   ('common'), ('common'), ('common'), ('common'), ('common'), ('rare')",
            ],
        )
        .await
        .expect("create a skewed table for table statistics");

        // Act
        client
            .batch_execute("ANALYZE compat_analyze_rows")
            .await
            .expect("collect table statistics");
        let most_common: Option<String> = client
            .query_one(
                "SELECT most_common_vals::text FROM pg_catalog.pg_stats \
                 WHERE schemaname = 'public' \
                   AND tablename = 'compat_analyze_rows' AND attname = 'label'",
                &[],
            )
            .await
            .expect("read the table's most common value statistics")
            .try_get(0)
            .expect("most common values");

        // Assert
        assert_eq!(most_common.as_deref(), Some("{common}"));

        drop(client);
        server.shutdown(connection).await;
    });
}
