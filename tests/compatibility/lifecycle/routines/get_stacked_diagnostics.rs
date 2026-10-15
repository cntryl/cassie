use super::*;

#[test]
fn should_return_stacked_diagnostics_for_caught_unique_violation() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("plpgsql_stacked_diagnostics").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat835_stacked_rows (\
                   id INT CONSTRAINT compat835_stacked_rows_pk PRIMARY KEY, payload TEXT)",
                "CREATE TABLE compat835_stacked_result (\
                   state TEXT, message TEXT, constraint_name TEXT, \
                   table_name TEXT, schema_name TEXT)",
                "INSERT INTO compat835_stacked_rows VALUES (1, 'original')",
            ],
        )
        .await
        .expect("create and seed the unique-violation diagnostic fixtures");

        // Act
        client
            .batch_execute(
                "DO $block$ \
                 DECLARE \
                   error_state TEXT; \
                   error_message TEXT; \
                   error_constraint TEXT; \
                   error_table TEXT; \
                   error_schema TEXT; \
                 BEGIN \
                   BEGIN \
                     INSERT INTO compat835_stacked_rows VALUES (1, 'duplicate'); \
                   EXCEPTION WHEN unique_violation THEN \
                     GET STACKED DIAGNOSTICS \
                       error_state = RETURNED_SQLSTATE, \
                       error_message = MESSAGE_TEXT, \
                       error_constraint = CONSTRAINT_NAME, \
                       error_table = TABLE_NAME, \
                       error_schema = SCHEMA_NAME; \
                   END; \
                   INSERT INTO compat835_stacked_result \
                     VALUES (error_state, error_message, error_constraint, \
                             error_table, error_schema); \
                 END; \
                 $block$",
            )
            .await
            .expect("capture the caught error through GET STACKED DIAGNOSTICS");
        let diagnostics = client
            .query_one(
                "SELECT state, message, constraint_name, table_name, schema_name \
                 FROM compat835_stacked_result",
                &[],
            )
            .await
            .expect("read the captured diagnostic fields");
        let source_rows: i64 = client
            .query_one("SELECT count(*) FROM compat835_stacked_rows", &[])
            .await
            .expect("count source rows after the handled unique violation")
            .get(0);

        // Assert
        assert_eq!(diagnostics.get::<_, String>(0), "23505");
        assert_eq!(
            diagnostics.get::<_, String>(1),
            "duplicate key value violates unique constraint \"compat835_stacked_rows_pk\""
        );
        assert_eq!(diagnostics.get::<_, String>(2), "compat835_stacked_rows_pk");
        assert_eq!(diagnostics.get::<_, String>(3), "compat835_stacked_rows");
        assert_eq!(diagnostics.get::<_, String>(4), "public");
        assert_eq!(
            source_rows, 1,
            "the caught duplicate insert must be rolled back"
        );

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_preserve_found_for_an_empty_dynamic_execute_result() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("plpgsql_dynamic_empty_result").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat835_dynamic_empty_source (id INT PRIMARY KEY)",
                "CREATE TABLE compat835_dynamic_empty_result (\
                   selected_id INT, found_after BOOL, affected_rows BIGINT)",
            ],
        )
        .await
        .expect("create an empty source and result table for dynamic EXECUTE INTO");

        // Act
        client
            .batch_execute(
                "DO $block$ \
                 DECLARE \
                   selected_id INT := 99; \
                   found_after BOOL; \
                   affected_rows BIGINT; \
                 BEGIN \
                   PERFORM 1; \
                   EXECUTE 'SELECT id FROM compat835_dynamic_empty_source WHERE id = $1' \
                     INTO selected_id USING 999; \
                   found_after := FOUND; \
                   GET DIAGNOSTICS affected_rows = ROW_COUNT; \
                   INSERT INTO compat835_dynamic_empty_result \
                     VALUES (selected_id, found_after, affected_rows); \
                 END; \
                 $block$",
            )
            .await
            .expect("execute a parameterized dynamic query that returns no rows");
        let result = client
            .query_one(
                "SELECT selected_id, found_after, affected_rows \
                 FROM compat835_dynamic_empty_result",
                &[],
            )
            .await
            .expect("read the dynamic INTO target and diagnostics");

        // Assert
        assert_eq!(result.get::<_, Option<i32>>(0), None);
        assert!(result.get::<_, bool>(1));
        assert_eq!(result.get::<_, i64>(2), 0);

        drop(client);
        server.shutdown(connection).await;
    });
}
