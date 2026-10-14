use super::*;

mod schema_authorization_elements;
mod schema_owner_transfer;
mod temp_views;

#[test]
fn should_keep_global_temporary_tables_isolated_to_their_session() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("global_temporary_table_isolation").await;
        let (owner, owner_connection) = server.connect().await;
        let (observer, observer_connection) = server.connect().await;
        owner
            .batch_execute(
                "CREATE GLOBAL TEMPORARY TABLE compat_global_temp_rows \
                 (id INTEGER) ON COMMIT PRESERVE ROWS",
            )
            .await
            .expect("create a PostgreSQL GLOBAL TEMPORARY table");
        owner
            .execute("INSERT INTO compat_global_temp_rows VALUES (18)", &[])
            .await
            .expect("insert a row into the owner's temporary table");

        // Act
        let invisible = observer
            .query("SELECT id FROM compat_global_temp_rows", &[])
            .await
            .expect_err("a second session must not resolve the owner's temp table");
        observer
            .batch_execute(
                "CREATE GLOBAL TEMPORARY TABLE compat_global_temp_rows \
                 (id INTEGER) ON COMMIT PRESERVE ROWS",
            )
            .await
            .expect("create an isolated temp table with the same relation name");
        observer
            .execute("INSERT INTO compat_global_temp_rows VALUES (19)", &[])
            .await
            .expect("insert a row into the observer's temporary table");
        let owner_row = owner
            .query_one("SELECT id FROM compat_global_temp_rows", &[])
            .await
            .expect("read the owner's temporary row");
        let observer_row = observer
            .query_one("SELECT id FROM compat_global_temp_rows", &[])
            .await
            .expect("read the observer's temporary row");

        // Assert
        assert_eq!(db_error(&invisible).code().code(), "42P01");
        assert_eq!(owner_row.get::<_, i32>(0), 18);
        assert_eq!(observer_row.get::<_, i32>(0), 19);

        drop((owner, observer));
        owner_connection.abort();
        let _ = owner_connection.await;
        server.shutdown(observer_connection).await;
    });
}

#[test]
fn should_accept_local_temporary_table_noise_word() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("local_temporary_table_noise_word").await;
        let (client, connection) = server.connect().await;

        // Act
        let result = client
            .batch_execute("CREATE LOCAL TEMPORARY TABLE compat_local_temp_rows (id INTEGER)")
            .await;
        let row = if result.is_ok() {
            Some(
                client
                    .query_one(
                        "INSERT INTO compat_local_temp_rows VALUES (18) RETURNING id",
                        &[],
                    )
                    .await,
            )
        } else {
            None
        };

        // Assert
        assert!(
            result.is_ok(),
            "PostgreSQL accepts LOCAL TEMPORARY as a temp-table noise word: {result:?}"
        );
        assert_eq!(
            row.expect("created table should accept an insert")
                .expect("insert into the local temporary table")
                .get::<_, i32>(0),
            18
        );

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_create_a_temporary_table_from_a_select_into_statement() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("select_into_temp_table").await;
        let (client, connection) = server.connect().await;

        // Act
        let creation = client
            .batch_execute("SELECT 18 AS id INTO TEMP compat_select_into_rows")
            .await;
        let row = if creation.is_ok() {
            Some(
                client
                    .query_one("SELECT id FROM compat_select_into_rows", &[])
                    .await,
            )
        } else {
            None
        };

        // Assert
        assert!(
            creation.is_ok(),
            "SELECT INTO TEMP should create the session-local relation: {creation:?}"
        );
        assert_eq!(
            row.expect("successful SELECT INTO should create a queryable relation")
                .expect("read the materialized SELECT INTO row")
                .get::<_, i32>(0),
            18
        );

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_create_a_temporary_table_from_a_query_as_relation_form() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("create_temp_table_as").await;
        let (client, connection) = server.connect().await;

        // Act
        let creation = client
            .batch_execute(
                "CREATE TEMP TABLE compat_temp_as_rows AS \
                 SELECT 18 AS id, 'materialized'::TEXT AS label",
            )
            .await;
        let rows = if creation.is_ok() {
            Some(
                client
                    .query("SELECT id, label FROM compat_temp_as_rows ORDER BY id", &[])
                    .await,
            )
        } else {
            None
        };

        // Assert
        assert!(
            creation.is_ok(),
            "CREATE TEMP TABLE AS should materialize its query: {creation:?}"
        );
        let rows = rows
            .expect("successful CREATE TABLE AS should create a queryable relation")
            .expect("read rows materialized by CREATE TEMP TABLE AS");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].get::<_, i32>(0), 18);
        assert_eq!(rows[0].get::<_, String>(1), "materialized");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_remove_temporary_table_indexes_when_the_owner_session_disconnects() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("temporary_index_disconnect_lifecycle").await;
        let (owner, owner_connection) = server.connect().await;
        let (observer, observer_connection) = server.connect().await;
        owner
            .batch_execute(
                "CREATE TEMP TABLE compat_temp_index_rows (id INTEGER NOT NULL); \
                 CREATE INDEX compat_temp_index_rows_id_idx \
                 ON compat_temp_index_rows (id)",
            )
            .await
            .expect("create a temporary table and its index");
        let index_exists: bool = owner
            .query_one(
                "SELECT EXISTS (SELECT 1 FROM pg_catalog.pg_class \
                 WHERE relname = 'compat_temp_index_rows_id_idx' AND relkind = 'i')",
                &[],
            )
            .await
            .expect("read the temporary index catalog entry")
            .get(0);

        // Act
        drop(owner);
        owner_connection.abort();
        let _ = owner_connection.await;
        let remaining_objects: i64 = observer
            .query_one(
                "SELECT COUNT(*) FROM pg_catalog.pg_class \
                 WHERE relname IN \
                 ('compat_temp_index_rows', 'compat_temp_index_rows_id_idx')",
                &[],
            )
            .await
            .expect("check that disconnect released temporary catalog entries")
            .get(0);

        // Assert
        assert!(
            index_exists,
            "the temporary index must be registered in pg_class"
        );
        assert_eq!(
            remaining_objects, 0,
            "the temporary table and its index must be removed when the owner disconnects"
        );

        drop(observer);
        server.shutdown(observer_connection).await;
    });
}
