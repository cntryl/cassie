#[test]
fn should_return_multiple_named_columns_from_a_sql_table_function() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("sql_table_function_multiple_rows").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute(
                "CREATE FUNCTION compat_table_rows() RETURNS TABLE(item_id INT, label TEXT) \
                   LANGUAGE SQL AS $$ \
                   SELECT 2::INT, 'second'::TEXT \
                   UNION ALL SELECT 1::INT, 'first'::TEXT $$",
            )
            .await
            .expect("create a SQL table function with two output columns and rows");

        // Act
        let rows = client
            .query(
                "SELECT item_id, label FROM compat_table_rows() ORDER BY item_id",
                &[],
            )
            .await
            .expect("read the complete TABLE result in deterministic order");

        // Assert
        assert_eq!(
            rows.iter()
                .map(|row| (row.get::<_, i32>(0), row.get::<_, String>(1)))
                .collect::<Vec<_>>(),
            vec![(1, "first".to_owned()), (2, "second".to_owned())]
        );

        drop(client);
        server.shutdown(connection).await;
    });
}
#[test]
fn should_fetch_a_bound_cursor_from_an_anonymous_block() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("do_bound_cursor").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_do_cursor_rows \
                   (id INT PRIMARY KEY, payload TEXT NOT NULL)",
                "CREATE TABLE compat_do_cursor_result (payload TEXT NOT NULL)",
                "INSERT INTO compat_do_cursor_rows VALUES \
                   (1, 'first'), (2, 'second')",
            ],
        )
        .await
        .expect("create and seed the cursor source and result tables");

        // Act
        client
            .batch_execute(
                "DO $block$ \
                 DECLARE target_id INT := 2; \
                         fetched_value TEXT; \
                         row_cursor CURSOR FOR \
                           SELECT payload FROM compat_do_cursor_rows \
                           WHERE id = target_id; \
                 BEGIN \
                   OPEN row_cursor; \
                   FETCH row_cursor INTO fetched_value; \
                   CLOSE row_cursor; \
                   INSERT INTO compat_do_cursor_result VALUES (fetched_value); \
                 END; \
                 $block$",
            )
            .await
            .expect("open, fetch and close the bound cursor in a DO block");
        let fetched_value: String = client
            .query_one("SELECT payload FROM compat_do_cursor_result", &[])
            .await
            .expect("read the cursor value copied by the block")
            .try_get(0)
            .expect("cursor payload");

        // Assert
        assert_eq!(fetched_value, "second");

        drop(client);
        server.shutdown(connection).await;
    });
}
#[test]
fn should_raise_no_data_found_for_strict_select_into_without_rows() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("plpgsql_select_into_strict_missing").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_strict_select_rows (id INT PRIMARY KEY)",
                "INSERT INTO compat_strict_select_rows VALUES (1)",
                "CREATE FUNCTION compat_strict_select_missing() RETURNS TEXT \
                   LANGUAGE plpgsql AS $$ \
                   DECLARE selected_id INT; \
                   BEGIN \
                     SELECT id INTO STRICT selected_id \
                       FROM compat_strict_select_rows WHERE id = 999; \
                     RETURN 'unexpected row'; \
                   EXCEPTION WHEN NO_DATA_FOUND THEN \
                     RETURN SQLSTATE; \
                   END; $$",
            ],
        )
        .await
        .expect("create a PL/pgSQL function that handles strict SELECT INTO with no rows");

        // Act
        let state: String = client
            .query_one("SELECT compat_strict_select_missing()", &[])
            .await
            .expect("read the SQLSTATE caught for strict SELECT INTO with no rows")
            .try_get(0)
            .expect("no-data-found SQLSTATE");

        // Assert
        assert_eq!(state, "P0002");

        drop(client);
        server.shutdown(connection).await;
    });
}
