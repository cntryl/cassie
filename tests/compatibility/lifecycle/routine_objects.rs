use super::*;

mod default_privileges;
mod expression_statistics;

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
fn should_rollback_exception_block_effects_return_from_the_handler() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("plpgsql_exception_subtransaction").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_plpgsql_exception_rows (id INT PRIMARY KEY)",
                "CREATE FUNCTION compat_plpgsql_catch_raise(input_id INT) RETURNS INT \
                   LANGUAGE plpgsql AS $function$ \
                   BEGIN \
                     BEGIN \
                       INSERT INTO compat_plpgsql_exception_rows VALUES (input_id); \
                       RAISE EXCEPTION 'rollback this block'; \
                     EXCEPTION WHEN raise_exception THEN \
                       RETURN 7; \
                     END; \
                   END; \
                   $function$",
            ],
        )
        .await
        .expect("create PL/pgSQL function with an exception subblock");

        // Act
        let returned: i32 = client
            .query_one("SELECT compat_plpgsql_catch_raise(4)", &[])
            .await
            .expect("invoke the PL/pgSQL exception handler")
            .try_get(0)
            .expect("handler return value");
        let rows = client
            .query("SELECT id FROM compat_plpgsql_exception_rows", &[])
            .await
            .expect("read rows after the handled exception");

        // Assert
        assert_eq!(returned, 7);
        assert!(
            rows.is_empty(),
            "the exception block must roll back its insert"
        );

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
fn should_execute_parameterized_dynamic_sql_in_a_do_block() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("do_dynamic_sql").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE compat_do_rows (id INT PRIMARY KEY)")
            .await
            .expect("create DO target");

        // Act
        client
            .batch_execute(
                "DO $block$ \
                 DECLARE target_id INT := 7; \
                 BEGIN \
                   EXECUTE 'INSERT INTO compat_do_rows VALUES ($1)' USING target_id; \
                 END; \
                 $block$",
            )
            .await
            .expect("execute parameterized dynamic SQL");
        let ids = client
            .query("SELECT id FROM compat_do_rows ORDER BY id", &[])
            .await
            .expect("read dynamic SQL result");

        // Assert
        assert_eq!(ids.len(), 1);
        assert_eq!(ids[0].get::<_, i32>(0), 7);

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
fn should_run_row_triggers_rollback_trigger_effects_with_the_statement() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("row_trigger_atomicity").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_trigger_source (id INT PRIMARY KEY)",
                "CREATE TABLE compat_trigger_log (id INT)",
                "CREATE FUNCTION compat_capture_insert() RETURNS TRIGGER \
                   LANGUAGE plpgsql AS $$ \
                   BEGIN \
                     INSERT INTO compat_trigger_log VALUES (NEW.id); \
                     RETURN NEW; \
                   END; $$",
                "CREATE TRIGGER compat_capture_insert_trigger \
                   AFTER INSERT ON compat_trigger_source FOR EACH ROW \
                   EXECUTE FUNCTION compat_capture_insert()",
            ],
        )
        .await
        .expect("create row trigger and function");

        // Act
        client
            .execute("INSERT INTO compat_trigger_source VALUES (9)", &[])
            .await
            .expect("insert row and run trigger");
        execute_statements(
            &client,
            &[
                "BEGIN",
                "INSERT INTO compat_trigger_source VALUES (10)",
                "ROLLBACK",
            ],
        )
        .await
        .expect("rollback row and its trigger effect together");
        let logged = client
            .query("SELECT id FROM compat_trigger_log ORDER BY id", &[])
            .await
            .expect("read trigger effects after rollback");

        // Assert
        assert_eq!(
            logged
                .iter()
                .map(|row| row.get::<_, i32>(0))
                .collect::<Vec<_>>(),
            vec![9],
            "rolling back a triggered insert also rolls back its log row"
        );

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_enforce_row_level_security_for_a_set_role_reader() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("row_level_security").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE ROLE compat_rls_reader NOLOGIN",
                "CREATE TABLE compat_rls_rows (id INT PRIMARY KEY, owner_name TEXT NOT NULL)",
                "INSERT INTO compat_rls_rows VALUES (1, 'compat_rls_reader'), (2, 'other')",
                "ALTER TABLE compat_rls_rows ENABLE ROW LEVEL SECURITY",
                "CREATE POLICY compat_rls_owner_policy ON compat_rls_rows \
                   FOR SELECT TO compat_rls_reader USING (owner_name = current_user)",
                "GRANT SELECT ON compat_rls_rows TO compat_rls_reader",
                "SET ROLE compat_rls_reader",
            ],
        )
        .await
        .expect("create and assume a row-scoped reader role");

        // Act
        let rows = client
            .query("SELECT id FROM compat_rls_rows ORDER BY id", &[])
            .await
            .expect("query through row security");
        client
            .batch_execute("RESET ROLE")
            .await
            .expect("restore session role");

        // Assert
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].get::<_, i32>(0), 1);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_persist_table_column_comments_in_catalogs_after_restart() {
    // Arrange
    support::use_local_storage();
    let path = support::data_dir("object-comments-restart");
    let _directory = DataDirectory(path.clone());
    let expected = vec![vec![
        cassie::types::Value::String("table documentation".into()),
        cassie::types::Value::String("column documentation".into()),
    ]];
    {
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie instance");
        cassie.startup().expect("start Cassie instance");
        let session = cassie.create_session("tester", None);
        for sql in [
            "CREATE TABLE compat_comment_rows (id INT, label TEXT)",
            "COMMENT ON TABLE compat_comment_rows IS 'table documentation'",
            "COMMENT ON COLUMN compat_comment_rows.label IS 'column documentation'",
        ] {
            cassie
                .execute_sql(&session, sql, vec![])
                .unwrap_or_else(|error| panic!("could not execute {sql}: {error}"));
        }
        let comments = cassie
            .execute_sql(
                &session,
                "SELECT obj_description(c.oid, 'pg_class'), \
                 col_description(c.oid, a.attnum) \
                 FROM pg_catalog.pg_class AS c \
                 JOIN pg_catalog.pg_attribute AS a ON a.attrelid = c.oid \
                 WHERE c.relname = 'compat_comment_rows' \
                   AND a.attname = 'label' AND a.attnum > 0",
                vec![],
            )
            .expect("read comments from catalogs")
            .rows()
            .to_vec();
        assert_eq!(comments, expected);
        drop(session);
        drop(cassie);
    }

    // Act
    let cassie = Cassie::new_with_data_dir(&path).expect("reopen Cassie instance");
    cassie.startup().expect("recover persisted comments");
    let session = cassie.create_session("tester", None);
    let comments = cassie
        .execute_sql(
            &session,
            "SELECT obj_description(c.oid, 'pg_class'), \
             col_description(c.oid, a.attnum) \
             FROM pg_catalog.pg_class AS c \
             JOIN pg_catalog.pg_attribute AS a ON a.attrelid = c.oid \
             WHERE c.relname = 'compat_comment_rows' \
               AND a.attname = 'label' AND a.attnum > 0",
            vec![],
        )
        .expect("read comments after restart")
        .rows()
        .to_vec();

    // Assert
    assert_eq!(comments, expected);
}

#[test]
fn should_collect_extended_statistics_restore_them_after_restart() {
    // Arrange
    support::use_local_storage();
    let path = support::data_dir("extended-statistics-restart");
    let _directory = DataDirectory(path.clone());
    let expected = vec![vec![
        cassie::types::Value::String("compat_extended_stats".into()),
        cassie::types::Value::Bool(true),
        cassie::types::Value::Bool(true),
        cassie::types::Value::Bool(true),
    ]];
    {
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie instance");
        cassie.startup().expect("start Cassie instance");
        let session = cassie.create_session("tester", None);
        for sql in [
            "CREATE TABLE compat_stats_rows (group_id INT, flag INT)",
            "INSERT INTO compat_stats_rows VALUES \
             (1,10),(1,10),(1,10),(1,10),(2,20),(2,20),(2,20),(2,20), \
             (3,30),(3,30),(3,30),(3,30),(4,40),(4,40),(4,40),(4,40), \
             (5,50),(5,50),(5,50),(5,50)",
            "CREATE STATISTICS compat_extended_stats (ndistinct, dependencies, mcv) \
             ON group_id, flag FROM compat_stats_rows",
            "ANALYZE compat_stats_rows",
        ] {
            cassie
                .execute_sql(&session, sql, vec![])
                .unwrap_or_else(|error| panic!("could not execute {sql}: {error}"));
        }
        drop(session);
        drop(cassie);
    }

    // Act
    let cassie = Cassie::new_with_data_dir(&path).expect("reopen Cassie instance");
    cassie.startup().expect("recover collected statistics");
    let session = cassie.create_session("tester", None);
    let statistics = cassie
        .execute_sql(
            &session,
            "SELECT s.stxname, d.stxdndistinct IS NOT NULL, \
             d.stxddependencies IS NOT NULL, d.stxdmcv IS NOT NULL \
             FROM pg_catalog.pg_statistic_ext AS s \
             JOIN pg_catalog.pg_statistic_ext_data AS d ON d.stxoid = s.oid \
             WHERE s.stxname = 'compat_extended_stats'",
            vec![],
        )
        .expect("read persisted extended-statistics data")
        .rows()
        .to_vec();

    // Assert
    assert_eq!(statistics, expected);
}

#[test]
fn should_hide_rolled_back_extended_statistics_from_other_sessions() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("extended_statistics_rollback").await;
        let (owner, owner_connection) = server.connect().await;
        let (observer, observer_connection) = server.connect().await;
        owner
            .batch_execute(
                "CREATE TABLE compat_stats_rollback_rows (left_value INT, right_value INT)",
            )
            .await
            .expect("create the statistics relation");

        // Act
        owner
            .batch_execute("BEGIN")
            .await
            .expect("begin a statistics transaction");
        owner
            .batch_execute(
                "CREATE STATISTICS compat_stats_rollback (dependencies) \
                 ON left_value, right_value FROM compat_stats_rollback_rows",
            )
            .await
            .expect("create extended statistics transactionally");
        let owner_count: i64 = owner
            .query_one(
                "SELECT count(*) FROM pg_catalog.pg_statistic_ext \
                 WHERE stxname = 'compat_stats_rollback'",
                &[],
            )
            .await
            .expect("read the owner's uncommitted statistics")
            .try_get(0)
            .expect("owner count");
        let observer_before: i64 = observer
            .query_one(
                "SELECT count(*) FROM pg_catalog.pg_statistic_ext \
                 WHERE stxname = 'compat_stats_rollback'",
                &[],
            )
            .await
            .expect("read statistics from an independent session")
            .try_get(0)
            .expect("observer count before rollback");
        owner
            .batch_execute("ROLLBACK")
            .await
            .expect("roll back statistics DDL");
        let observer_after: i64 = observer
            .query_one(
                "SELECT count(*) FROM pg_catalog.pg_statistic_ext \
                 WHERE stxname = 'compat_stats_rollback'",
                &[],
            )
            .await
            .expect("verify rollback removed the catalog row")
            .try_get(0)
            .expect("observer count after rollback");

        // Assert
        assert_eq!(owner_count, 1);
        assert_eq!(observer_before, 0);
        assert_eq!(observer_after, 0);

        drop(owner);
        drop(observer);
        server.shutdown(owner_connection).await;
        observer_connection.abort();
        let _ = observer_connection.await;
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
fn should_read_then_clear_a_schema_comment() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("schema_routine_comments").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE SCHEMA compat_comment_scope")
            .await
            .expect("create schema for comment lookup");
        client
            .batch_execute("COMMENT ON SCHEMA compat_comment_scope IS 'schema documentation'")
            .await
            .expect("annotate schema");

        // Act
        let comments = client
            .query_one(
                "SELECT obj_description(oid, 'pg_namespace') \
                 FROM pg_catalog.pg_namespace WHERE nspname = 'compat_comment_scope'",
                &[],
            )
            .await
            .expect("read schema comment");
        let comment = comments.try_get::<_, String>(0).expect("schema comment");
        client
            .batch_execute("COMMENT ON SCHEMA compat_comment_scope IS NULL")
            .await
            .expect("clear the schema comment");
        let schema_comment_cleared: bool = client
            .query_one(
                "SELECT obj_description(oid, 'pg_namespace') IS NULL \
                 FROM pg_catalog.pg_namespace WHERE nspname = 'compat_comment_scope'",
                &[],
            )
            .await
            .expect("read cleared schema comment")
            .try_get(0)
            .expect("schema comment nullness");

        // Assert
        assert_eq!(comment, "schema documentation");
        assert!(schema_comment_cleared);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_roundtrip_sequence_comments_through_catalog() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("sequence_comments").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE SEQUENCE compat_comment_sequence")
            .await
            .expect("create a sequence to annotate");

        // Act
        client
            .batch_execute("COMMENT ON SEQUENCE compat_comment_sequence IS 'sequence note'")
            .await
            .expect("store an object comment on the sequence");
        let comment: Option<String> = client
            .query_one(
                "SELECT obj_description('compat_comment_sequence'::regclass, 'pg_class')",
                &[],
            )
            .await
            .expect("read the sequence comment from PostgreSQL object metadata")
            .try_get(0)
            .expect("sequence comment");

        // Assert
        assert_eq!(comment.as_deref(), Some("sequence note"));

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

#[test]
fn should_apply_row_trigger_when_predicate_only_to_matching_rows() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("row_trigger_when_filter").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_when_source (id INT PRIMARY KEY, capture BOOLEAN NOT NULL)",
                "CREATE TABLE compat_when_log (id INT)",
                "CREATE FUNCTION compat_when_capture() RETURNS TRIGGER \
                   LANGUAGE plpgsql AS $function$ \
                   BEGIN \
                     INSERT INTO compat_when_log VALUES (NEW.id); \
                     RETURN NEW; \
                   END; \
                   $function$",
                "CREATE TRIGGER compat_when_capture_insert \
                   AFTER INSERT ON compat_when_source FOR EACH ROW \
                   WHEN (NEW.capture) EXECUTE FUNCTION compat_when_capture()",
            ],
        )
        .await
        .expect("create WHEN-filtered trigger");

        // Act
        client
            .batch_execute("INSERT INTO compat_when_source VALUES (1, TRUE), (2, FALSE)")
            .await
            .expect("insert matching and nonmatching rows");
        let logged_ids = client
            .query("SELECT id FROM compat_when_log ORDER BY id", &[])
            .await
            .expect("read trigger effects");

        // Assert
        assert_eq!(logged_ids.len(), 1);
        assert_eq!(logged_ids[0].get::<_, i32>(0), 1);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_fire_statement_trigger_once_for_a_multirow_insert() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("statement_trigger_lifecycle").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_statement_trigger_rows (id INT)",
                "CREATE TABLE compat_statement_trigger_events (operation TEXT, rows_seen INT)",
                "CREATE FUNCTION compat_statement_trigger() RETURNS TRIGGER \
                   LANGUAGE plpgsql AS $function$ \
                   BEGIN \
                     INSERT INTO compat_statement_trigger_events \
                     VALUES (TG_OP, (SELECT count(*) FROM compat_statement_trigger_rows)); \
                     RETURN NULL; \
                   END; \
                   $function$",
                "CREATE TRIGGER compat_statement_after_insert \
                   AFTER INSERT ON compat_statement_trigger_rows \
                   FOR EACH STATEMENT EXECUTE FUNCTION compat_statement_trigger()",
            ],
        )
        .await
        .expect("create a statement-level trigger and its audit table");

        // Act
        client
            .batch_execute("INSERT INTO compat_statement_trigger_rows VALUES (1), (2), (3)")
            .await
            .expect("insert multiple rows in one statement");
        let events = client
            .query(
                "SELECT operation, rows_seen FROM compat_statement_trigger_events",
                &[],
            )
            .await
            .expect("read statement trigger effects");

        // Assert
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].get::<_, String>(0), "INSERT");
        assert_eq!(events[0].get::<_, i32>(1), 3);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_roundtrip_constraint_comments_through_catalog() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("constraint_comments").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute(
                "CREATE TABLE compat_constraint_comments ( \
                   id INT CONSTRAINT compat_constraint_comments_key UNIQUE)",
            )
            .await
            .expect("create a named constraint for comment metadata");

        // Act
        client
            .batch_execute(
                "COMMENT ON CONSTRAINT compat_constraint_comments_key \
                 ON compat_constraint_comments IS 'unique business key'",
            )
            .await
            .expect("set a comment on the named table constraint");
        let before_clear: Option<String> = client
            .query_one(
                "SELECT obj_description(oid, 'pg_constraint') \
                 FROM pg_constraint WHERE conname = 'compat_constraint_comments_key'",
                &[],
            )
            .await
            .expect("read the constraint comment")
            .try_get(0)
            .expect("comment value");
        client
            .batch_execute(
                "COMMENT ON CONSTRAINT compat_constraint_comments_key \
                 ON compat_constraint_comments IS NULL",
            )
            .await
            .expect("clear a constraint comment with IS NULL");
        let after_clear: Option<String> = client
            .query_one(
                "SELECT obj_description(oid, 'pg_constraint') \
                 FROM pg_constraint WHERE conname = 'compat_constraint_comments_key'",
                &[],
            )
            .await
            .expect("read the cleared constraint comment")
            .try_get(0)
            .expect("cleared comment value");

        // Assert
        assert_eq!(before_clear.as_deref(), Some("unique business key"));
        assert_eq!(after_clear, None);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_enforce_table_privileges_for_an_assumed_role() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("assumed_role_table_privileges").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE ROLE compat_assumed_reader NOLOGIN")
            .await
            .expect("create a role with limited table access");
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_assumed_privilege_rows (id INT PRIMARY KEY)",
                "INSERT INTO compat_assumed_privilege_rows VALUES (1)",
                "GRANT SELECT ON TABLE compat_assumed_privilege_rows TO compat_assumed_reader",
                "SET ROLE compat_assumed_reader",
            ],
        )
        .await
        .expect("grant read access and assume the grantee role");

        // Act
        let current_role: String = client
            .query_one("SELECT current_user", &[])
            .await
            .expect("read the active SQL role")
            .try_get(0)
            .expect("current role name");
        let visible: i32 = client
            .query_one("SELECT id FROM compat_assumed_privilege_rows", &[])
            .await
            .expect("read a row granted to the active role")
            .try_get(0)
            .expect("visible row id");
        let denied_write = client
            .execute("INSERT INTO compat_assumed_privilege_rows VALUES (2)", &[])
            .await
            .expect_err("a SELECT-only role cannot insert rows");
        client
            .batch_execute("RESET ROLE")
            .await
            .expect("restore the session's original role");

        // Assert
        assert_eq!(current_role, "compat_assumed_reader");
        assert_eq!(visible, 1);
        assert_eq!(db_error(&denied_write).code().code(), "42501");

        client
            .batch_execute("DROP TABLE compat_assumed_privilege_rows")
            .await
            .expect("drop the table privilege fixture");
        client
            .batch_execute("DROP ROLE compat_assumed_reader")
            .await
            .expect("drop the temporary privilege role");
        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_roundtrip_role_comments_through_shared_object_metadata() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("role_comments").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE ROLE compat_comment_role NOLOGIN")
            .await
            .expect("create a role to annotate");

        // Act
        client
            .batch_execute("COMMENT ON ROLE compat_comment_role IS 'role metadata'")
            .await
            .expect("store a comment on the shared role object");
        let before_clear: Option<String> = client
            .query_one(
                "SELECT shobj_description(oid, 'pg_authid') \
                 FROM pg_catalog.pg_authid WHERE rolname = 'compat_comment_role'",
                &[],
            )
            .await
            .expect("read the role comment from shared object metadata")
            .try_get(0)
            .expect("role comment");
        client
            .batch_execute("COMMENT ON ROLE compat_comment_role IS NULL")
            .await
            .expect("clear a role comment with IS NULL");
        let after_clear: Option<String> = client
            .query_one(
                "SELECT shobj_description(oid, 'pg_authid') \
                 FROM pg_catalog.pg_authid WHERE rolname = 'compat_comment_role'",
                &[],
            )
            .await
            .expect("read the cleared role comment")
            .try_get(0)
            .expect("cleared role comment");

        // Assert
        assert_eq!(before_clear.as_deref(), Some("role metadata"));
        assert_eq!(after_clear, None);

        client
            .batch_execute("DROP ROLE compat_comment_role")
            .await
            .expect("drop the annotated role");
        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_roundtrip_view_comment_through_relation_metadata() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("view_comments").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_view_comment_source (id INT)",
                "CREATE VIEW compat_comment_view AS \
                   SELECT id FROM compat_view_comment_source",
            ],
        )
        .await
        .expect("create a view to annotate");

        // Act
        let commented = client
            .batch_execute("COMMENT ON VIEW compat_comment_view IS 'view note'")
            .await;
        let comment = if commented.is_ok() {
            Some(
                client
                    .query_one(
                        "SELECT obj_description('compat_comment_view'::regclass, 'pg_class')",
                        &[],
                    )
                    .await,
            )
        } else {
            None
        };

        // Assert
        let comment_sqlstate = commented
            .as_ref()
            .err()
            .map(|error| db_error(error).code().code());
        drop(client);
        server.shutdown(connection).await;

        assert!(
            commented.is_ok(),
            "COMMENT ON VIEW should store relation metadata; SQLSTATE: {comment_sqlstate:?}"
        );
        let comment = comment
            .expect("successful COMMENT ON VIEW should allow catalog inspection")
            .expect("read view comment");
        assert_eq!(
            comment.get::<_, Option<String>>(0).as_deref(),
            Some("view note")
        );
    });
}

#[test]
fn should_store_database_comments_in_shared_object_metadata() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("database_comments").await;
        let (client, connection) = server.connect().await;

        // Act
        let commented = client
            .batch_execute("COMMENT ON DATABASE postgres IS 'database note'")
            .await;
        let comment = if commented.is_ok() {
            Some(
                client
                    .query_one(
                        "SELECT shobj_description(oid, 'pg_database') \
                         FROM pg_catalog.pg_database WHERE datname = 'postgres'",
                        &[],
                    )
                    .await,
            )
        } else {
            None
        };

        // Assert
        let comment_sqlstate = commented
            .as_ref()
            .err()
            .map(|error| db_error(error).code().code());
        drop(client);
        server.shutdown(connection).await;

        assert!(
            commented.is_ok(),
            "COMMENT ON DATABASE should store shared metadata; SQLSTATE: {comment_sqlstate:?}"
        );
        let comment = comment
            .expect("successful COMMENT ON DATABASE should allow catalog inspection")
            .expect("read database comment");
        assert_eq!(
            comment.get::<_, Option<String>>(0).as_deref(),
            Some("database note")
        );
    });
}
