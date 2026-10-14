use super::*;

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
fn should_apply_security_definer_privileges_without_granting_table_access() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("security_definer_table_privilege").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE ROLE compat_definer_reader NOLOGIN",
                "CREATE TABLE compat_definer_secret (value TEXT)",
                "INSERT INTO compat_definer_secret VALUES ('sealed')",
                "CREATE FUNCTION compat_definer_read_secret() RETURNS TEXT \
                   LANGUAGE SQL SECURITY DEFINER AS $$ \
                   SELECT value FROM public.compat_definer_secret $$",
                "GRANT EXECUTE ON FUNCTION compat_definer_read_secret() \
                   TO compat_definer_reader",
                "SET ROLE compat_definer_reader",
            ],
        )
        .await
        .expect("create the protected table and a callable SECURITY DEFINER routine");

        // Act
        let result = client
            .query_one(
                "SELECT compat_definer_read_secret(), \
                        has_table_privilege(current_user, \
                          'public.compat_definer_secret', 'SELECT')",
                &[],
            )
            .await
            .expect("read the protected value through the definer routine");
        let direct_read_error = client
            .query("SELECT value FROM compat_definer_secret", &[])
            .await
            .expect_err("the caller must not read the protected table directly");
        client
            .batch_execute("RESET ROLE")
            .await
            .expect("restore the administrative role");

        // Assert
        assert_eq!(result.get::<_, String>(0), "sealed");
        assert!(!result.get::<_, bool>(1));
        assert_eq!(db_error(&direct_read_error).code().code(), "42501");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_restore_routine_scoped_settings_after_a_function_error() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("routine_scoped_setting_error").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE FUNCTION compat_scoped_divide_by_zero() RETURNS INT \
                   LANGUAGE SQL SET application_name = 'inside-failing-function' \
                   AS $$ SELECT 1 / 0 $$",
                "SET application_name = 'outside-failing-function'",
            ],
        )
        .await
        .expect("create a function whose scoped setting surrounds an error");

        // Act
        let error = client
            .query("SELECT compat_scoped_divide_by_zero()", &[])
            .await
            .expect_err("the function body should raise division by zero");
        let application_name: String = client
            .query_one("SHOW application_name", &[])
            .await
            .expect("read the caller setting after the function error")
            .try_get(0)
            .expect("caller application name");

        // Assert
        assert_eq!(db_error(&error).code().code(), "22012");
        assert_eq!(application_name, "outside-failing-function");

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
fn should_fire_update_when_trigger_only_for_changed_values() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("update_trigger_when_old_new").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_update_when_rows \
                   (id INT PRIMARY KEY, state TEXT NOT NULL)",
                "CREATE TABLE compat_update_when_log \
                   (id INT, old_state TEXT, new_state TEXT)",
                "CREATE FUNCTION compat_update_when_capture() RETURNS TRIGGER \
                   LANGUAGE plpgsql AS $$ \
                   BEGIN \
                     INSERT INTO compat_update_when_log \
                       VALUES (NEW.id, OLD.state, NEW.state); \
                     RETURN NEW; \
                   END; $$",
                "CREATE TRIGGER compat_update_when_capture \
                   AFTER UPDATE ON compat_update_when_rows FOR EACH ROW \
                   WHEN (OLD.state IS DISTINCT FROM NEW.state) \
                   EXECUTE FUNCTION compat_update_when_capture()",
                "INSERT INTO compat_update_when_rows VALUES \
                   (1, 'unchanged'), (2, 'before')",
            ],
        )
        .await
        .expect("create an AFTER UPDATE trigger filtered by OLD and NEW values");

        // Act
        client
            .batch_execute(
                "UPDATE compat_update_when_rows SET state = state WHERE id = 1; \
                 UPDATE compat_update_when_rows SET state = 'after' WHERE id = 2",
            )
            .await
            .expect("run one filtered no-op update and one changed update");
        let events = client
            .query(
                "SELECT id, old_state, new_state FROM compat_update_when_log ORDER BY id",
                &[],
            )
            .await
            .expect("read the AFTER trigger's OLD and NEW values");

        // Assert
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].get::<_, i32>(0), 2);
        assert_eq!(events[0].get::<_, String>(1), "before");
        assert_eq!(events[0].get::<_, String>(2), "after");

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
