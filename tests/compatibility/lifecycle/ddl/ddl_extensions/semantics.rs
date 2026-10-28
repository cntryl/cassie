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
                "SELECT count(*) FROM information_schema.table_constraints \
                 WHERE table_schema = 'public' AND table_name = 'compat_like_copy' \
                   AND constraint_type = 'PRIMARY KEY'",
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
fn should_reject_drop_table_if_exists_for_a_virtual_view() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("drop_virtual_view_if_exists").await;
        let (client, connection) = server.connect().await;

        // Act
        let drop_result = client
            .batch_execute("DROP TABLE IF EXISTS pg_catalog.pg_class")
            .await;
        drop(client);
        server.shutdown(connection).await;

        // Assert
        let error = drop_result.expect_err("DROP TABLE must reject a virtual view");
        let db_error = error
            .as_db_error()
            .expect("virtual view rejection should be a database error");
        assert!(
            db_error.message().contains("is a view"),
            "unexpected error when dropping a virtual view: {}",
            db_error.message()
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
fn should_bind_constraints_against_the_updated_multi_action_schema() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("alter_table_dependent_actions").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_dependent_alter (id INT PRIMARY KEY)",
                "INSERT INTO compat_dependent_alter VALUES (1), (2)",
            ],
        )
        .await
        .expect("create existing rows before the multi-action ALTER");

        // Act
        let alter_result = client
            .batch_execute(
                "ALTER TABLE compat_dependent_alter \
                 ADD COLUMN new_key INT, \
                 RENAME COLUMN new_key TO renamed_key, \
                 ADD CONSTRAINT compat_dependent_alter_new_key UNIQUE (renamed_key)",
            )
            .await;
        let null_keys = client
            .query_one(
                "SELECT count(*) FROM compat_dependent_alter WHERE renamed_key IS NULL",
                &[],
            )
            .await;
        let duplicate_result = client
            .batch_execute(
                "INSERT INTO compat_dependent_alter (id, renamed_key) VALUES (3, 7), (4, 7)",
            )
            .await;

        // Assert
        drop(client);
        server.shutdown(connection).await;
        alter_result.expect("the later constraint should bind to the earlier added column");
        assert_eq!(
            null_keys
                .expect("read rows after adding the constrained column")
                .get::<_, i64>(0),
            2
        );
        let error = duplicate_result.expect_err("the new UNIQUE constraint must remain active");
        assert_eq!(db_error(&error).code().code(), "23505");
    });
}

#[test]
fn should_rollback_defaulted_column_when_its_later_constraint_fails() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("alter_new_column_constraint_rollback").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_new_column_rollback (id INT PRIMARY KEY)",
                "INSERT INTO compat_new_column_rollback VALUES (1), (2)",
                "CREATE TABLE compat_new_column_simple_rollback (id INT PRIMARY KEY)",
                "INSERT INTO compat_new_column_simple_rollback VALUES (1), (2)",
            ],
        )
        .await
        .expect("create populated tables for dependent constraint failures");

        // Act
        let extended_result = client
            .execute(
                "ALTER TABLE compat_new_column_rollback \
                 ADD COLUMN new_key INT DEFAULT 1, \
                 ADD CONSTRAINT compat_new_column_rollback_key UNIQUE (new_key)",
                &[],
            )
            .await;
        let simple_result = client
            .batch_execute(
                "ALTER TABLE compat_new_column_simple_rollback \
                 ADD COLUMN new_key INT DEFAULT 1, \
                 ADD CONSTRAINT compat_new_column_simple_key UNIQUE (new_key)",
            )
            .await;
        let remaining_columns = client
            .query_one(
                "SELECT count(*) FROM information_schema.columns \
                 WHERE table_name IN (\
                   'compat_new_column_rollback', \
                   'compat_new_column_simple_rollback'\
                 ) AND column_name = 'new_key'",
                &[],
            )
            .await;

        // Assert
        drop(client);
        server.shutdown(connection).await;
        let extended_error = extended_result
            .expect_err("duplicate backfilled values must fail UNIQUE validation");
        let simple_error =
            simple_result.expect_err("duplicate backfilled values must fail UNIQUE validation");
        for error in [&extended_error, &simple_error] {
            assert_eq!(db_error(error).code().code(), "23505");
        }
        assert_eq!(
            remaining_columns
                .expect("inspect the schema after both failed ALTER statements")
                .get::<_, i64>(0),
            0,
            "neither protocol may publish a column from a failed ALTER"
        );
    });
}

#[test]
fn should_rollback_earlier_alter_action_when_later_constraint_validation_fails_in_both_protocols() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("multi_action_alter_rollback").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_multi_action_rollback \
                 (id INT, duplicate_key INT)",
                "INSERT INTO compat_multi_action_rollback VALUES (1, 5), (2, 5)",
                "CREATE TABLE compat_multi_action_simple_rollback \
                 (id INT, duplicate_key INT)",
                "INSERT INTO compat_multi_action_simple_rollback VALUES (1, 6), (2, 6)",
            ],
        )
        .await
        .expect("create duplicate values for a later UNIQUE validation failure");

        // Act
        let alter_result = client
            .execute(
                "ALTER TABLE compat_multi_action_rollback \
                 ADD COLUMN transient INT, \
                 ADD CONSTRAINT unique_duplicate_key UNIQUE (duplicate_key)",
                &[],
            )
            .await;
        let transient_columns: i64 = client
            .query_one(
                "SELECT count(*) FROM information_schema.columns \
                 WHERE table_name = 'compat_multi_action_rollback' \
                   AND column_name = 'transient'",
                &[],
            )
            .await
            .expect("inspect the earlier action after the failed ALTER")
            .get(0);
        let simple_alter_result = client
            .batch_execute(
                "ALTER TABLE compat_multi_action_simple_rollback \
                 ADD COLUMN transient INT, \
                 ADD CONSTRAINT unique_duplicate_key_simple UNIQUE (duplicate_key)",
            )
            .await;
        let simple_transient_columns: i64 = client
            .query_one(
                "SELECT count(*) FROM information_schema.columns \
                 WHERE table_name = 'compat_multi_action_simple_rollback' \
                   AND column_name = 'transient'",
                &[],
            )
            .await
            .expect("inspect the earlier action after the Simple Query ALTER failure")
            .get(0);

        // Assert
        drop(client);
        server.shutdown(connection).await;
        let error = alter_result.expect_err("UNIQUE validation must reject duplicate keys");
        assert_eq!(db_error(&error).code().code(), "23505");
        assert_eq!(transient_columns, 0);
        let simple_error =
            simple_alter_result.expect_err("Simple Query must reject duplicate unique keys");
        assert_eq!(db_error(&simple_error).code().code(), "23505");
        assert_eq!(simple_transient_columns, 0);
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
                "CREATE SCHEMA compat_drop_schema_cascade_refs",
                "CREATE TABLE compat_drop_schema_cascade.rows \
                 (id INT PRIMARY KEY)",
                "INSERT INTO compat_drop_schema_cascade.rows VALUES (9)",
                "CREATE VIEW compat_drop_schema_cascade.row_view \
                 AS SELECT id FROM compat_drop_schema_cascade.rows",
                "CREATE TABLE compat_drop_schema_cascade_refs.child \
                 (parent_id INT REFERENCES compat_drop_schema_cascade.rows(id))",
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
        let remaining_relations: i64 = client
            .query_one(
                "SELECT count(*) FROM pg_catalog.pg_class \
                 WHERE relname IN ('rows', 'row_view')",
                &[],
            )
            .await
            .expect("inspect dependent relations after DROP CASCADE")
            .get(0);
        let surviving_child_insert = client
            .execute(
                "INSERT INTO compat_drop_schema_cascade_refs.child VALUES (99)",
                &[],
            )
            .await;

        // Assert
        assert_eq!(remaining, 0);
        assert_eq!(remaining_relations, 0);
        assert!(
            surviving_child_insert.is_ok(),
            "DROP SCHEMA CASCADE should remove the incoming foreign key from a surviving schema: {surviving_child_insert:?}"
        );

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_preserve_schema_objects_when_drop_schema_restricts_dependents() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("drop_schema_restrict").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE SCHEMA compat_drop_schema_restrict",
                "CREATE TABLE compat_drop_schema_restrict.rows (id INT)",
                "INSERT INTO compat_drop_schema_restrict.rows VALUES (11)",
            ],
        )
        .await
        .expect("create a non-empty schema for DROP RESTRICT");

        // Act
        let drop_result = client
            .batch_execute("DROP SCHEMA compat_drop_schema_restrict RESTRICT")
            .await;
        let row_count: i64 = client
            .query_one(
                "SELECT count(*) FROM compat_drop_schema_restrict.rows WHERE id = 11",
                &[],
            )
            .await
            .expect("read the row after DROP RESTRICT rejects the schema drop")
            .get(0);

        // Assert
        drop(client);
        server.shutdown(connection).await;
        assert!(drop_result.is_err(), "RESTRICT must reject a non-empty schema");
        assert_eq!(row_count, 1);
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
