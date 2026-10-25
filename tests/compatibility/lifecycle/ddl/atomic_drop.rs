use super::*;

#[path = "catalog_visibility.rs"]
mod catalog_visibility;
#[path = "transactional_index_visibility.rs"]
mod transactional_index_visibility;

#[test]
fn should_keep_all_drop_targets_when_a_later_target_is_missing() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("multi_drop_atomicity").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_multi_drop_first (id INT PRIMARY KEY)",
                "CREATE TABLE compat_multi_drop_second (id INT PRIMARY KEY)",
                "INSERT INTO compat_multi_drop_first VALUES (1)",
                "INSERT INTO compat_multi_drop_second VALUES (2)",
            ],
        )
        .await
        .expect("create populated DROP TABLE targets");

        // Act
        let drop_result = client
            .batch_execute("DROP TABLE compat_multi_drop_first, compat_multi_drop_missing")
            .await;
        let sqlstate = drop_result
            .as_ref()
            .err()
            .map(|error| db_error(error).code().code().to_owned());
        let first_rows = client
            .query("SELECT id FROM compat_multi_drop_first ORDER BY id", &[])
            .await;
        let second_rows = client
            .query("SELECT id FROM compat_multi_drop_second ORDER BY id", &[])
            .await;

        drop(client);
        server.shutdown(connection).await;

        // Assert
        assert_eq!(sqlstate.as_deref(), Some("42P01"));
        let first_rows = first_rows.expect("read the first table after the rejected multi-drop");
        let second_rows = second_rows.expect("read the second table after the rejected multi-drop");
        assert_eq!(
            first_rows
                .iter()
                .map(|row| row.get::<_, i32>(0))
                .collect::<Vec<_>>(),
            vec![1],
            "DROP TABLE must not remove an earlier target before a later target fails"
        );
        assert_eq!(
            second_rows
                .iter()
                .map(|row| row.get::<_, i32>(0))
                .collect::<Vec<_>>(),
            vec![2]
        );
    });
}

#[test]
fn should_skip_missing_targets_in_multi_name_drop_table_if_exists() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("multi_drop_if_exists").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_multi_if_exists_present (id INT PRIMARY KEY)",
                "INSERT INTO compat_multi_if_exists_present VALUES (1)",
            ],
        )
        .await
        .expect("create the present target");

        // Act
        let drop_result = client
            .batch_execute(
                "DROP TABLE IF EXISTS compat_multi_if_exists_present, \
                 compat_multi_if_exists_missing",
            )
            .await;
        let remaining = client
            .query("SELECT id FROM compat_multi_if_exists_present", &[])
            .await;

        // Assert
        drop(client);
        server.shutdown(connection).await;
        drop_result.expect("IF EXISTS must skip an absent member of the target list");
        let error = remaining.expect_err("the present table must still be dropped");
        assert_eq!(db_error(&error).code().code(), "42P01");
    });
}

#[test]
fn should_rollback_earlier_multi_drop_target_when_later_restrict_fails() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("multi_drop_restrict_atomicity").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_multi_drop_ext_first (id INT PRIMARY KEY)",
                "INSERT INTO compat_multi_drop_ext_first VALUES (1)",
                "CREATE TABLE compat_multi_drop_simple_first (id INT PRIMARY KEY)",
                "INSERT INTO compat_multi_drop_simple_first VALUES (2)",
                "CREATE TABLE compat_multi_drop_restricted (id INT PRIMARY KEY)",
                "INSERT INTO compat_multi_drop_restricted VALUES (3)",
                "CREATE VIEW compat_multi_drop_dependent \
                 AS SELECT id FROM compat_multi_drop_restricted",
            ],
        )
        .await
        .expect("create a first target and a later target with a dependent view");

        // Act
        let extended_result = client
            .execute(
                "DROP TABLE compat_multi_drop_ext_first, \
                 compat_multi_drop_restricted RESTRICT",
                &[],
            )
            .await;
        let extended_rows = client
            .query("SELECT id FROM compat_multi_drop_ext_first", &[])
            .await;
        let simple_result = client
            .batch_execute(
                "DROP TABLE compat_multi_drop_simple_first, \
                 compat_multi_drop_restricted RESTRICT",
            )
            .await;
        let simple_rows = client
            .query("SELECT id FROM compat_multi_drop_simple_first", &[])
            .await;

        // Assert
        drop(client);
        server.shutdown(connection).await;
        let extended_error =
            extended_result.expect_err("later dependent target must reject extended multi-drop");
        assert_eq!(db_error(&extended_error).code().code(), "2BP01");
        assert_eq!(
            extended_rows.expect("the first extended target must survive rollback")[0]
                .get::<_, i32>(0),
            1
        );
        let simple_error =
            simple_result.expect_err("later dependent target must reject simple multi-drop");
        assert_eq!(db_error(&simple_error).code().code(), "2BP01");
        assert_eq!(
            simple_rows.expect("the first Simple Query target must survive rollback")[0]
                .get::<_, i32>(0),
            2
        );
    });
}

#[test]
fn should_cascade_multi_drop_across_shared_dependencies() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("multi_drop_shared_cascade").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_multi_cascade_first (id INT PRIMARY KEY)",
                "CREATE TABLE compat_multi_cascade_second (id INT PRIMARY KEY)",
                "CREATE TABLE compat_multi_cascade_child_first \
                 (parent_id INT REFERENCES compat_multi_cascade_first(id))",
                "CREATE TABLE compat_multi_cascade_child_second \
                 (parent_id INT REFERENCES compat_multi_cascade_second(id))",
                "CREATE VIEW compat_multi_cascade_shared_view AS \
                 SELECT id FROM compat_multi_cascade_first \
                 UNION ALL SELECT id FROM compat_multi_cascade_second",
            ],
        )
        .await
        .expect("create two targets with a shared view and separate foreign keys");

        // Act
        client
            .batch_execute(
                "DROP TABLE compat_multi_cascade_first, \
                 compat_multi_cascade_second CASCADE",
            )
            .await
            .expect("cascade all dependencies across both DROP targets");
        let remaining: i64 = client
            .query_one(
                "SELECT count(*) FROM pg_catalog.pg_class \
                 WHERE relname IN (\
                   'compat_multi_cascade_first', \
                   'compat_multi_cascade_second', \
                   'compat_multi_cascade_shared_view'\
                 )",
                &[],
            )
            .await
            .expect("inspect dropped tables and shared view")
            .get(0);
        let child_first = client
            .execute(
                "INSERT INTO compat_multi_cascade_child_first VALUES (1)",
                &[],
            )
            .await;
        let child_second = client
            .execute(
                "INSERT INTO compat_multi_cascade_child_second VALUES (2)",
                &[],
            )
            .await;

        // Assert
        drop(client);
        server.shutdown(connection).await;
        assert_eq!(remaining, 0);
        child_first.expect("CASCADE must remove the first surviving child's foreign key");
        child_second.expect("CASCADE must remove the second surviving child's foreign key");
    });
}

#[test]
fn should_restore_every_multi_drop_target_after_explicit_rollback() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("multi_drop_explicit_rollback").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_multi_rollback_first (id INT PRIMARY KEY)",
                "INSERT INTO compat_multi_rollback_first VALUES (1)",
                "CREATE TABLE compat_multi_rollback_second (id INT PRIMARY KEY)",
                "INSERT INTO compat_multi_rollback_second VALUES (2)",
            ],
        )
        .await
        .expect("create populated tables for explicit rollback");

        // Act
        client
            .batch_execute("BEGIN")
            .await
            .expect("begin an explicit transaction");
        client
            .batch_execute(
                "DROP TABLE compat_multi_rollback_first, \
                 compat_multi_rollback_second",
            )
            .await
            .expect("drop both targets inside the explicit transaction");
        let staged_count: i64 = client
            .query_one(
                "SELECT count(*) FROM pg_catalog.pg_class \
                 WHERE relname IN (\
                   'compat_multi_rollback_first', 'compat_multi_rollback_second'\
                 )",
                &[],
            )
            .await
            .expect("inspect staged relation drops")
            .get(0);
        client
            .batch_execute("ROLLBACK")
            .await
            .expect("roll back both relation drops");
        let first_rows = client
            .query("SELECT id FROM compat_multi_rollback_first", &[])
            .await;
        let second_rows = client
            .query("SELECT id FROM compat_multi_rollback_second", &[])
            .await;

        // Assert
        drop(client);
        server.shutdown(connection).await;
        assert_eq!(staged_count, 0);
        assert_eq!(
            first_rows.expect("first table and row must be restored after rollback")[0]
                .get::<_, i32>(0),
            1
        );
        assert_eq!(
            second_rows.expect("second table and row must be restored after rollback")[0]
                .get::<_, i32>(0),
            2
        );
    });
}

#[test]
fn should_resolve_each_quoted_schema_qualified_multi_drop_target() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("multi_drop_qualified_names").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE SCHEMA compat_multi_drop_schema_first",
                "CREATE SCHEMA compat_multi_drop_schema_second",
                "CREATE SCHEMA compat_multi_drop_schema_keep",
                "CREATE TABLE compat_multi_drop_schema_first.\"SharedRows\" \
                 (id INT PRIMARY KEY)",
                "CREATE TABLE compat_multi_drop_schema_second.\"SharedRows\" \
                 (id INT PRIMARY KEY)",
                "CREATE TABLE compat_multi_drop_schema_keep.\"SharedRows\" \
                 (id INT PRIMARY KEY)",
                "INSERT INTO compat_multi_drop_schema_keep.\"SharedRows\" VALUES (3)",
            ],
        )
        .await
        .expect("create same-name tables in three schemas");

        // Act
        client
            .batch_execute(
                "DROP TABLE compat_multi_drop_schema_first.\"SharedRows\", \
                 compat_multi_drop_schema_second.\"SharedRows\"",
            )
            .await
            .expect("drop both explicitly qualified targets");
        let first = client
            .query(
                "SELECT id FROM compat_multi_drop_schema_first.\"SharedRows\"",
                &[],
            )
            .await;
        let second = client
            .query(
                "SELECT id FROM compat_multi_drop_schema_second.\"SharedRows\"",
                &[],
            )
            .await;
        let kept = client
            .query_one(
                "SELECT id FROM compat_multi_drop_schema_keep.\"SharedRows\"",
                &[],
            )
            .await;

        // Assert
        drop(client);
        server.shutdown(connection).await;
        assert_eq!(
            db_error(&first.expect_err("first listed relation must be dropped"))
                .code()
                .code(),
            "42P01"
        );
        assert_eq!(
            db_error(&second.expect_err("second listed relation must be dropped"))
                .code()
                .code(),
            "42P01"
        );
        assert_eq!(
            kept.expect("same-name table in unrelated schema remains")
                .get::<_, i32>(0),
            3
        );
    });
}

#[test]
fn should_keep_all_view_targets_when_a_later_view_is_missing() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("multi_drop_view_atomicity").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE VIEW compat_multi_drop_view_first AS SELECT 1 AS id",
                "CREATE VIEW compat_multi_drop_view_second AS SELECT 2 AS id",
            ],
        )
        .await
        .expect("create both view targets before multi-name DROP");

        // Act
        let drop_result = client
            .batch_execute("DROP VIEW compat_multi_drop_view_first, compat_multi_drop_view_missing")
            .await;
        let sqlstate = drop_result
            .as_ref()
            .err()
            .map(|error| db_error(error).code().code().to_owned());
        let remaining: i64 = client
            .query_one(
                "SELECT count(*) FROM pg_catalog.pg_class \
                 WHERE relname IN (\
                   'compat_multi_drop_view_first', 'compat_multi_drop_view_second'\
                 ) AND relkind = 'v'",
                &[],
            )
            .await
            .expect("inspect both views after the failed multi-name DROP")
            .try_get(0)
            .expect("remaining view count");

        // Assert
        drop(client);
        server.shutdown(connection).await;
        assert_eq!(sqlstate.as_deref(), Some("42P01"));
        assert_eq!(remaining, 2);
    });
}

#[test]
fn should_skip_missing_targets_in_multi_name_drop_view_if_exists() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("multi_drop_view_if_exists").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE VIEW compat_multi_view_if_exists_present AS SELECT 1 AS id")
            .await
            .expect("create the present view");

        // Act
        let drop_result = client
            .batch_execute(
                "DROP VIEW IF EXISTS compat_multi_view_if_exists_present, \
                 compat_multi_view_if_exists_missing",
            )
            .await;
        let remaining: i64 = client
            .query_one(
                "SELECT count(*) FROM pg_catalog.pg_class \
                 WHERE relname = 'compat_multi_view_if_exists_present' \
                   AND relkind = 'v'",
                &[],
            )
            .await
            .expect("inspect the dropped view")
            .get(0);

        // Assert
        drop(client);
        server.shutdown(connection).await;
        drop_result.expect("IF EXISTS must skip an absent view in the target list");
        assert_eq!(remaining, 0);
    });
}

#[test]
fn should_keep_all_type_targets_when_a_later_type_is_missing() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("multi_drop_type_atomicity").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TYPE compat_multi_drop_type AS (id INT)")
            .await
            .expect("create a composite type to drop");

        // Act
        let drop_result = client
            .batch_execute("DROP TYPE compat_multi_drop_type, compat_multi_drop_type_missing")
            .await;
        let sqlstate = drop_result
            .as_ref()
            .err()
            .map(|error| db_error(error).code().code().to_owned());
        let value = client
            .query_one("SELECT (ROW(18)::compat_multi_drop_type).id", &[])
            .await
            .expect("use the first type after the failed multi-name DROP");

        // Assert
        assert_eq!(sqlstate.as_deref(), Some("42704"));
        assert_eq!(value.get::<_, i32>(0), 18);

        drop(client);
        server.shutdown(connection).await;
    });
}
