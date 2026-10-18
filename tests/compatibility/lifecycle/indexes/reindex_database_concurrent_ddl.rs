use super::*;

async fn sqlstate_inside_transaction(client: &tokio_postgres::Client, statement: &str) -> String {
    client
        .batch_execute("BEGIN")
        .await
        .expect("begin a transaction for the maintenance statement");
    let error = client
        .batch_execute(statement)
        .await
        .expect_err("PostgreSQL requires this maintenance statement to run in autocommit mode");
    let sqlstate = db_error(&error).code().code().to_owned();
    client
        .batch_execute("ROLLBACK")
        .await
        .expect("clear the transaction after PostgreSQL rejects the statement");
    sqlstate
}

async fn quoted_current_database(client: &tokio_postgres::Client) -> String {
    let database_name: String = client
        .query_one("SELECT current_database()", &[])
        .await
        .expect("read the current database name")
        .try_get(0)
        .expect("database name");
    format!("\"{}\"", database_name.replace('"', "\"\""))
}

#[test]
fn should_reindex_current_database_in_both_name_forms() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("reindex_database_named_forms").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_reindex_database_rows \
                   (id INT PRIMARY KEY, payload TEXT)",
                "INSERT INTO compat_reindex_database_rows VALUES \
                   (2, 'beta'), (1, 'alpha')",
                "CREATE INDEX compat_reindex_database_payload_idx \
                   ON compat_reindex_database_rows (payload)",
            ],
        )
        .await
        .expect("create a user index in the isolated compatibility database");
        let database_identifier = quoted_current_database(&client).await;

        // Act
        client
            .batch_execute(&format!(
                "REINDEX (VERBOSE FALSE, CONCURRENTLY TRUE) DATABASE {database_identifier}"
            ))
            .await
            .expect("reindex the named current database with concurrent options");
        client
            .batch_execute("REINDEX DATABASE")
            .await
            .expect("reindex the current database when its name is omitted");
        let index_state = client
            .query_one(
                "SELECT indisready, indisvalid \
                 FROM pg_catalog.pg_index \
                 WHERE indexrelid = \
                   'compat_reindex_database_payload_idx'::regclass",
                &[],
            )
            .await
            .expect("read the user index state after database reindexing");
        let rows = client
            .query(
                "SELECT id, payload FROM compat_reindex_database_rows \
                 ORDER BY id",
                &[],
            )
            .await
            .expect("read table rows after database reindexing");

        // Assert
        assert!(index_state.get::<_, bool>(0));
        assert!(index_state.get::<_, bool>(1));
        assert_eq!(
            rows.iter()
                .map(|row| (row.get::<_, i32>(0), row.get::<_, String>(1)))
                .collect::<Vec<_>>(),
            vec![(1, "alpha".to_owned()), (2, "beta".to_owned())]
        );

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_reindex_system_catalogs_with_optional_database_name() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("reindex_system_named_forms").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_reindex_system_rows \
                   (id INT PRIMARY KEY, payload TEXT)",
                "INSERT INTO compat_reindex_system_rows VALUES \
                   (1, 'kept'), (2, 'also kept')",
                "CREATE INDEX compat_reindex_system_payload_idx \
                   ON compat_reindex_system_rows (payload)",
            ],
        )
        .await
        .expect("create a user index beside the system catalogs");
        let database_identifier = quoted_current_database(&client).await;

        // Act
        client
            .batch_execute(&format!(
                "REINDEX (VERBOSE FALSE, CONCURRENTLY FALSE) \
                 SYSTEM {database_identifier}"
            ))
            .await
            .expect("reindex system catalogs in the named current database");
        client
            .batch_execute("REINDEX SYSTEM")
            .await
            .expect("reindex system catalogs when the database name is omitted");
        let system_index_valid: bool = client
            .query_one(
                "SELECT indisvalid FROM pg_catalog.pg_index \
                 WHERE indexrelid = 'pg_catalog.pg_type_oid_index'::regclass",
                &[],
            )
            .await
            .expect("read a system-catalog index after system reindexing")
            .try_get(0)
            .expect("system index validity");
        let user_index_valid: bool = client
            .query_one(
                "SELECT indisvalid FROM pg_catalog.pg_index \
                 WHERE indexrelid = \
                   'compat_reindex_system_payload_idx'::regclass",
                &[],
            )
            .await
            .expect("read the user index after system reindexing")
            .try_get(0)
            .expect("user index validity");
        let rows = client
            .query(
                "SELECT id, payload FROM compat_reindex_system_rows ORDER BY id",
                &[],
            )
            .await
            .expect("read user rows after system-catalog reindexing");

        // Assert
        assert!(system_index_valid);
        assert!(user_index_valid);
        assert_eq!(
            rows.iter()
                .map(|row| (row.get::<_, i32>(0), row.get::<_, String>(1)))
                .collect::<Vec<_>>(),
            vec![(1, "kept".to_owned()), (2, "also kept".to_owned())]
        );

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_reject_concurrent_system_reindex_with_feature_not_supported() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("reindex_system_concurrently_rejected").await;
        let (client, connection) = server.connect().await;
        let database_identifier = quoted_current_database(&client).await;

        // Act
        let rejected = client
            .batch_execute(&format!(
                "REINDEX (VERBOSE FALSE, CONCURRENTLY TRUE) \
                 SYSTEM {database_identifier}"
            ))
            .await
            .expect_err("PostgreSQL does not reindex system catalogs concurrently");
        let system_index_valid: bool = client
            .query_one(
                "SELECT indisvalid FROM pg_catalog.pg_index \
                 WHERE indexrelid = 'pg_catalog.pg_type_oid_index'::regclass",
                &[],
            )
            .await
            .expect("read a system index after the rejected statement")
            .try_get(0)
            .expect("system index validity");

        // Assert
        assert_eq!(db_error(&rejected).code().code(), "0A000");
        assert!(system_index_valid);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_create_concurrent_index_with_ready_valid_catalog_state() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("create_index_concurrently_catalog_state").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_create_concurrent_index_rows \
                   (id INT, payload TEXT)",
                "INSERT INTO compat_create_concurrent_index_rows VALUES \
                   (2, 'beta'), (1, 'alpha')",
            ],
        )
        .await
        .expect("create table rows before concurrent index creation");

        // Act
        client
            .batch_execute(
                "CREATE INDEX CONCURRENTLY compat_create_concurrent_index_idx \
                 ON compat_create_concurrent_index_rows (payload)",
            )
            .await
            .expect("build a btree index concurrently outside a transaction");
        let index_state = client
            .query_one(
                "SELECT indisready, indisvalid, indislive \
                 FROM pg_catalog.pg_index \
                 WHERE indexrelid = \
                   'compat_create_concurrent_index_idx'::regclass",
                &[],
            )
            .await
            .expect("read concurrent index lifecycle flags");
        let rows = client
            .query(
                "SELECT id, payload FROM compat_create_concurrent_index_rows \
                 ORDER BY payload, id",
                &[],
            )
            .await
            .expect("read indexed rows after concurrent index creation");

        // Assert
        assert!(index_state.get::<_, bool>(0));
        assert!(index_state.get::<_, bool>(1));
        assert!(index_state.get::<_, bool>(2));
        assert_eq!(
            rows.iter()
                .map(|row| (row.get::<_, i32>(0), row.get::<_, String>(1)))
                .collect::<Vec<_>>(),
            vec![(1, "alpha".to_owned()), (2, "beta".to_owned())]
        );

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_reindex_index_concurrently_without_changing_query_results() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("reindex_index_concurrently_rows").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_reindex_concurrent_rows \
                   (id INT PRIMARY KEY, payload TEXT)",
                "INSERT INTO compat_reindex_concurrent_rows VALUES \
                   (3, 'gamma'), (1, 'alpha'), (2, 'beta')",
                "CREATE INDEX compat_reindex_concurrent_idx \
                   ON compat_reindex_concurrent_rows (payload)",
            ],
        )
        .await
        .expect("create indexed rows for concurrent reindexing");

        // Act
        client
            .batch_execute(
                "REINDEX (VERBOSE FALSE, CONCURRENTLY TRUE) \
                 INDEX compat_reindex_concurrent_idx",
            )
            .await
            .expect("rebuild the index concurrently outside a transaction");
        let index_state = client
            .query_one(
                "SELECT indisready, indisvalid FROM pg_catalog.pg_index \
                 WHERE indexrelid = 'compat_reindex_concurrent_idx'::regclass",
                &[],
            )
            .await
            .expect("read index state after concurrent reindexing");
        let rows = client
            .query(
                "SELECT id, payload FROM compat_reindex_concurrent_rows \
                 ORDER BY payload, id",
                &[],
            )
            .await
            .expect("read indexed rows after concurrent reindexing");

        // Assert
        assert!(index_state.get::<_, bool>(0));
        assert!(index_state.get::<_, bool>(1));
        assert_eq!(
            rows.iter()
                .map(|row| (row.get::<_, i32>(0), row.get::<_, String>(1)))
                .collect::<Vec<_>>(),
            vec![
                (1, "alpha".to_owned()),
                (2, "beta".to_owned()),
                (3, "gamma".to_owned()),
            ]
        );

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_require_autocommit_for_concurrent_index_ddl() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("concurrent_index_transaction_boundary").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_concurrent_index_transaction_rows (id INT, payload TEXT)",
                "INSERT INTO compat_concurrent_index_transaction_rows VALUES \
                   (1, 'alpha'), (2, 'beta')",
                "CREATE INDEX compat_concurrent_index_transaction_idx \
                   ON compat_concurrent_index_transaction_rows (payload)",
            ],
        )
        .await
        .expect("create a table and valid index for transaction-boundary checks");

        // Act
        let create_state = sqlstate_inside_transaction(
            &client,
            "CREATE INDEX CONCURRENTLY compat_concurrent_index_transaction_new_idx \
             ON compat_concurrent_index_transaction_rows (id)",
        )
        .await;
        let drop_state = sqlstate_inside_transaction(
            &client,
            "DROP INDEX CONCURRENTLY compat_concurrent_index_transaction_idx",
        )
        .await;
        let reindex_state = sqlstate_inside_transaction(
            &client,
            "REINDEX INDEX CONCURRENTLY compat_concurrent_index_transaction_idx",
        )
        .await;
        let created_index_exists: bool = client
            .query_one(
                "SELECT pg_catalog.to_regclass( \
                   'compat_concurrent_index_transaction_new_idx') IS NOT NULL",
                &[],
            )
            .await
            .expect("check the failed concurrent creation left no index")
            .try_get(0)
            .expect("created index state");
        let original_index_valid: bool = client
            .query_one(
                "SELECT indisvalid FROM pg_catalog.pg_index \
                 WHERE indexrelid = \
                   'compat_concurrent_index_transaction_idx'::regclass",
                &[],
            )
            .await
            .expect("check the original index after rejected operations")
            .try_get(0)
            .expect("original index validity");

        // Assert
        assert_eq!(create_state, "25001");
        assert_eq!(drop_state, "25001");
        assert_eq!(reindex_state, "25001");
        assert!(!created_index_exists);
        assert!(original_index_valid);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_require_autocommit_for_database_reindex_scopes() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("reindex_database_transaction_boundary").await;
        let (client, connection) = server.connect().await;
        let database_identifier = quoted_current_database(&client).await;

        // Act
        let database_state = sqlstate_inside_transaction(
            &client,
            &format!("REINDEX DATABASE {database_identifier}"),
        )
        .await;
        let system_state =
            sqlstate_inside_transaction(&client, &format!("REINDEX SYSTEM {database_identifier}"))
                .await;

        // Assert
        assert_eq!(database_state, "25001");
        assert_eq!(system_state, "25001");

        drop(client);
        server.shutdown(connection).await;
    });
}
