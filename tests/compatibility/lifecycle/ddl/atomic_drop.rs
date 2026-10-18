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
