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

#[test]
fn should_preserve_unique_null_policy_after_restart() {
    // Arrange
    use_local_storage();
    let path = data_dir("unique_null_policy_restart");
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
                    "CREATE TABLE sql_unique_null_policy_restart (key_value INT)",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO sql_unique_null_policy_restart VALUES (NULL)",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "CREATE UNIQUE INDEX sql_unique_null_policy_restart_idx \
                     ON sql_unique_null_policy_restart (key_value) NULLS NOT DISTINCT",
                    vec![],
                )
                .unwrap();
        }

        let restarted = Cassie::new_with_data_dir(&path).unwrap();
        restarted.startup().unwrap();
        let session = restarted.create_session("tester", None);

        // Act
        let metadata = restarted
            .execute_sql(
                &session,
                "SELECT indnullsnotdistinct FROM pg_catalog.pg_index \
                 WHERE indexrelid = 'sql_unique_null_policy_restart_idx'",
                vec![],
            )
            .unwrap();
        let duplicate_null = restarted.execute_sql(
            &session,
            "INSERT INTO sql_unique_null_policy_restart VALUES (NULL)",
            vec![],
        );

        // Assert
        assert_eq!(metadata.rows(), &[vec![Value::Bool(true)]]);
        assert!(
            duplicate_null.is_err(),
            "the hydrated unique index must continue to reject duplicate NULL keys"
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_recover_partial_index_entries_after_committed_writes_on_restart() {
    // Arrange
    use_local_storage();
    let path = data_dir("partial_index_entries_restart");
    {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        for statement in [
            "CREATE TABLE sql_partial_index_entries_restart \
               (id INT PRIMARY KEY, key_value INT, active BOOLEAN NOT NULL, payload TEXT)",
            "INSERT INTO sql_partial_index_entries_restart VALUES \
               (1, 30, TRUE, 'delete'), (2, 20, FALSE, 'enter'), \
               (3, 30, TRUE, 'leave'), (4, 40, FALSE, 'inactive-40'), \
               (5, 30, TRUE, 'key-leave'), (6, 30, TRUE, 'stay'), \
               (7, 70, FALSE, 'inactive-70'), (8, 80, FALSE, 'inactive-80'), \
               (9, 90, FALSE, 'inactive-90'), (10, 100, FALSE, 'inactive-100'), \
               (11, 110, FALSE, 'inactive-110'), (12, 120, FALSE, 'inactive-120')",
            "CREATE INDEX sql_partial_index_entries_restart_idx \
               ON sql_partial_index_entries_restart (key_value) INCLUDE (payload) \
               WHERE active AND key_value = 30",
            "UPDATE sql_partial_index_entries_restart \
               SET active = TRUE, key_value = 30, payload = 'entered' WHERE id = 2",
            "UPDATE sql_partial_index_entries_restart \
               SET active = FALSE, payload = 'left-predicate' WHERE id = 3",
            "UPDATE sql_partial_index_entries_restart \
               SET key_value = 20 WHERE id = 5",
            "UPDATE sql_partial_index_entries_restart \
               SET payload = 'updated' WHERE id = 6",
            "DELETE FROM sql_partial_index_entries_restart WHERE id = 1",
            "INSERT INTO sql_partial_index_entries_restart \
               VALUES (13, 30, TRUE, 'inserted')",
        ] {
            cassie
                .execute_sql(&session, statement, vec![])
                .unwrap_or_else(|error| panic!("execute {statement}: {error}"));
        }
    }

    let restarted = Cassie::new_with_data_dir(&path).unwrap();
    restarted.startup().unwrap();
    let session = restarted.create_session("tester", None);

    // Act
    let indexed_rows = restarted
        .execute_sql(
            &session,
            "SELECT key_value, payload FROM sql_partial_index_entries_restart \
             WHERE active AND key_value = 30 ORDER BY payload",
            vec![],
        )
        .unwrap()
        .rows()
        .to_vec();
    let explain = restarted
        .execute_sql(
            &session,
            "EXPLAIN SELECT key_value, payload \
             FROM sql_partial_index_entries_restart \
             WHERE active AND key_value = 30 ORDER BY payload",
            vec![],
        )
        .unwrap();
    let Value::String(plan) = &explain.rows()[0][0] else {
        panic!("expected textual plan");
    };
    restarted
        .execute_sql(&session, "DROP INDEX sql_partial_index_entries_restart_idx", vec![])
        .unwrap();
    let scanned_rows = restarted
        .execute_sql(
            &session,
            "SELECT key_value, payload FROM sql_partial_index_entries_restart \
             WHERE active AND key_value = 30 ORDER BY payload",
            vec![],
        )
        .unwrap()
        .rows()
        .to_vec();

    // Assert
    assert!(plan.contains("sql_partial_index_entries_restart_idx"));
    assert_eq!(
        indexed_rows,
        vec![
            vec![Value::Int64(30), Value::String("entered".to_owned())],
            vec![Value::Int64(30), Value::String("inserted".to_owned())],
            vec![Value::Int64(30), Value::String("updated".to_owned())],
        ]
    );
    assert_eq!(scanned_rows, indexed_rows);

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_recover_composite_index_entries_after_committed_writes_on_restart() {
    // Arrange
    use_local_storage();
    let path = data_dir("composite_index_entries_restart");
    {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        for statement in [
            "CREATE TABLE sql_composite_index_entries_restart \
               (id INT PRIMARY KEY, tenant_id TEXT NOT NULL, status TEXT NOT NULL, payload TEXT)",
            "INSERT INTO sql_composite_index_entries_restart VALUES \
               (1, 'tenant-a', 'open', 'move-in'), \
               (2, 'tenant-a', 'closed', 'delete'), \
               (3, 'tenant-b', 'closed', 'other-tenant'), \
               (4, 'tenant-a', 'open', 'move-out'), \
               (6, 'tenant-a', 'closed', 'existing')",
            "CREATE INDEX sql_composite_index_entries_restart_idx \
               ON sql_composite_index_entries_restart (tenant_id, status)",
            "UPDATE sql_composite_index_entries_restart \
               SET status = 'closed', payload = 'moved-in' WHERE id = 1",
            "UPDATE sql_composite_index_entries_restart \
               SET tenant_id = 'tenant-b', status = 'closed' WHERE id = 4",
            "DELETE FROM sql_composite_index_entries_restart WHERE id = 2",
            "INSERT INTO sql_composite_index_entries_restart \
               VALUES (5, 'tenant-a', 'closed', 'inserted')",
        ] {
            cassie
                .execute_sql(&session, statement, vec![])
                .unwrap_or_else(|error| panic!("execute {statement}: {error}"));
        }
    }

    let restarted = Cassie::new_with_data_dir(&path).unwrap();
    restarted.startup().unwrap();
    let session = restarted.create_session("tester", None);

    // Act
    let indexed_rows = restarted
        .execute_sql(
            &session,
            "SELECT id, payload FROM sql_composite_index_entries_restart \
             WHERE tenant_id = 'tenant-a' AND status = 'closed' ORDER BY id",
            vec![],
        )
        .unwrap()
        .rows()
        .to_vec();
    let explain = restarted
        .execute_sql(
            &session,
            "EXPLAIN SELECT id, payload \
             FROM sql_composite_index_entries_restart \
             WHERE tenant_id = 'tenant-a' AND status = 'closed' ORDER BY id",
            vec![],
        )
        .unwrap();
    let Value::String(plan) = &explain.rows()[0][0] else {
        panic!("expected textual plan");
    };
    restarted
        .execute_sql(
            &session,
            "DROP INDEX sql_composite_index_entries_restart_idx",
            vec![],
        )
        .unwrap();
    let scanned_rows = restarted
        .execute_sql(
            &session,
            "SELECT id, payload FROM sql_composite_index_entries_restart \
             WHERE tenant_id = 'tenant-a' AND status = 'closed' ORDER BY id",
            vec![],
        )
        .unwrap()
        .rows()
        .to_vec();

    // Assert
    assert!(
        plan.contains("sql_composite_index_entries_restart_idx"),
        "the hydrated composite index should serve its matching query: {plan}"
    );
    assert_eq!(
        indexed_rows,
        vec![
            vec![Value::Int64(1), Value::String("moved-in".to_owned())],
            vec![Value::Int64(5), Value::String("inserted".to_owned())],
            vec![Value::Int64(6), Value::String("existing".to_owned())],
        ]
    );
    assert_eq!(scanned_rows, indexed_rows);

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_recover_expression_index_after_committed_rekey() {
    // Arrange
    use_local_storage();
    let path = data_dir("expression_index_entries_restart");
    {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        for statement in [
            "CREATE TABLE sql_expression_index_entries_restart \
               (id INT PRIMARY KEY, email TEXT NOT NULL)",
            "INSERT INTO sql_expression_index_entries_restart VALUES \
               (1, 'Alice@example.com'), (2, 'bob@example.com'), \
               (3, 'carol@example.com')",
            "CREATE INDEX sql_expression_index_entries_restart_lower_idx \
               ON sql_expression_index_entries_restart ((lower(email)))",
        ] {
            cassie
                .execute_sql(&session, statement, vec![])
                .unwrap_or_else(|error| panic!("execute {statement}: {error}"));
        }
        for statement in [
            "UPDATE sql_expression_index_entries_restart \
               SET email = 'ALICE@NEW.EXAMPLE.COM' WHERE id = 1",
            "DELETE FROM sql_expression_index_entries_restart WHERE id = 2",
            "INSERT INTO sql_expression_index_entries_restart VALUES \
               (4, 'alice@new.example.com')",
        ] {
            cassie
                .execute_sql(&session, statement, vec![])
                .unwrap_or_else(|error| panic!("execute {statement}: {error}"));
        }
    }

    let restarted = Cassie::new_with_data_dir(&path).unwrap();
    restarted.startup().unwrap();
    let session = restarted.create_session("tester", None);

    // Act
    let indexed_rows = restarted
        .execute_sql(
            &session,
            "SELECT id FROM sql_expression_index_entries_restart \
             WHERE lower(email) = 'alice@new.example.com' ORDER BY id",
            vec![],
        )
        .unwrap()
        .rows()
        .to_vec();
    let explain = restarted
        .execute_sql(
            &session,
            "EXPLAIN SELECT id FROM sql_expression_index_entries_restart \
             WHERE lower(email) = 'alice@new.example.com' ORDER BY id",
            vec![],
        )
        .unwrap();
    let Value::String(plan) = &explain.rows()[0][0] else {
        panic!("expected textual plan");
    };
    restarted
        .execute_sql(
            &session,
            "DROP INDEX sql_expression_index_entries_restart_lower_idx",
            vec![],
        )
        .unwrap();
    let scanned_rows = restarted
        .execute_sql(
            &session,
            "SELECT id FROM sql_expression_index_entries_restart \
             WHERE lower(email) = 'alice@new.example.com' ORDER BY id",
            vec![],
        )
        .unwrap()
        .rows()
        .to_vec();

    // Assert
    assert!(
        plan.contains("sql_expression_index_entries_restart_lower_idx"),
        "the hydrated expression index should serve its matching query: {plan}"
    );
    assert_eq!(
        indexed_rows,
        vec![vec![Value::Int64(1)], vec![Value::Int64(4)]]
    );
    assert_eq!(scanned_rows, indexed_rows);

    let _ = std::fs::remove_dir_all(path);
}
