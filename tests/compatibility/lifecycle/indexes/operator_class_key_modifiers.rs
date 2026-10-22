use super::*;

#[test]
fn should_preserve_operator_class_with_mixed_btree_key_modifiers() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("index_operator_class_key_modifiers").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_index_operator_class_rows \
                   (id INT, label TEXT, score INT, payload TEXT)",
                "INSERT INTO compat_index_operator_class_rows VALUES \
                   (1, 'alpha', 10, 'a'), (2, 'alpine', 20, 'b'), \
                   (3, 'alpha', 30, 'c'), (4, NULL, 99, 'null'), \
                   (5, 'beta', 40, 'z')",
                "CREATE INDEX compat_index_operator_class_idx \
                   ON compat_index_operator_class_rows USING btree \
                   (label text_pattern_ops ASC NULLS FIRST, \
                    score DESC NULLS LAST) INCLUDE (payload)",
                "CREATE INDEX compat_index_default_modifiers_idx \
                   ON compat_index_operator_class_rows USING btree \
                   (score ASC NULLS LAST)",
            ],
        )
        .await
        .expect("create a btree index with an operator class and mixed key modifiers");

        // Act
        let metadata = client
            .query_one(
                "SELECT index_row.indnkeyatts::INT, index_row.indnatts::INT, \
                        index_row.indoption::text, \
                        pg_catalog.pg_get_indexdef(index_row.indexrelid) \
                 FROM pg_catalog.pg_index AS index_row \
                 WHERE index_row.indexrelid = \
                   'compat_index_operator_class_idx'",
                &[],
            )
            .await
            .expect("read index key count, ordering flags, and canonical definition");
        let default_metadata = client
            .query_one(
                "SELECT index_row.indoption::text, \
                        pg_catalog.pg_get_indexdef(index_row.indexrelid) \
                 FROM pg_catalog.pg_index AS index_row \
                 WHERE index_row.indexrelid = 'compat_index_default_modifiers_idx'",
                &[],
            )
            .await
            .expect("read default-order metadata and canonical definition");
        let ordered_rows = client
            .query(
                "SELECT id, label, score, payload \
                 FROM compat_index_operator_class_rows \
                 ORDER BY label ASC NULLS FIRST, score DESC NULLS LAST",
                &[],
            )
            .await
            .expect("read rows in the indexed key order");

        // Assert
        assert_eq!(
            metadata
                .columns()
                .iter()
                .map(|column| column.type_().oid())
                .collect::<Vec<_>>(),
            vec![23, 23, 25, 25]
        );
        assert_eq!(metadata.get::<_, i32>(0), 2);
        assert_eq!(metadata.get::<_, i32>(1), 3);
        assert_eq!(metadata.get::<_, String>(2), "2 1");
        assert_eq!(
            metadata.get::<_, String>(3),
            "CREATE INDEX compat_index_operator_class_idx \
             ON public.compat_index_operator_class_rows USING btree \
             (label text_pattern_ops NULLS FIRST, score DESC NULLS LAST) \
             INCLUDE (payload)"
        );
        assert_eq!(default_metadata.get::<_, String>(0), "0");
        assert_eq!(
            default_metadata.get::<_, String>(1),
            "CREATE INDEX compat_index_default_modifiers_idx \
             ON public.compat_index_operator_class_rows USING btree (score)"
        );
        assert_eq!(
            ordered_rows
                .iter()
                .map(|row| (
                    row.get::<_, i32>(0),
                    row.get::<_, Option<String>>(1),
                    row.get::<_, i32>(2),
                    row.get::<_, String>(3),
                ))
                .collect::<Vec<_>>(),
            vec![
                (4, None, 99, "null".to_owned()),
                (3, Some("alpha".to_owned()), 30, "c".to_owned()),
                (1, Some("alpha".to_owned()), 10, "a".to_owned()),
                (2, Some("alpine".to_owned()), 20, "b".to_owned()),
                (5, Some("beta".to_owned()), 40, "z".to_owned()),
            ]
        );

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_reject_unselected_btree_operator_classes() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("unsupported_btree_operator_class").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE compat_unsupported_operator_class_rows (key_value INT)")
            .await
            .expect("create a table for unsupported operator-class validation");

        // Act
        let rejected = client
            .batch_execute(
                "CREATE INDEX compat_unsupported_operator_class_idx \
                   ON compat_unsupported_operator_class_rows (key_value int4_ops)",
            )
            .await
            .expect_err("unselected operator classes must be rejected");
        let index_count: i64 = client
            .query_one(
                "SELECT count(*) FROM pg_catalog.pg_indexes \
                 WHERE indexname = 'compat_unsupported_operator_class_idx'",
                &[],
            )
            .await
            .expect("check that the unsupported index was not published")
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
fn should_reject_text_pattern_operator_class_for_nontext_keys() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("text_pattern_operator_class_type").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE compat_text_pattern_type_rows (key_value INT)")
            .await
            .expect("create a numeric table for operator-class type validation");

        // Act
        let rejected = client
            .batch_execute(
                "CREATE INDEX compat_text_pattern_type_idx \
                   ON compat_text_pattern_type_rows (key_value text_pattern_ops)",
            )
            .await
            .expect_err("text_pattern_ops must not be attached to an integer key");
        let index_count: i64 = client
            .query_one(
                "SELECT count(*) FROM pg_catalog.pg_indexes \
                 WHERE indexname = 'compat_text_pattern_type_idx'",
                &[],
            )
            .await
            .expect("check that the invalid index was not published")
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
fn should_reject_key_modifiers_for_interleaved_key_kinds() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("mixed_index_key_modifier_list").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE compat_mixed_index_key_rows (label TEXT, score INT)")
            .await
            .expect("create a table for mixed index-key validation");

        // Act
        let rejected = client
            .batch_execute(
                "CREATE INDEX compat_mixed_index_key_idx \
                   ON compat_mixed_index_key_rows (lower(label), score DESC)",
            )
            .await
            .expect_err("mixed field and expression key order is not represented");
        let index_count: i64 = client
            .query_one(
                "SELECT count(*) FROM pg_catalog.pg_indexes \
                 WHERE indexname = 'compat_mixed_index_key_idx'",
                &[],
            )
            .await
            .expect("check that the unsupported index was not published")
            .try_get(0)
            .expect("index count");

        // Assert
        assert_eq!(db_error(&rejected).code().code(), "0A000");
        assert_eq!(index_count, 0);

        drop(client);
        server.shutdown(connection).await;
    });
}
