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

#[test]
fn should_reject_reserved_index_metadata_options() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("reserved_index_metadata_options").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_reserved_index_metadata_rows (payload TEXT)",
                "CREATE INDEX compat_reserved_index_metadata_idx \
                   ON compat_reserved_index_metadata_rows (payload)",
            ],
        )
        .await
        .expect("create the table and btree index");

        // Act
        let alter_result = client
            .batch_execute(
                "ALTER INDEX compat_reserved_index_metadata_idx \
                 SET (__cassie_index_key_0_order = desc)",
            )
            .await;
        let create_result = client
            .batch_execute(
                "CREATE INDEX compat_reserved_index_metadata_forged_idx \
                 ON compat_reserved_index_metadata_rows (payload) \
                 WITH (__cassie_storage_id = 7)",
            )
            .await;
        let index_state = client
            .query_one(
                "SELECT pg_catalog.to_regclass(\
                            'compat_reserved_index_metadata_forged_idx') IS NULL, \
                        pg_catalog.pg_get_indexdef(oid) \
                 FROM pg_catalog.pg_class \
                 WHERE oid = 'compat_reserved_index_metadata_idx'::regclass",
                &[],
            )
            .await
            .expect("read index state after rejected metadata options");

        // Assert
        assert!(alter_result.is_err());
        assert!(create_result.is_err());
        assert!(index_state.get::<_, bool>(0));
        assert_eq!(
            index_state.get::<_, String>(1),
            "CREATE INDEX compat_reserved_index_metadata_idx \
             ON public.compat_reserved_index_metadata_rows USING btree (payload)"
        );

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_accept_btree_fillfactor_bounds() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("btree_fillfactor_bounds").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_btree_fillfactor_bounds (payload TEXT)",
                "CREATE INDEX compat_btree_fillfactor_min_idx \
                   ON compat_btree_fillfactor_bounds (payload) WITH (fillfactor = 10)",
                "CREATE INDEX compat_btree_fillfactor_max_idx \
                   ON compat_btree_fillfactor_bounds (payload) WITH (fillfactor = 100)",
            ],
        )
        .await
        .expect("create btree indexes at the supported fillfactor bounds");

        // Act
        let minimum: Option<String> = client
            .query_one(
                "SELECT reloptions::text FROM pg_catalog.pg_class \
                 WHERE oid = 'compat_btree_fillfactor_min_idx'::regclass",
                &[],
            )
            .await
            .expect("read minimum fillfactor")
            .try_get(0)
            .expect("minimum reloptions");
        let maximum: Option<String> = client
            .query_one(
                "SELECT reloptions::text FROM pg_catalog.pg_class \
                 WHERE oid = 'compat_btree_fillfactor_max_idx'::regclass",
                &[],
            )
            .await
            .expect("read maximum fillfactor")
            .try_get(0)
            .expect("maximum reloptions");

        // Assert
        assert_eq!(minimum.as_deref(), Some("{fillfactor=10}"));
        assert_eq!(maximum.as_deref(), Some("{fillfactor=100}"));

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_reject_invalid_btree_storage_parameters() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("invalid_btree_storage_parameters").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_invalid_btree_storage_rows (payload TEXT)",
                "CREATE INDEX compat_invalid_btree_storage_idx \
                   ON compat_invalid_btree_storage_rows (payload) \
                   WITH (fillfactor = 80, deduplicate_items = off)",
            ],
        )
        .await
        .expect("create a btree index with supported options");

        // Act
        let invalid_create = client
            .batch_execute(
                "CREATE INDEX compat_invalid_btree_fillfactor_idx \
                 ON compat_invalid_btree_storage_rows (payload) WITH (fillfactor = 101)",
            )
            .await;
        let below_range = client
            .batch_execute("ALTER INDEX compat_invalid_btree_storage_idx SET (fillfactor = 9)")
            .await;
        let above_range = client
            .batch_execute("ALTER INDEX compat_invalid_btree_storage_idx SET (fillfactor = 101)")
            .await;
        let invalid_boolean = client
            .batch_execute(
                "ALTER INDEX compat_invalid_btree_storage_idx \
                 SET (deduplicate_items = maybe)",
            )
            .await;
        let unsupported_parameter = client
            .batch_execute("ALTER INDEX compat_invalid_btree_storage_idx SET (fastupdate = off)")
            .await;
        let mixed_parameters = client
            .batch_execute(
                "ALTER INDEX compat_invalid_btree_storage_idx \
                 SET (fillfactor = 70, fastupdate = off)",
            )
            .await;
        let index_state = client
            .query_one(
                "SELECT pg_catalog.to_regclass('compat_invalid_btree_fillfactor_idx') IS NULL, \
                        reloptions::text \
                 FROM pg_catalog.pg_class \
                 WHERE oid = 'compat_invalid_btree_storage_idx'::regclass",
                &[],
            )
            .await
            .expect("read index state after rejected storage parameters");

        // Assert
        assert!(invalid_create.is_err());
        assert!(below_range.is_err());
        assert!(above_range.is_err());
        assert!(invalid_boolean.is_err());
        assert!(unsupported_parameter.is_err());
        assert!(mixed_parameters.is_err());
        assert!(index_state.get::<_, bool>(0));
        assert_eq!(
            index_state.get::<_, String>(1),
            "{deduplicate_items=off,fillfactor=80}"
        );

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_apply_multiple_btree_storage_parameters() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("multiple_btree_storage_parameters").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_multiple_btree_storage_rows (payload TEXT)",
                "CREATE INDEX compat_multiple_btree_storage_idx \
                   ON compat_multiple_btree_storage_rows (payload)",
            ],
        )
        .await
        .expect("create a btree index with default storage parameters");

        // Act
        client
            .batch_execute(
                "ALTER INDEX compat_multiple_btree_storage_idx \
                 SET (fillfactor = 73, deduplicate_items = off)",
            )
            .await
            .expect("set both supported btree storage parameters");
        let set_options: Option<String> = client
            .query_one(
                "SELECT reloptions::text FROM pg_catalog.pg_class \
                 WHERE oid = 'compat_multiple_btree_storage_idx'::regclass",
                &[],
            )
            .await
            .expect("read both configured storage parameters")
            .try_get(0)
            .expect("configured reloptions");
        let definition: String = client
            .query_one(
                "SELECT pg_catalog.pg_get_indexdef(\
                            'compat_multiple_btree_storage_idx'::regclass)",
                &[],
            )
            .await
            .expect("read index definition with both storage parameters")
            .try_get(0)
            .expect("index definition");
        client
            .batch_execute(
                "ALTER INDEX compat_multiple_btree_storage_idx \
                 RESET (deduplicate_items, fillfactor)",
            )
            .await
            .expect("reset both supported btree storage parameters");
        let reset_is_null: bool = client
            .query_one(
                "SELECT reloptions IS NULL FROM pg_catalog.pg_class \
                 WHERE oid = 'compat_multiple_btree_storage_idx'::regclass",
                &[],
            )
            .await
            .expect("read reset storage parameter state")
            .try_get(0)
            .expect("reset reloptions state");

        // Assert
        assert_eq!(
            set_options.as_deref(),
            Some("{deduplicate_items=off,fillfactor=73}")
        );
        assert_eq!(
            definition,
            "CREATE INDEX compat_multiple_btree_storage_idx \
             ON public.compat_multiple_btree_storage_rows USING btree \
             (payload) WITH (deduplicate_items='off', fillfactor='73')"
        );
        assert!(reset_is_null);

        drop(client);
        server.shutdown(connection).await;
    });
}
