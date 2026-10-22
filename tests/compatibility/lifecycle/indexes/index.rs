use super::*;

#[path = "drop_index_if_exists_scope.rs"]
mod drop_index_if_exists_scope;
#[path = "expression_index_definition.rs"]
mod expression_index_definition;
#[path = "generated_index_name.rs"]
mod generated_index_name;
#[path = "included_partial_indexes.rs"]
mod included_partial_indexes;
#[path = "maintenance.rs"]
mod maintenance_contracts;
#[path = "maintenance_options.rs"]
mod maintenance_options;
#[path = "nulls_not_distinct.rs"]
mod nulls_not_distinct;
#[path = "operator_class_key_modifiers.rs"]
mod operator_class_key_modifiers;
#[path = "partial_index_query_equivalence.rs"]
mod partial_index_query_equivalence;
#[path = "reindex_database_concurrent_ddl.rs"]
mod reindex_database_concurrent_ddl;
#[path = "reindex_options.rs"]
mod reindex_options;
#[path = "reindex_table_schema_concurrent.rs"]
mod reindex_table_schema_concurrent;
#[path = "storage_maintenance.rs"]
mod residual_storage_maintenance;
#[path = "selected_access_methods.rs"]
mod selected_access_methods;
#[path = "storage_parameter_options.rs"]
mod storage_parameter_options;

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
fn should_reject_hash_indexes_until_storage_supports_the_method() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("hash_index_catalog").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE compat_hash_index_rows (id INT)")
            .await
            .expect("create a table for the unsupported hash index");

        // Act
        let rejected = client
            .batch_execute(
                "CREATE INDEX compat_hash_index_idx \
                   ON compat_hash_index_rows USING hash (id)",
            )
            .await
            .expect_err("hash index creation must not claim an unsupported access method");
        let index_count: i64 = client
            .query_one(
                "SELECT count(*) FROM pg_catalog.pg_indexes \
                 WHERE tablename = 'compat_hash_index_rows' \
                   AND indexname = 'compat_hash_index_idx'",
                &[],
            )
            .await
            .expect("check that the rejected index was not published")
            .try_get(0)
            .expect("index count");

        // Assert
        assert_eq!(db_error(&rejected).code().code(), "0A000");
        assert_eq!(index_count, 0);

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
fn should_rebuild_all_indexes_for_a_table() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("reindex_table").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_reindex_table_rows (id INT, value INT)",
                "INSERT INTO compat_reindex_table_rows VALUES (1, 20), (2, 10)",
                "CREATE INDEX compat_reindex_table_value_idx \
                   ON compat_reindex_table_rows (value)",
            ],
        )
        .await
        .expect("create a table with an index and rows");

        // Act
        client
            .batch_execute("REINDEX TABLE compat_reindex_table_rows")
            .await
            .expect("rebuild every index belonging to the table");
        let ordered = client
            .query(
                "SELECT value FROM compat_reindex_table_rows ORDER BY value",
                &[],
            )
            .await
            .expect("read table rows after table-level reindex");

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

#[test]
fn should_drop_only_the_schema_qualified_index_without_an_on_clause() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("schema_qualified_index_drop").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE SCHEMA compat_index_drop_left",
                "CREATE SCHEMA compat_index_drop_right",
                "CREATE TABLE compat_index_drop_left.rows (id INT)",
                "CREATE TABLE compat_index_drop_right.rows (id INT)",
                "CREATE INDEX shared_idx ON compat_index_drop_left.rows (id)",
                "CREATE INDEX shared_idx ON compat_index_drop_right.rows (id)",
            ],
        )
        .await
        .expect("create same-named indexes in separate schemas");

        // Act
        client
            .batch_execute("DROP INDEX compat_index_drop_left.shared_idx")
            .await
            .expect("drop the index resolved in the selected schema");
        let remaining = client
            .query(
                "SELECT schemaname, indexname, tablename \
                 FROM pg_catalog.pg_indexes \
                 WHERE indexname = 'shared_idx' \
                   AND schemaname IN ('compat_index_drop_left', 'compat_index_drop_right') \
                 ORDER BY schemaname",
                &[],
            )
            .await
            .expect("read the remaining same-named index");

        // Assert
        let remaining = remaining
            .iter()
            .map(|row| {
                (
                    row.get::<_, String>(0),
                    row.get::<_, String>(1),
                    row.get::<_, String>(2),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            remaining,
            vec![(
                "compat_index_drop_right".to_string(),
                "shared_idx".to_string(),
                "rows".to_string(),
            )]
        );

        drop(client);
        server.shutdown(connection).await;
    });
}
