use super::*;

#[test]
fn should_update_index_storage_parameter_catalog_state() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("index_storage_parameter_options").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_index_storage_parameter_rows \
                   (id INT PRIMARY KEY, payload TEXT)",
                "CREATE INDEX compat_index_storage_parameter_idx \
                   ON compat_index_storage_parameter_rows (payload)",
            ],
        )
        .await
        .expect("create a btree index with a nondefault storage parameter");

        // Act
        let initial_is_null: bool = client
            .query_one(
                "SELECT reloptions IS NULL \
                 FROM pg_catalog.pg_class \
                 WHERE oid = 'compat_index_storage_parameter_idx'::regclass",
                &[],
            )
            .await
            .expect("read the default index storage-option state")
            .try_get(0)
            .expect("default reloptions state");
        client
            .batch_execute(
                "ALTER INDEX compat_index_storage_parameter_idx \
                 SET (fillfactor = 81)",
            )
            .await
            .expect("change the index fillfactor through ALTER INDEX");
        let altered = client
            .query_one(
                "SELECT index_class.reloptions::text, \
                        pg_catalog.pg_get_indexdef(index_class.oid) \
                 FROM pg_catalog.pg_class AS index_class \
                 WHERE index_class.oid = \
                   'compat_index_storage_parameter_idx'::regclass",
                &[],
            )
            .await
            .expect("read the altered index storage option and definition");
        client
            .batch_execute(
                "ALTER INDEX compat_index_storage_parameter_idx \
                 RESET (fillfactor)",
            )
            .await
            .expect("reset the index fillfactor to its default");
        let reset_is_null: bool = client
            .query_one(
                "SELECT reloptions IS NULL \
                 FROM pg_catalog.pg_class \
                 WHERE oid = 'compat_index_storage_parameter_idx'::regclass",
                &[],
            )
            .await
            .expect("read whether the reset cleared custom storage options")
            .try_get(0)
            .expect("reset reloptions state");

        // Assert
        assert!(initial_is_null);
        assert_eq!(
            altered
                .columns()
                .iter()
                .map(|column| column.type_().oid())
                .collect::<Vec<_>>(),
            vec![25, 25]
        );
        assert_eq!(
            altered.get::<_, Option<String>>(0).as_deref(),
            Some("{fillfactor=81}")
        );
        assert_eq!(
            altered.get::<_, String>(1),
            "CREATE INDEX compat_index_storage_parameter_idx \
             ON public.compat_index_storage_parameter_rows USING btree \
             (payload) WITH (fillfactor='81')"
        );
        assert!(reset_is_null);

        drop(client);
        server.shutdown(connection).await;
    });
}
