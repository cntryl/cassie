use super::*;

#[test]
fn should_publish_unique_index_nulls_not_distinct_metadata() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("unique_index_nulls_not_distinct_catalog").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_index_nulls_not_distinct_rows \
                   (id INT PRIMARY KEY, key_value INT, label TEXT)",
                "INSERT INTO compat_index_nulls_not_distinct_rows \
                   VALUES (1, NULL, 'first')",
                "CREATE UNIQUE INDEX compat_index_nulls_not_distinct_idx \
                   ON compat_index_nulls_not_distinct_rows (key_value) NULLS NOT DISTINCT",
            ],
        )
        .await
        .expect("build the unique index over its first NULL key");

        // Act
        let metadata = client
            .query_one(
                "SELECT index_row.indisunique, index_row.indnullsnotdistinct, \
                        pg_catalog.pg_get_indexdef(index_row.indexrelid) \
                 FROM pg_catalog.pg_index AS index_row \
                 WHERE index_row.indexrelid = \
                   'compat_index_nulls_not_distinct_idx'",
                &[],
            )
            .await
            .expect("read unique-index NULL policy metadata");
        let duplicate_null = client
            .execute(
                "INSERT INTO compat_index_nulls_not_distinct_rows \
                 VALUES (2, NULL, 'duplicate')",
                &[],
            )
            .await
            .expect_err("the index must treat NULL keys as equal");
        let rows = client
            .query(
                "SELECT id, key_value, label \
                 FROM compat_index_nulls_not_distinct_rows ORDER BY id",
                &[],
            )
            .await
            .expect("read rows after duplicate NULL rejection");

        // Assert
        assert_eq!(
            metadata
                .columns()
                .iter()
                .map(|column| column.type_().oid())
                .collect::<Vec<_>>(),
            vec![16, 16, 25]
        );
        assert!(metadata.get::<_, bool>(0));
        assert!(metadata.get::<_, bool>(1));
        assert_eq!(
            metadata.get::<_, String>(2),
            "CREATE UNIQUE INDEX compat_index_nulls_not_distinct_idx \
             ON public.compat_index_nulls_not_distinct_rows USING btree \
             (key_value) NULLS NOT DISTINCT"
        );
        assert_eq!(db_error(&duplicate_null).code().code(), "23505");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].get::<_, i32>(0), 1);
        assert_eq!(rows[0].get::<_, Option<i32>>(1), None);
        assert_eq!(rows[0].get::<_, String>(2), "first");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_reject_null_policy_on_nonunique_index() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("nonunique_index_null_policy").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE compat_nonunique_null_policy (key_value INT)")
            .await
            .expect("create the index target table");

        // Act
        let created = client
            .batch_execute(
                "CREATE INDEX compat_nonunique_null_policy_idx \
                 ON compat_nonunique_null_policy (key_value) NULLS DISTINCT",
            )
            .await
            .expect_err("NULLS DISTINCT is only supported for unique scalar indexes");
        let remaining_indexes = client
            .query_one(
                "SELECT count(*) FROM pg_catalog.pg_index \
                 WHERE indexrelid = 'compat_nonunique_null_policy_idx'",
                &[],
            )
            .await
            .expect("inspect index publication after rejection");

        // Assert
        assert_eq!(db_error(&created).code().code(), "0A000");
        assert_eq!(remaining_indexes.get::<_, i64>(0), 0);
        drop(client);
        server.shutdown(connection).await;
    });
}
