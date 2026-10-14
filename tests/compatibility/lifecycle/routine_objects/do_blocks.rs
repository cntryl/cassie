use super::*;

#[test]
fn should_format_identifiers_in_dynamic_do_sql() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("do_dynamic_format").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE SCHEMA compat_do_dynamic_format")
            .await
            .expect("create the schema used by the search_path probe");
        client
            .batch_execute(
                r#"CREATE TABLE compat_do_dynamic_format."Quoted""Target" (
                       id INT PRIMARY KEY,
                       "Odd""Column" TEXT,
                       "Second Column" TEXT
                   )"#,
            )
            .await
            .expect("create a table with quoted identifiers");
        client
            .batch_execute("SET search_path TO compat_do_dynamic_format, public")
            .await
            .expect("select the schema for unqualified dynamic SQL");

        // Act
        client
            .batch_execute(
                r#"DO $outer$
                   DECLARE
                     target_table TEXT := 'Quoted"Target';
                     target_column TEXT := 'Odd"Column';
                     first_value TEXT := $payload$O'Reilly; SELECT 99; -- $tag$ remains literal$payload$;
                     second_value TEXT := $second$dollar $inner$; --$second$;
                   BEGIN
                     EXECUTE format(
                       'INSERT INTO %I (id, %I) VALUES (%L, %L)',
                       target_table,
                       target_column,
                       1,
                       first_value
                     );
                     EXECUTE format(
                       'UPDATE %I SET %I = %L WHERE id = %L',
                       target_table,
                       'Second Column',
                       second_value,
                       1
                     );
                   END;
                   $outer$;"#,
            )
            .await
            .expect("execute multiple formatted dynamic statements in a DO block");
        let row = client
            .query_one(
                r#"SELECT id, "Odd""Column", "Second Column"
                   FROM "Quoted""Target""#,
                &[],
            )
            .await
            .expect("read values inserted by dynamic SQL through search_path");

        // Assert
        assert_eq!(row.get::<_, i32>(0), 1);
        assert_eq!(
            row.get::<_, String>(1),
            "O'Reilly; SELECT 99; -- $tag$ remains literal"
        );
        assert_eq!(row.get::<_, String>(2), "dollar $inner$; --");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_roll_back_parameterized_dynamic_sql_after_a_do_block_exception() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("do_dynamic_sql_rollback").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute(
                "CREATE TABLE compat_do_rollback_rows (id INT PRIMARY KEY, label TEXT NOT NULL)",
            )
            .await
            .expect("create the dynamic-update target");
        client
            .batch_execute("INSERT INTO compat_do_rollback_rows VALUES (1, 'before')")
            .await
            .expect("insert the row whose update must be rolled back");

        // Act
        let error = client
            .batch_execute(
                r#"DO $block$
                   DECLARE
                     target_id INT := 1;
                     target_label TEXT := 'after';
                   BEGIN
                     EXECUTE 'UPDATE compat_do_rollback_rows SET label = $1 WHERE id = $2'
                       USING target_label, target_id;
                     RAISE EXCEPTION 'abort dynamic update';
                   END;
                   $block$;"#,
            )
            .await
            .expect_err("the DO block should raise after its dynamic update");
        let label: String = client
            .query_one(
                "SELECT label FROM compat_do_rollback_rows WHERE id = 1",
                &[],
            )
            .await
            .expect("read the row after the failed DO block")
            .try_get(0)
            .expect("row label");

        // Assert
        assert_eq!(db_error(&error).code().code(), "P0001");
        assert_eq!(label, "before");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_apply_invoker_privileges_to_dynamic_sql_in_a_do_block() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("do_dynamic_sql_invoker_privileges").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE ROLE compat_do_exec_reader NOLOGIN",
                "CREATE TABLE compat_do_exec_rows (id INT PRIMARY KEY)",
                "GRANT SELECT ON TABLE compat_do_exec_rows TO compat_do_exec_reader",
                "SET ROLE compat_do_exec_reader",
            ],
        )
        .await
        .expect("create a read-only role and assume it");

        // Act
        let error = client
            .batch_execute(
                r#"DO $block$
                   BEGIN
                     EXECUTE 'INSERT INTO compat_do_exec_rows VALUES (1)';
                   END;
                   $block$"#,
            )
            .await
            .expect_err("dynamic SQL in a DO block must use the invoker's table privileges");
        client
            .batch_execute("RESET ROLE")
            .await
            .expect("restore the administrative role after the denied statement");
        let rows = client
            .query("SELECT id FROM compat_do_exec_rows", &[])
            .await
            .expect("read the table after the rejected dynamic insert");

        // Assert
        assert_eq!(db_error(&error).code().code(), "42501");
        assert!(rows.is_empty(), "the denied insert must leave no row");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_not_partially_execute_a_do_block_with_invalid_embedded_sql() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("do_embedded_sql_syntax_atomicity").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE compat_do_embedded_syntax_rows (id INT PRIMARY KEY)")
            .await
            .expect("create the table used to observe partial execution");

        // Act
        let error = client
            .batch_execute(
                r#"DO $block$
                   BEGIN
                     INSERT INTO compat_do_embedded_syntax_rows VALUES (1);
                     PERFORM 1 + ;
                   END;
                   $block$"#,
            )
            .await
            .expect_err("invalid embedded SQL must reject the complete DO statement");
        let rows = client
            .query("SELECT id FROM compat_do_embedded_syntax_rows", &[])
            .await
            .expect("read the table after the rejected DO block");

        // Assert
        assert_eq!(db_error(&error).code().code(), "42601");
        assert!(
            rows.is_empty(),
            "the earlier insert must roll back with the DO block"
        );

        drop(client);
        server.shutdown(connection).await;
    });
}
