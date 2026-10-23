#[test]
fn should_preserve_covering_scalar_index_order_after_restart() {
    // Arrange
    use_local_storage();
    let path = data_dir("covering_scalar_restart_order");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);
            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE sql_covering_scalar_restart_order (email TEXT, title TEXT)",
                    vec![],
                )
                .unwrap();
            let collection = cassie
                .catalog
                .get_schema("sql_covering_scalar_restart_order")
                .expect("catalog collection")
                .collection;
            cassie
                .midge
                .put_document(
                    &collection,
                    Some("doc-2".to_string()),
                    serde_json::json!({"email": "a@example.com", "title": "alpha"}),
                )
                .unwrap();
            cassie
                .midge
                .put_document(
                    &collection,
                    Some("doc-1".to_string()),
                    serde_json::json!({"email": "a@example.com"}),
                )
                .unwrap();
            cassie
                .midge
                .put_document(
                    &collection,
                    Some("doc-3".to_string()),
                    serde_json::json!({"email": "b@example.com", "title": "beta"}),
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "CREATE INDEX sql_covering_scalar_restart_order_email_idx ON sql_covering_scalar_restart_order USING btree (email)",
                    vec![],
                )
                .unwrap();
        }

        let restarted = Cassie::new_with_data_dir(&path).unwrap();
        restarted.startup().unwrap();
        let session = restarted.create_session("tester", None);

        // Act
        let result = restarted
            .execute_sql(
                &session,
                "SELECT id, email FROM sql_covering_scalar_restart_order WHERE email = 'a@example.com' ORDER BY id DESC",
                vec![],
            )
            .unwrap();
        let explain = restarted
            .execute_sql(
                &session,
                "EXPLAIN SELECT id, email FROM sql_covering_scalar_restart_order WHERE email = 'a@example.com' ORDER BY id DESC",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            result.rows(),
            vec![
                vec![
                    Value::String("doc-2".to_string()),
                    Value::String("a@example.com".to_string())
                ],
                vec![
                    Value::String("doc-1".to_string()),
                    Value::String("a@example.com".to_string())
                ],
            ]
        );
        let Value::String(plan) = &explain.rows()[0][0] else {
            panic!("expected textual plan");
        };
        assert!(plan.contains("covered_index=true"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_persist_include_index_metadata_after_restart() {
    // Arrange
    use_local_storage();
    let path = data_dir("include_metadata_restart");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);
            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE sql_include_metadata_restart (email TEXT, title TEXT, body TEXT)",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "CREATE INDEX sql_include_metadata_restart_email_idx ON sql_include_metadata_restart USING btree (email text_pattern_ops ASC NULLS FIRST) INCLUDE (title, body)",
                    vec![],
                )
                .unwrap();
        }

        // Act
        let restarted = Cassie::new_with_data_dir(&path).unwrap();
        restarted.startup().unwrap();
        let index = restarted
            .catalog
            .get_index(
                "sql_include_metadata_restart",
                "sql_include_metadata_restart_email_idx",
            )
            .expect("index should hydrate");
        let session = restarted.create_session("tester", None);
        let introspection = restarted
            .execute_sql(
                &session,
                "SELECT indexdef FROM pg_catalog.pg_indexes WHERE tablename = 'sql_include_metadata_restart'",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            index.include_fields,
            vec!["title".to_string(), "body".to_string()]
        );
        assert_eq!(
            index
                .options
                .get("__cassie_index_key_0_operator_class")
                .map(String::as_str),
            Some("text_pattern_ops")
        );
        assert_eq!(
            index
                .options
                .get("__cassie_index_key_0_order")
                .map(String::as_str),
            Some("asc")
        );
        assert_eq!(
            index
                .options
                .get("__cassie_index_key_0_nulls")
                .map(String::as_str),
            Some("first")
        );
        let Value::String(indexdef) = &introspection.rows()[0][0] else {
            panic!("expected textual index definition");
        };
        assert!(indexdef.contains("INCLUDE (title, body)"));
        assert!(indexdef.contains("email text_pattern_ops NULLS FIRST"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_persist_altered_btree_storage_parameters_after_restart() {
    // Arrange
    use_local_storage();
    let path = data_dir("altered_btree_storage_parameters_restart");
    {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE sql_btree_storage_parameters_restart (payload TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX sql_btree_storage_parameters_restart_idx \
                 ON sql_btree_storage_parameters_restart USING btree (payload) \
                 WITH (deduplicate_items = off)",
                vec![],
            )
            .unwrap();

        // Act
        cassie
            .execute_sql(
                &session,
                "ALTER INDEX sql_btree_storage_parameters_restart_idx \
                 SET (fillfactor = 81)",
                vec![],
            )
            .unwrap();
    }

    let restarted = Cassie::new_with_data_dir(&path).unwrap();
    restarted.startup().unwrap();
    let index = restarted
        .catalog
        .get_index(
            "sql_btree_storage_parameters_restart",
            "sql_btree_storage_parameters_restart_idx",
        )
        .expect("index metadata should hydrate after restart");
    let session = restarted.create_session("tester", None);
    let introspection = restarted
        .execute_sql(
            &session,
            "SELECT reloptions, pg_catalog.pg_get_indexdef(oid) \
             FROM pg_catalog.pg_class \
             WHERE oid = 'sql_btree_storage_parameters_restart_idx'::regclass",
            vec![],
        )
        .unwrap();

    // Assert
    assert_eq!(
        index.options.get("deduplicate_items").map(String::as_str),
        Some("off")
    );
    assert_eq!(
        index.options.get("fillfactor").map(String::as_str),
        Some("81")
    );
    assert_eq!(
        introspection.rows()[0][0],
        Value::String("{deduplicate_items=off,fillfactor=81}".to_string())
    );
    assert_eq!(
        introspection.rows()[0][1],
        Value::String(
            "CREATE INDEX sql_btree_storage_parameters_restart_idx \
             ON public.sql_btree_storage_parameters_restart USING btree \
             (payload) WITH (deduplicate_items='off', fillfactor='81')"
                .to_string()
        )
    );

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_hydrate_partial_index_metadata_after_restart() {
    // Arrange
    use_local_storage();
    let path = data_dir("partial_index_restart");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);
            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE sql_partial_index_restart (title TEXT, status TEXT)",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "CREATE INDEX sql_partial_index_restart_title_idx ON sql_partial_index_restart USING btree (title) WHERE title = 'alpha'",
                    vec![],
                )
                .unwrap();
        }

        let restarted = Cassie::new_with_data_dir(&path).unwrap();
        restarted.startup().unwrap();
        let session = restarted.create_session("tester", None);

        // Act
        let index = restarted
            .catalog
            .get_index(
                "sql_partial_index_restart",
                "sql_partial_index_restart_title_idx",
            )
            .expect("partial index should hydrate");
        let selected = restarted
            .execute_sql(
                &session,
                "EXPLAIN SELECT title FROM sql_partial_index_restart WHERE title = 'alpha'",
                vec![],
            )
            .unwrap();
        let fallback = restarted
            .execute_sql(
                &session,
                "EXPLAIN SELECT title FROM sql_partial_index_restart WHERE title = 'beta'",
                vec![],
            )
            .unwrap();

        // Assert
        assert!(index.predicate.is_some());
        let Value::String(selected_plan) = &selected.rows()[0][0] else {
            panic!("expected selected plan text");
        };
        assert!(selected_plan.contains("sql_partial_index_restart_title_idx"));
        let Value::String(fallback_plan) = &fallback.rows()[0][0] else {
            panic!("expected fallback plan text");
        };
        assert!(fallback_plan.contains("index=none"));

        let _ = std::fs::remove_dir_all(path);
    });
}
