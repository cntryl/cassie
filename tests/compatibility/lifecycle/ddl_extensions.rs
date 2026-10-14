use super::*;

mod index_contracts;
mod pgcrypto_extension;
mod table_forms;
mod transactional_catalog;
mod type_conversion;

#[test]
fn should_copy_rows_with_column_types_when_creating_table_as_select() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("create_table_as_select").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_ctas_source (id INT PRIMARY KEY, label TEXT NOT NULL)",
                "INSERT INTO compat_ctas_source VALUES (1, 'skip'), (2, 'two'), (3, 'three')",
            ],
        )
        .await
        .expect("create CTAS source rows");

        // Act
        client
            .batch_execute(
                "CREATE TABLE compat_ctas_copy AS \
                 SELECT id, label FROM compat_ctas_source WHERE id > 1 ORDER BY id",
            )
            .await
            .expect("create a table from a query result");
        let rows = client
            .query("SELECT id, label FROM compat_ctas_copy ORDER BY id", &[])
            .await
            .expect("read CTAS rows");
        let columns = client
            .query(
                "SELECT column_name, data_type FROM information_schema.columns \
                 WHERE table_name = 'compat_ctas_copy' ORDER BY ordinal_position",
                &[],
            )
            .await
            .expect("read CTAS column types");

        // Assert
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].get::<_, i32>(0), 2);
        assert_eq!(rows[0].get::<_, String>(1), "two");
        assert_eq!(rows[1].get::<_, i32>(0), 3);
        assert_eq!(rows[1].get::<_, String>(1), "three");
        assert_eq!(columns.len(), 2);
        assert_eq!(columns[0].get::<_, String>(0), "id");
        assert_eq!(columns[0].get::<_, String>(1), "integer");
        assert_eq!(columns[1].get::<_, String>(0), "label");
        assert_eq!(columns[1].get::<_, String>(1), "text");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_create_a_table_from_select_with_no_data() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("create_table_as_select_no_data").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_ctas_no_data_source (id INT, label TEXT)",
                "INSERT INTO compat_ctas_no_data_source VALUES (1, 'one'), (2, 'two')",
            ],
        )
        .await
        .expect("create a populated CTAS source");

        // Act
        let created = client
            .batch_execute(
                "CREATE TABLE compat_ctas_no_data AS \
                 SELECT id, label FROM compat_ctas_no_data_source WITH NO DATA",
            )
            .await;
        let rows = if created.is_ok() {
            Some(
                client
                    .query("SELECT id, label FROM compat_ctas_no_data", &[])
                    .await,
            )
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
            "CTAS WITH NO DATA should create an empty table; SQLSTATE: {create_sqlstate:?}"
        );
        assert_eq!(
            rows.expect("successful CTAS should be queryable")
                .unwrap()
                .len(),
            0
        );
    });
}

#[test]
fn should_preserve_cloned_table_defaults_constraints_indexes() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("create_table_like_lifecycle").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute(
                "CREATE TABLE compat_like_source (id INT PRIMARY KEY, label TEXT DEFAULT 'template')",
            )
            .await
            .expect("create LIKE source table");

        // Act
        client
            .batch_execute(
                "CREATE TABLE compat_like_copy (LIKE compat_like_source \
                 INCLUDING DEFAULTS INCLUDING CONSTRAINTS INCLUDING INDEXES)",
            )
            .await
            .expect("clone the source columns, defaults, constraints, and indexes");
        let inserted = client
            .query_one(
                "INSERT INTO compat_like_copy (id) VALUES (1) RETURNING label",
                &[],
            )
            .await
            .expect("use the cloned default");
        let duplicate = client
            .execute("INSERT INTO compat_like_copy (id) VALUES (1)", &[])
            .await
            .expect_err("the cloned primary key must reject duplicate values");
        let primary_keys: i64 = client
            .query_one(
                "SELECT count(*) FROM pg_catalog.pg_constraint \
                 WHERE conrelid = 'compat_like_copy'::regclass AND contype = 'p'",
                &[],
            )
            .await
            .expect("inspect the cloned primary-key constraint")
            .try_get(0)
            .expect("primary-key count");

        // Assert
        assert_eq!(inserted.get::<_, String>(0), "template");
        assert_eq!(db_error(&duplicate).code().code(), "23505");
        assert_eq!(primary_keys, 1);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_route_rows_to_the_matching_range_partition() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("range_partition_routing").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_range_parent (id INT, label TEXT) PARTITION BY RANGE (id)",
                "CREATE TABLE compat_range_low PARTITION OF compat_range_parent \
                   FOR VALUES FROM (MINVALUE) TO (10)",
                "CREATE TABLE compat_range_high PARTITION OF compat_range_parent \
                   FOR VALUES FROM (10) TO (MAXVALUE)",
                "INSERT INTO compat_range_parent VALUES (1, 'low'), (10, 'high')",
            ],
        )
        .await
        .expect("create range partitions and route boundary rows");

        // Act
        let rows = client
            .query("SELECT id, label FROM compat_range_parent ORDER BY id", &[])
            .await
            .expect("read routed rows through the partitioned parent");

        // Assert
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].get::<_, i32>(0), 1);
        assert_eq!(rows[0].get::<_, String>(1), "low");
        assert_eq!(rows[1].get::<_, i32>(0), 10);
        assert_eq!(rows[1].get::<_, String>(1), "high");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_route_rows_to_the_matching_hash_partition() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("hash_partition_routing").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_hash_parent (id INT, label TEXT) PARTITION BY HASH (id)",
                "CREATE TABLE compat_hash_even PARTITION OF compat_hash_parent \
                   FOR VALUES WITH (MODULUS 2, REMAINDER 0)",
                "CREATE TABLE compat_hash_odd PARTITION OF compat_hash_parent \
                   FOR VALUES WITH (MODULUS 2, REMAINDER 1)",
                "INSERT INTO compat_hash_parent VALUES \
                   (1, 'one'), (2, 'two'), (3, 'three'), (4, 'four')",
            ],
        )
        .await
        .expect("create hash partitions and route rows");

        // Act
        let rows = client
            .query("SELECT id, label FROM compat_hash_parent ORDER BY id", &[])
            .await
            .expect("read routed rows through the partitioned parent");

        // Assert
        assert_eq!(
            rows.iter()
                .map(|row| (row.get::<_, i32>(0), row.get::<_, String>(1)))
                .collect::<Vec<_>>(),
            vec![
                (1, "one".to_string()),
                (2, "two".to_string()),
                (3, "three".to_string()),
                (4, "four".to_string())
            ]
        );

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_route_unlisted_values_to_a_default_list_partition() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("default_list_partition_routing").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_default_parent (id INT, region TEXT) \
                   PARTITION BY LIST (region)",
                "CREATE TABLE compat_default_east PARTITION OF compat_default_parent \
                   FOR VALUES IN ('east')",
                "CREATE TABLE compat_default_fallback PARTITION OF compat_default_parent DEFAULT",
            ],
        )
        .await
        .expect("create explicit and default partitions");

        // Act
        client
            .batch_execute("INSERT INTO compat_default_parent VALUES (1, 'east'), (2, 'west')")
            .await
            .expect("route explicit and unlisted values");
        let parent_rows = client
            .query(
                "SELECT id, region FROM compat_default_parent ORDER BY id",
                &[],
            )
            .await
            .expect("read rows through the partitioned parent");
        let fallback_count: i64 = client
            .query_one("SELECT count(*) FROM compat_default_fallback", &[])
            .await
            .expect("count rows routed to the default partition")
            .try_get(0)
            .expect("fallback row count");

        // Assert
        assert_eq!(
            parent_rows
                .iter()
                .map(|row| (row.get::<_, i32>(0), row.get::<_, String>(1)))
                .collect::<Vec<_>>(),
            vec![(1, "east".to_string()), (2, "west".to_string())]
        );
        assert_eq!(fallback_count, 1);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_add_column_if_not_exists_without_replacing_existing_default() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("alter_add_column_if_not_exists").await;
        let (client, connection) = server.connect().await;
        let setup = execute_statements(
            &client,
            &[
                "CREATE TABLE compat_alter_add_column (id INT PRIMARY KEY)",
                "INSERT INTO compat_alter_add_column VALUES (1)",
                "ALTER TABLE compat_alter_add_column ADD COLUMN IF NOT EXISTS score INT DEFAULT 7",
                "ALTER TABLE compat_alter_add_column ADD COLUMN IF NOT EXISTS score INT DEFAULT 9",
                "INSERT INTO compat_alter_add_column (id) VALUES (2)",
            ],
        )
        .await;

        // Act
        let rows = if setup.is_ok() {
            Some(
                client
                    .query(
                        "SELECT id, score FROM compat_alter_add_column ORDER BY id",
                        &[],
                    )
                    .await,
            )
        } else {
            None
        };
        drop(client);
        server.shutdown(connection).await;

        // Assert
        assert!(
            setup.is_ok(),
            "ADD COLUMN IF NOT EXISTS should be accepted: {setup:?}"
        );
        let rows = rows
            .expect("successful ALTER statements should leave queryable rows")
            .expect("read rows after repeated ADD COLUMN IF NOT EXISTS");
        assert_eq!(rows.len(), 2);
        assert_eq!((rows[0].get::<_, i32>(0), rows[0].get::<_, i32>(1)), (1, 7));
        assert_eq!((rows[1].get::<_, i32>(0), rows[1].get::<_, i32>(1)), (2, 7));
    });
}

#[test]
fn should_drop_an_absent_table_if_exists_before_creating_it() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("drop_absent_table_if_exists").await;
        let (client, connection) = server.connect().await;

        // Act
        let drop_result = client
            .batch_execute("DROP TABLE IF EXISTS compat_drop_if_exists_rows")
            .await;
        let create_result = if drop_result.is_ok() {
            Some(
                client
                    .batch_execute(
                        "CREATE TABLE compat_drop_if_exists_rows (id INTEGER PRIMARY KEY)",
                    )
                    .await,
            )
        } else {
            None
        };
        let inserted_row = if create_result.as_ref().is_some_and(Result::is_ok) {
            Some(
                client
                    .query_one(
                        "INSERT INTO compat_drop_if_exists_rows VALUES (826) RETURNING id",
                        &[],
                    )
                    .await,
            )
        } else {
            None
        };
        drop(client);
        server.shutdown(connection).await;

        // Assert
        assert!(
            drop_result.is_ok(),
            "DROP TABLE IF EXISTS should succeed for a missing relation: {drop_result:?}"
        );
        assert!(
            create_result.as_ref().is_some_and(Result::is_ok),
            "CREATE TABLE should succeed after dropping a missing relation: {create_result:?}"
        );
        let inserted_row = inserted_row
            .expect("successful CREATE TABLE should allow the follow-up INSERT")
            .expect("insert after DROP TABLE IF EXISTS and CREATE TABLE");
        assert_eq!(inserted_row.get::<_, i32>(0), 826);
    });
}

#[test]
fn should_delete_temp_table_rows_on_commit() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("temp_delete_rows_on_commit").await;
        let (client, connection) = server.connect().await;
        let setup = execute_statements(
            &client,
            &[
                "BEGIN",
                "CREATE TEMP TABLE compat_temp_delete_rows (id INT) ON COMMIT DELETE ROWS",
                "INSERT INTO compat_temp_delete_rows VALUES (1), (2)",
                "COMMIT",
            ],
        )
        .await;

        // Act
        let count = if setup.is_ok() {
            Some(
                client
                    .query_one("SELECT count(*) FROM compat_temp_delete_rows", &[])
                    .await
                    .map(|row| row.get::<_, i64>(0)),
            )
        } else {
            let _ = client.batch_execute("ROLLBACK").await;
            None
        };
        drop(client);
        server.shutdown(connection).await;

        // Assert
        assert!(
            setup.is_ok(),
            "ON COMMIT DELETE ROWS should complete: {setup:?}"
        );
        assert_eq!(
            count
                .expect("table should remain after deleting its rows")
                .expect("read row count after commit"),
            0
        );
    });
}

#[test]
fn should_drop_an_index_without_an_on_clause() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("drop_index_without_on").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_drop_index_rows (id INT PRIMARY KEY, value TEXT)",
                "INSERT INTO compat_drop_index_rows VALUES (1, 'alpha'), (2, 'beta')",
                "CREATE INDEX compat_drop_index_idx ON compat_drop_index_rows (value)",
            ],
        )
        .await
        .expect("create rows and an index");

        // Act
        let drop_result = client
            .batch_execute("DROP INDEX compat_drop_index_idx")
            .await;
        let recreate_result = if drop_result.is_ok() {
            Some(
                client
                    .batch_execute(
                        "CREATE INDEX compat_drop_index_idx ON compat_drop_index_rows (value)",
                    )
                    .await,
            )
        } else {
            None
        };
        let rows_result = if recreate_result.as_ref().is_some_and(Result::is_ok) {
            Some(
                client
                    .query(
                        "SELECT id, value FROM compat_drop_index_rows ORDER BY id",
                        &[],
                    )
                    .await,
            )
        } else {
            None
        };
        drop(client);
        server.shutdown(connection).await;

        // Assert
        assert!(
            drop_result.is_ok(),
            "DROP INDEX without ON should succeed: {drop_result:?}"
        );
        recreate_result
            .expect("drop should permit index recreation")
            .expect("recreate the dropped index name");
        let rows = rows_result
            .expect("recreated index should allow a table query")
            .expect("read table rows after index removal");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].get::<_, i32>(0), 1);
        assert_eq!(rows[0].get::<_, String>(1), "alpha");
        assert_eq!(rows[1].get::<_, i32>(0), 2);
        assert_eq!(rows[1].get::<_, String>(1), "beta");
    });
}

#[test]
fn should_honor_sequence_state_across_setval_nextval_lastval() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("sequence_value_lifecycle").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE SEQUENCE compat_value_sequence")
            .await
            .expect("create a sequence");

        // Act
        let first: i64 = client
            .query_one("SELECT nextval('compat_value_sequence')", &[])
            .await
            .expect("read the initial sequence value")
            .try_get(0)
            .expect("first sequence value");
        let set_without_call: i64 = client
            .query_one("SELECT setval('compat_value_sequence', 40, false)", &[])
            .await
            .expect("set the sequence as not yet called")
            .try_get(0)
            .expect("setval result");
        let next_without_call: i64 = client
            .query_one("SELECT nextval('compat_value_sequence')", &[])
            .await
            .expect("return the set value on the next call")
            .try_get(0)
            .expect("next sequence value");
        let last_after_next: i64 = client
            .query_one("SELECT lastval()", &[])
            .await
            .expect("read the most recently returned nextval")
            .try_get(0)
            .expect("last sequence value");
        client
            .batch_execute("SELECT setval('compat_value_sequence', 50, true)")
            .await
            .expect("set the sequence as already called");
        let next_after_called: i64 = client
            .query_one("SELECT nextval('compat_value_sequence')", &[])
            .await
            .expect("advance from the set called value")
            .try_get(0)
            .expect("advanced sequence value");
        let last_after_advance: i64 = client
            .query_one("SELECT lastval()", &[])
            .await
            .expect("read the advanced sequence value")
            .try_get(0)
            .expect("last advanced value");

        // Assert
        assert_eq!(first, 1);
        assert_eq!(set_without_call, 40);
        assert_eq!(next_without_call, 40);
        assert_eq!(last_after_next, 40);
        assert_eq!(next_after_called, 51);
        assert_eq!(last_after_advance, 51);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_rollback_transactional_partition_attachment_detachment() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("partition_attach_detach").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_attach_parent (id INT, label TEXT) PARTITION BY RANGE (id)",
                "CREATE TABLE compat_attach_part (id INT, label TEXT)",
                "INSERT INTO compat_attach_part VALUES (1, 'one')",
                "ALTER TABLE compat_attach_parent ATTACH PARTITION compat_attach_part \
                   FOR VALUES FROM (0) TO (10)",
            ],
        )
        .await
        .expect("attach a populated table as a partition");
        let attached: i64 = client
            .query_one("SELECT count(*) FROM compat_attach_parent", &[])
            .await
            .expect("read the attached row through the parent")
            .try_get(0)
            .expect("parent row count");

        // Act
        client
            .batch_execute("BEGIN")
            .await
            .expect("begin transactional detach");
        client
            .batch_execute("ALTER TABLE compat_attach_parent DETACH PARTITION compat_attach_part")
            .await
            .expect("detach the partition inside a transaction");
        client
            .batch_execute("ROLLBACK")
            .await
            .expect("restore the attachment after rollback");
        let after_rollback: i64 = client
            .query_one("SELECT count(*) FROM compat_attach_parent", &[])
            .await
            .expect("read parent after rolled-back detach")
            .try_get(0)
            .expect("parent row count after rollback");
        client
            .batch_execute("ALTER TABLE compat_attach_parent DETACH PARTITION compat_attach_part")
            .await
            .expect("detach the partition permanently");
        let detached_parent: i64 = client
            .query_one("SELECT count(*) FROM compat_attach_parent", &[])
            .await
            .expect("read parent after detach")
            .try_get(0)
            .expect("detached parent row count");
        let detached_child: i64 = client
            .query_one("SELECT count(*) FROM compat_attach_part", &[])
            .await
            .expect("read rows retained by the detached table")
            .try_get(0)
            .expect("detached child row count");

        // Assert
        assert_eq!(attached, 1);
        assert_eq!(after_rollback, 1);
        assert_eq!(detached_parent, 0);
        assert_eq!(detached_child, 1);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_drop_a_sequence_with_its_owned_table_column() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("owned_sequence_lifecycle").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_owned_sequence_rows (id INT)",
                "CREATE SEQUENCE compat_owned_sequence",
                "ALTER SEQUENCE compat_owned_sequence \\
                   OWNED BY compat_owned_sequence_rows.id",
            ],
        )
        .await
        .expect("associate the sequence with its table column");

        // Act
        client
            .batch_execute("DROP TABLE compat_owned_sequence_rows")
            .await
            .expect("drop the owning table");
        let sequence_dropped: bool = client
            .query_one(
                "SELECT NOT EXISTS (SELECT 1 FROM pg_catalog.pg_class \
                 WHERE relname = 'compat_owned_sequence')",
                &[],
            )
            .await
            .expect("check the owned sequence dependency")
            .try_get(0)
            .expect("owned sequence nullness");

        // Assert
        assert!(sequence_dropped);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_copy_identity_sequence_independently_with_like_including_identity() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("like_identity_sequence").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_like_identity_source \
                   (id INT GENERATED BY DEFAULT AS IDENTITY, label TEXT)",
                "CREATE TABLE compat_like_identity_copy \
                   (LIKE compat_like_identity_source INCLUDING IDENTITY)",
            ],
        )
        .await
        .expect("clone identity semantics into a separate table");

        // Act
        let source_id: i32 = client
            .query_one(
                "INSERT INTO compat_like_identity_source (label) \
                 VALUES ('source') RETURNING id",
                &[],
            )
            .await
            .expect("generate the source identity")
            .try_get(0)
            .expect("source identity value");
        let copy_id: i32 = client
            .query_one(
                "INSERT INTO compat_like_identity_copy (label) \
                 VALUES ('copy') RETURNING id",
                &[],
            )
            .await
            .expect("generate the cloned table identity")
            .try_get(0)
            .expect("copied identity value");

        // Assert
        assert_eq!(source_id, 1);
        assert_eq!(
            copy_id, 1,
            "the cloned table must own an independent sequence"
        );

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_apply_multiple_alter_table_actions_to_existing_rows() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("alter_table_multiple_actions").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_multi_action_alter \
                   (id INT, label TEXT, score INT, active BOOL)",
                "INSERT INTO compat_multi_action_alter VALUES (1, 'kept', 7, TRUE)",
            ],
        )
        .await
        .expect("create a table with a row before altering it");

        // Act
        client
            .batch_execute(
                "ALTER TABLE compat_multi_action_alter \
                 DROP COLUMN score, DROP COLUMN active",
            )
            .await
            .expect("apply both ALTER actions in one statement");
        let row = client
            .query_one("SELECT * FROM compat_multi_action_alter", &[])
            .await
            .expect("read the original row through the altered schema");

        // Assert
        assert_eq!(row.get::<_, i32>(0), 1);
        assert_eq!(row.get::<_, String>(1), "kept");
        assert_eq!(row.len(), 2);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_atomically_rollback_table_creation_after_insert() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("transactional_table_creation").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("BEGIN")
            .await
            .expect("begin the DDL transaction");

        // Act
        client
            .batch_execute("CREATE TABLE compat_transactional_ddl_rows (id INT PRIMARY KEY)")
            .await
            .expect("create a table inside the open transaction");
        client
            .batch_execute("INSERT INTO compat_transactional_ddl_rows VALUES (1)")
            .await
            .expect("insert data into the uncommitted table");
        client
            .batch_execute("ROLLBACK")
            .await
            .expect("roll back both catalog and row changes");
        let table_count: i64 = client
            .query_one(
                "SELECT count(*) FROM information_schema.tables \
                 WHERE table_schema = 'public' \
                   AND table_name = 'compat_transactional_ddl_rows'",
                &[],
            )
            .await
            .expect("inspect table publication after rollback")
            .try_get(0)
            .expect("matching table count");

        // Assert
        assert_eq!(table_count, 0);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_drop_schema_dependents_with_cascade() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("drop_schema_cascade").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE SCHEMA compat_drop_schema_cascade",
                "CREATE TABLE compat_drop_schema_cascade.rows (id INT PRIMARY KEY)",
            ],
        )
        .await
        .expect("create a schema with a dependent table");

        // Act
        client
            .batch_execute("DROP SCHEMA compat_drop_schema_cascade CASCADE")
            .await
            .expect("drop the schema and its dependent table");
        let remaining: i64 = client
            .query_one(
                "SELECT count(*) FROM information_schema.schemata \
                 WHERE schema_name = 'compat_drop_schema_cascade'",
                &[],
            )
            .await
            .expect("inspect the schema catalog after DROP CASCADE")
            .try_get(0)
            .expect("matching schema count");

        // Assert
        assert_eq!(remaining, 0);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_move_table_between_schemas_without_changing_its_rows() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("alter_table_set_schema").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE SCHEMA compat_move_table_source",
                "CREATE SCHEMA compat_move_table_target",
                "CREATE TABLE compat_move_table_source.rows (id INT PRIMARY KEY, label TEXT)",
                "INSERT INTO compat_move_table_source.rows VALUES (1, 'preserved')",
            ],
        )
        .await
        .expect("create a populated source table and destination schema");

        // Act
        client
            .batch_execute(
                "ALTER TABLE compat_move_table_source.rows \
                 SET SCHEMA compat_move_table_target",
            )
            .await
            .expect("move the table to another schema");
        let rows = client
            .query(
                "SELECT id, label FROM compat_move_table_target.rows ORDER BY id",
                &[],
            )
            .await
            .expect("read the table after moving it");

        // Assert
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].get::<_, i32>(0), 1);
        assert_eq!(rows[0].get::<_, String>(1), "preserved");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_scope_temporary_sequence_to_its_creating_session() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("temporary_sequence_scope").await;
        let (owner, owner_connection) = server.connect().await;
        let (observer, observer_connection) = server.connect().await;
        owner
            .batch_execute("CREATE TEMP SEQUENCE compat_temp_sequence START WITH 7")
            .await
            .expect("create a session-local sequence");

        // Act
        let first_value: i64 = owner
            .query_one("SELECT nextval('compat_temp_sequence')", &[])
            .await
            .expect("advance the sequence in its owning session")
            .try_get(0)
            .expect("first sequence value");
        let invisible = observer
            .query_one("SELECT nextval('compat_temp_sequence')", &[])
            .await
            .expect_err("another session must not resolve a temporary sequence");
        drop(owner);
        owner_connection.abort();
        let _ = owner_connection.await;
        let (replacement, replacement_connection) = server.connect().await;
        replacement
            .batch_execute("CREATE TEMP SEQUENCE compat_temp_sequence START WITH 7")
            .await
            .expect("create the same temporary name after the owner disconnects");
        let replacement_value: i64 = replacement
            .query_one("SELECT nextval('compat_temp_sequence')", &[])
            .await
            .expect("advance the replacement session's independent sequence")
            .try_get(0)
            .expect("replacement sequence value");

        // Assert
        assert_eq!(first_value, 7);
        assert_eq!(db_error(&invisible).code().code(), "42P01");
        assert_eq!(replacement_value, 7);

        drop((observer, replacement));
        replacement_connection.abort();
        let _ = replacement_connection.await;
        server.shutdown(observer_connection).await;
    });
}

#[test]
fn should_apply_alter_sequence_increment_to_catalog_metadata() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("alter_sequence_increment").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE SEQUENCE compat_increment_sequence")
            .await
            .expect("create a sequence with default options");

        // Act
        client
            .batch_execute("ALTER SEQUENCE compat_increment_sequence INCREMENT BY 2")
            .await
            .expect("change the sequence increment");
        let increment: i64 = client
            .query_one(
                "SELECT increment_by FROM pg_catalog.pg_sequences \
                 WHERE schemaname = 'public' AND sequencename = 'compat_increment_sequence'",
                &[],
            )
            .await
            .expect("read the altered sequence metadata")
            .try_get(0)
            .expect("sequence increment");

        // Assert
        assert_eq!(increment, 2);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_apply_alter_sequence_cache_to_catalog_metadata() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("alter_sequence_cache").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE SEQUENCE compat_alter_cache_sequence")
            .await
            .expect("create a sequence with the default cache size");

        // Act
        client
            .batch_execute("ALTER SEQUENCE compat_alter_cache_sequence CACHE 7")
            .await
            .expect("change the sequence cache size");
        let cache_size: i64 = client
            .query_one(
                "SELECT cache_size FROM pg_catalog.pg_sequences \
                 WHERE schemaname = 'public' \
                   AND sequencename = 'compat_alter_cache_sequence'",
                &[],
            )
            .await
            .expect("read the altered sequence cache metadata")
            .try_get(0)
            .expect("sequence cache size");

        // Assert
        assert_eq!(cache_size, 7);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_rollback_alter_table_column_addition() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("transactional_alter_rollback").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_alter_rollback_rows (id INT PRIMARY KEY)",
                "INSERT INTO compat_alter_rollback_rows VALUES (1)",
                "BEGIN",
            ],
        )
        .await
        .expect("create the existing table and start a transaction");

        // Act
        client
            .batch_execute(
                "ALTER TABLE compat_alter_rollback_rows \
                 ADD COLUMN label TEXT DEFAULT 'temporary'",
            )
            .await
            .expect("add a column inside the open transaction");
        client
            .batch_execute("ROLLBACK")
            .await
            .expect("roll back the catalog mutation");
        let column_count: i64 = client
            .query_one(
                "SELECT count(*) FROM information_schema.columns \
                 WHERE table_schema = 'public' \
                   AND table_name = 'compat_alter_rollback_rows' \
                   AND column_name = 'label'",
                &[],
            )
            .await
            .expect("inspect the table after rollback")
            .try_get(0)
            .expect("matching column count");
        let rows: i64 = client
            .query_one("SELECT count(*) FROM compat_alter_rollback_rows", &[])
            .await
            .expect("read the existing row after rollback")
            .try_get(0)
            .expect("row count");

        // Assert
        assert_eq!(column_count, 0);
        assert_eq!(rows, 1);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_publish_sequence_cache_size_in_catalog_metadata() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("sequence_cache_metadata").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE SEQUENCE compat_cache_sequence CACHE 3")
            .await
            .expect("create a sequence with a nondefault cache size");

        // Act
        let cache_size: i64 = client
            .query_one(
                "SELECT cache_size FROM pg_catalog.pg_sequences \
                 WHERE schemaname = 'public' \
                   AND sequencename = 'compat_cache_sequence'",
                &[],
            )
            .await
            .expect("read the configured sequence cache size")
            .try_get(0)
            .expect("sequence cache size");

        // Assert
        assert_eq!(cache_size, 3);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_set_partition_pruning_compatibility_setting() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("partition_pruning_setting").await;
        let (client, connection) = server.connect().await;

        // Act
        client
            .batch_execute("SET enable_partition_pruning = off")
            .await
            .expect("accept the PostgreSQL partition-pruning setting");
        let setting: String = client
            .query_one("SHOW enable_partition_pruning", &[])
            .await
            .expect("read the configured partition-pruning setting")
            .try_get(0)
            .expect("partition-pruning setting");

        // Assert
        assert_eq!(setting, "off");

        drop(client);
        server.shutdown(connection).await;
    });
}
