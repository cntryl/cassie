#[test]
fn should_resolve_sql_routine_defaults_named_arguments() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("routine_signatures").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute(
                "CREATE FUNCTION compat_signature_sum(left_value INT, right_value INT DEFAULT 2) \
                   RETURNS INT LANGUAGE SQL IMMUTABLE \
                   AS $$ SELECT left_value + right_value $$",
            )
            .await
            .expect("create SQL routine with a defaulted argument");

        // Act
        let defaulted = client
            .query_one("SELECT compat_signature_sum(40)", &[])
            .await
            .expect("call with defaulted right argument");
        let named = client
            .query_one(
                "SELECT compat_signature_sum(right_value => 4, left_value => 3)",
                &[],
            )
            .await
            .expect("call by named arguments in a different order");

        // Assert
        assert_eq!(defaulted.get::<_, i32>(0), 42);
        assert_eq!(named.get::<_, i32>(0), 7);

        drop(client);
        server.shutdown(connection).await;
    });
}
#[test]
fn should_return_table_rows_from_a_sql_function() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("table_returning_function").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute(
                "CREATE FUNCTION compat_table_result() RETURNS TABLE(total INT) \
                   LANGUAGE SQL IMMUTABLE AS $$ SELECT 42 $$",
            )
            .await
            .expect("create SQL function with a TABLE result");

        // Act
        let rows = client
            .query("SELECT total FROM compat_table_result()", &[])
            .await
            .expect("call table-returning SQL function");

        // Assert
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].get::<_, i32>(0), 42);

        drop(client);
        server.shutdown(connection).await;
    });
}
#[test]
fn should_execute_plpgsql_variables_with_branching_in_a_scalar_function() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("plpgsql_scalar_function").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute(
                "CREATE FUNCTION compat_plpgsql_clamp_increment(input_value BIGINT) \
                   RETURNS BIGINT LANGUAGE plpgsql AS $function$ \
                   DECLARE result_value BIGINT := input_value; \
                   BEGIN \
                     IF result_value < 0 THEN \
                       result_value := 0; \
                     ELSE \
                       result_value := result_value + 1; \
                     END IF; \
                     RETURN result_value; \
                   END; \
                   $function$",
            )
            .await
            .expect("create PL/pgSQL scalar function with a declaration and IF branch");

        // Act
        let rows = client
            .query(
                "SELECT compat_plpgsql_clamp_increment(4), \
                        compat_plpgsql_clamp_increment(-1)",
                &[],
            )
            .await
            .expect("execute the PL/pgSQL scalar function");

        // Assert
        assert_eq!(rows[0].get::<_, i64>(0), 5);
        assert_eq!(rows[0].get::<_, i64>(1), 0);

        drop(client);
        server.shutdown(connection).await;
    });
}
#[test]
fn should_return_each_loop_value_from_a_set_returning_plpgsql_function() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("plpgsql_set_returning_function").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute(
                "CREATE FUNCTION compat_plpgsql_series(bound INT) RETURNS SETOF INT \
                   LANGUAGE plpgsql AS $function$ \
                   DECLARE current_value INT := 1; \
                   BEGIN \
                     WHILE current_value <= bound LOOP \
                       RETURN NEXT current_value; \
                       current_value := current_value + 1; \
                     END LOOP; \
                     RETURN; \
                   END; \
                   $function$",
            )
            .await
            .expect("create PL/pgSQL set-returning function with a loop");

        // Act
        let rows = client
            .query("SELECT compat_plpgsql_series(3) AS value", &[])
            .await
            .expect("execute the PL/pgSQL set-returning function");

        // Assert
        assert_eq!(
            rows.iter()
                .map(|row| row.get::<_, i32>(0))
                .collect::<Vec<_>>(),
            vec![1, 2, 3]
        );

        drop(client);
        server.shutdown(connection).await;
    });
}
#[test]
fn should_set_select_into_targets_found_for_both_match_outcomes() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("plpgsql_select_into_found").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_select_into_rows (id INT PRIMARY KEY, stored_value INT)",
                "INSERT INTO compat_select_into_rows VALUES (1, 42)",
                "CREATE FUNCTION compat_select_into_state(input_id INT) \
                   RETURNS TEXT LANGUAGE plpgsql AS $function$ \
                   DECLARE matched_value INT; \
                           missing_value INT; \
                           matched_found BOOLEAN; \
                           missing_found BOOLEAN; \
                   BEGIN \
                     SELECT stored_value INTO matched_value \
                       FROM compat_select_into_rows WHERE id = input_id; \
                     matched_found := FOUND; \
                     SELECT stored_value INTO missing_value \
                       FROM compat_select_into_rows WHERE id = input_id + 1; \
                     missing_found := FOUND; \
                     RETURN matched_found::TEXT || ':' || COALESCE(matched_value::TEXT, 'NULL') || ',' || missing_found::TEXT || ':' || COALESCE(missing_value::TEXT, 'NULL'); \
                   END; \
                   $function$",
            ],
        )
        .await
        .expect("create PL/pgSQL function with matching and empty SELECT INTO queries");

        // Act
        let result: String = client
            .query_one("SELECT compat_select_into_state(1)", &[])
            .await
            .expect("execute SELECT INTO cases and return FOUND state")
            .try_get(0)
            .expect("SELECT INTO state");

        // Assert
        assert_eq!(result, "true:42,false:NULL");

        drop(client);
        server.shutdown(connection).await;
    });
}
#[test]
fn should_restore_routine_scoped_settings_after_function_return() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("routine_scoped_settings").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE FUNCTION compat_scoped_setting() RETURNS TEXT STABLE \
                   LANGUAGE SQL SET application_name = 'inside-function' \
                   AS $$ SELECT current_setting('application_name') $$",
                "SET application_name = 'outside-function'",
            ],
        )
        .await
        .expect("create routine with a scoped setting");

        // Act
        let during: String = client
            .query_one("SELECT compat_scoped_setting()", &[])
            .await
            .expect("call scoped-setting routine")
            .try_get(0)
            .expect("routine value");
        let after: String = client
            .query_one("SHOW application_name", &[])
            .await
            .expect("read caller setting after function return")
            .try_get(0)
            .expect("caller setting");

        // Assert
        assert_eq!(during, "inside-function");
        assert_eq!(after, "outside-function");

        drop(client);
        server.shutdown(connection).await;
    });
}
#[test]
fn should_store_dynamic_execute_into_result_from_a_do_block() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("do_dynamic_execute_into").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE compat_do_dynamic_result (value INT)")
            .await
            .expect("create result table");

        // Act
        client
            .batch_execute(
                "DO $block$ \
                 DECLARE target_id INT := 41; \
                         selected_value INT; \
                 BEGIN \
                   EXECUTE 'SELECT $1::integer + 1' INTO selected_value USING target_id; \
                   INSERT INTO compat_do_dynamic_result VALUES (selected_value); \
                 END; \
                 $block$",
            )
            .await
            .expect("execute dynamic SQL into a PL/pgSQL variable");
        let result: i32 = client
            .query_one("SELECT value FROM compat_do_dynamic_result", &[])
            .await
            .expect("read dynamic SQL result")
            .try_get(0)
            .expect("result value");

        // Assert
        assert_eq!(result, 42);

        drop(client);
        server.shutdown(connection).await;
    });
}
#[test]
fn should_return_query_rows_from_a_plpgsql_table_function() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("plpgsql_return_query").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute(
                "CREATE FUNCTION compat_plpgsql_query_rows(bound INT) \
                 RETURNS TABLE(value INT) LANGUAGE plpgsql AS $function$ \
                 BEGIN \
                   RETURN QUERY SELECT generated_value \
                   FROM generate_series(1, bound) AS generated_value; \
                 END; \
                 $function$",
            )
            .await
            .expect("create a PL/pgSQL function with RETURN QUERY");

        // Act
        let returned = client
            .query(
                "SELECT value FROM compat_plpgsql_query_rows(3) ORDER BY value",
                &[],
            )
            .await
            .expect("return rows from PL/pgSQL");

        // Assert
        assert_eq!(
            returned
                .iter()
                .map(|row| row.get::<_, i32>(0))
                .collect::<Vec<_>>(),
            vec![1, 2, 3]
        );

        drop(client);
        server.shutdown(connection).await;
    });
}
#[test]
fn should_report_plpgsql_perform_found_status_with_row_count() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("plpgsql_perform_diagnostics").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute(
                "CREATE FUNCTION compat_plpgsql_perform_status(should_find BOOLEAN) \
                 RETURNS TEXT LANGUAGE plpgsql AS $function$ \
                 DECLARE affected_rows BIGINT; \
                 BEGIN \
                   IF should_find THEN \
                     PERFORM 1; \
                   ELSE \
                     PERFORM 1 WHERE false; \
                   END IF; \
                   GET DIAGNOSTICS affected_rows = ROW_COUNT; \
                   RETURN FOUND::TEXT || ':' || affected_rows::TEXT; \
                 END; \
                 $function$",
            )
            .await
            .expect("create a PL/pgSQL PERFORM and GET DIAGNOSTICS function");

        // Act
        let diagnostics = client
            .query_one(
                "SELECT compat_plpgsql_perform_status(FALSE), \
                 compat_plpgsql_perform_status(TRUE)",
                &[],
            )
            .await
            .expect("read PERFORM, FOUND, and GET DIAGNOSTICS results");

        // Assert
        assert_eq!(diagnostics.get::<_, String>(0), "false:0");
        assert_eq!(diagnostics.get::<_, String>(1), "true:1");

        drop(client);
        server.shutdown(connection).await;
    });
}
#[test]
fn should_keep_prior_do_block_work_committed_after_a_later_exception() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("do_block_transaction_control").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE compat_do_commit_rows (id INT PRIMARY KEY)")
            .await
            .expect("create the transaction-control probe table");

        // Act
        let after_commit_error = client
            .batch_execute(
                "DO $block$ \
                   BEGIN \
                     INSERT INTO compat_do_commit_rows VALUES (1); \
                     COMMIT; \
                     RAISE EXCEPTION 'after commit'; \
                   END; \
                 $block$",
            )
            .await
            .expect_err("the DO block should raise after committing its first action");
        let committed_rows = client
            .query("SELECT id FROM compat_do_commit_rows ORDER BY id", &[])
            .await
            .expect("read rows after the DO block error");

        // Assert
        assert_eq!(db_error(&after_commit_error).code().code(), "P0001");
        assert_eq!(committed_rows.len(), 1);
        assert_eq!(committed_rows[0].get::<_, i32>(0), 1);

        drop(client);
        server.shutdown(connection).await;
    });
}
