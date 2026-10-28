#[test]
fn should_execute_sql_level_prepared_statements_then_deallocate_them() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("sql_prepared_lifecycle").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_sql_prepared (id INT PRIMARY KEY, label TEXT)",
                "INSERT INTO compat_sql_prepared VALUES (1, 'alpha'), (2, 'beta')",
                "PREPARE compat_find_label(INT) AS \
                   SELECT label FROM compat_sql_prepared WHERE id = $1",
            ],
        )
        .await
        .expect("SQL PREPARE should register a statement");

        // Act
        let first = client
            .query_one("EXECUTE compat_find_label(1)", &[])
            .await
            .expect("execute prepared SQL for the first input");
        let second = client
            .query_one("EXECUTE compat_find_label(2)", &[])
            .await
            .expect("execute prepared SQL for the second input");
        client
            .batch_execute("DEALLOCATE compat_find_label")
            .await
            .expect("deallocate SQL statement");
        let missing = client
            .query_one("EXECUTE compat_find_label(1)", &[])
            .await
            .expect_err("deallocated statement must not execute");

        // Assert
        let first_label: String = first.try_get(0).expect("first label");
        let second_label: String = second.try_get(0).expect("second label");
        assert_eq!(first_label, "alpha");
        assert_eq!(second_label, "beta");
        assert_eq!(db_error(&missing).code().code(), "26000");

        drop(client);
        server.shutdown(connection).await;
    });
}
#[test]
fn should_clear_extended_prepared_statement_with_deallocate_all() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("deallocate_all_prepared").await;
        let (client, connection) = server.connect().await;
        let prepared = client
            .prepare("SELECT $1::INT")
            .await
            .expect("prepare a statement through the extended query protocol");
        let before: i32 = client
            .query_one(&prepared, &[&41_i32])
            .await
            .expect("execute the named prepared statement before deallocation")
            .try_get(0)
            .expect("prepared statement value");

        // Act
        client
            .batch_execute("DEALLOCATE ALL")
            .await
            .expect("deallocate every SQL and extended-protocol prepared statement");
        let missing = client
            .query_one(&prepared, &[&41_i32])
            .await
            .expect_err("DEALLOCATE ALL removes the extended-protocol statement");

        // Assert
        assert_eq!(before, 41);
        assert_eq!(db_error(&missing).code().code(), "26000");

        drop(client);
        server.shutdown(connection).await;
    });
}
#[test]
fn should_keep_sql_prepared_statement_after_reset_all() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("prepared_statement_reset_all").await;
        let (client, connection) = server.connect().await;
        let startup_application_name: String = client
            .query_one("SHOW application_name", &[])
            .await
            .expect("read the session startup application name")
            .get(0);
        client
            .batch_execute(
                "PREPARE compat_reset_preserves_prepared(INT) AS SELECT $1 + 1; \
                 SET application_name = 'before-reset'",
            )
            .await
            .expect("prepare SQL and set a session value");

        // Act
        client
            .batch_execute("RESET ALL")
            .await
            .expect("reset session configuration without deallocating SQL PREPARE");
        let result = client
            .query_one("EXECUTE compat_reset_preserves_prepared(41)", &[])
            .await
            .expect("execute the prepared statement after RESET ALL");
        let application_name: String = client
            .query_one("SHOW application_name", &[])
            .await
            .expect("read reset application_name")
            .try_get(0)
            .expect("application_name text");

        // Assert
        assert_eq!(result.get::<_, i32>(0), 42);
        assert_eq!(application_name, startup_application_name);

        drop(client);
        server.shutdown(connection).await;
    });
}
#[test]
fn should_discard_session_settings_clear_wire_prepared_state() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("discard_session_state").await;
        let (client, connection) = server.connect().await;
        execute_statements(&client, &["SET application_name = 'discard-me'"])
            .await
            .expect("set session state before reset");
        let prepared = client
            .prepare("SELECT $1::INT")
            .await
            .expect("prepare wire statement before reset");

        // Act
        client
            .batch_execute("DISCARD ALL")
            .await
            .expect("discard all session state");
        let application_name: String = client
            .query_one("SHOW application_name", &[])
            .await
            .expect("read reset application name")
            .try_get(0)
            .expect("application name");
        let missing = client
            .query_one(&prepared, &[&1_i32])
            .await
            .expect_err("DISCARD ALL must clear wire prepared statements");

        // Assert
        assert_eq!(application_name, "");
        assert_eq!(db_error(&missing).code().code(), "26000");

        drop(client);
        server.shutdown(connection).await;
    });
}
#[test]
fn should_reset_all_session_settings_to_their_startup_values() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("reset_all_settings").await;
        let (client, connection) = server.connect().await;
        let startup_application_name: String = client
            .query_one("SHOW application_name", &[])
            .await
            .expect("read startup application_name")
            .try_get(0)
            .expect("application_name text");
        client
            .batch_execute("SET application_name = 'temporary-session-value'")
            .await
            .expect("change a session setting before reset");

        // Act
        client
            .batch_execute("RESET ALL")
            .await
            .expect("reset all configurable session settings");
        let reset_application_name: String = client
            .query_one("SHOW application_name", &[])
            .await
            .expect("read application_name after reset")
            .try_get(0)
            .expect("application_name text");

        // Assert
        assert_eq!(reset_application_name, startup_application_name);

        drop(client);
        server.shutdown(connection).await;
    });
}
#[test]
fn should_complete_sql_cursor_lifecycle_fetch_move_close() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("cursor_lifecycle").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_cursor_rows (id INT PRIMARY KEY)",
                "INSERT INTO compat_cursor_rows VALUES (1), (2), (3)",
                "BEGIN",
                "DECLARE compat_cursor NO SCROLL CURSOR FOR \
                   SELECT id FROM compat_cursor_rows ORDER BY id",
            ],
        )
        .await
        .expect("declare cursor over ordered fixture rows");

        // Act
        let first_page = client
            .query("FETCH FORWARD 1 FROM compat_cursor", &[])
            .await
            .expect("fetch first cursor row");
        client
            .batch_execute("MOVE FORWARD 1 FROM compat_cursor")
            .await
            .expect("move cursor over one row");
        let last_page = client
            .query("FETCH FORWARD 1 FROM compat_cursor", &[])
            .await
            .expect("fetch row after move");
        client
            .batch_execute("CLOSE compat_cursor")
            .await
            .expect("close cursor");
        client
            .batch_execute("COMMIT")
            .await
            .expect("commit cursor transaction");
        let closed = client
            .query("FETCH FORWARD 1 FROM compat_cursor", &[])
            .await
            .expect_err("closed cursor must not fetch");

        // Assert
        let first_ids = first_page
            .iter()
            .map(|row| row.get::<_, i32>(0))
            .collect::<Vec<_>>();
        let last_ids = last_page
            .iter()
            .map(|row| row.get::<_, i32>(0))
            .collect::<Vec<_>>();
        assert_eq!(first_ids, vec![1]);
        assert_eq!(last_ids, vec![3]);
        assert_eq!(db_error(&closed).code().code(), "34000");

        drop(client);
        server.shutdown(connection).await;
    });
}
#[test]
fn should_keep_a_holdable_cursor_open_after_commit() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("holdable_cursor_commit").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE compat_holdable_rows (id INT PRIMARY KEY)")
            .await
            .expect("create holdable cursor fixture table");
        client
            .batch_execute("INSERT INTO compat_holdable_rows VALUES (1), (2), (3)")
            .await
            .expect("seed holdable cursor fixture rows");
        client
            .batch_execute("BEGIN")
            .await
            .expect("begin holdable cursor transaction");
        client
            .batch_execute(
                "DECLARE compat_holdable CURSOR WITH HOLD FOR \
                 SELECT id FROM compat_holdable_rows ORDER BY id",
            )
            .await
            .expect("declare holdable cursor inside a transaction");
        let first_page = client
            .query("FETCH FORWARD 1 FROM compat_holdable", &[])
            .await
            .expect("fetch before commit");

        // Act
        client
            .batch_execute("COMMIT")
            .await
            .expect("commit while preserving a holdable cursor");
        let remaining_page = client
            .query("FETCH FORWARD 2 FROM compat_holdable", &[])
            .await
            .expect("fetch remaining cursor rows after commit");
        client
            .batch_execute("CLOSE compat_holdable")
            .await
            .expect("close holdable cursor after reading it");

        // Assert
        assert_eq!(
            first_page
                .iter()
                .map(|row| row.get::<_, i32>(0))
                .collect::<Vec<_>>(),
            vec![1]
        );
        assert_eq!(
            remaining_page
                .iter()
                .map(|row| row.get::<_, i32>(0))
                .collect::<Vec<_>>(),
            vec![2, 3]
        );

        drop(client);
        server.shutdown(connection).await;
    });
}
#[test]
fn should_close_all_after_scroll_cursor_navigation() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("scroll_cursor_navigation").await;
        let (client, connection) = server.connect().await;
        client
            .execute("CREATE TABLE compat_scroll_cursor_rows (n INT)", &[])
            .await
            .expect("create scroll cursor input rows");
        client
            .execute(
                "INSERT INTO compat_scroll_cursor_rows VALUES (1), (2), (3)",
                &[],
            )
            .await
            .expect("insert scroll cursor input rows");
        client
            .batch_execute(
                "BEGIN; DECLARE compat_scroll_cursor SCROLL CURSOR FOR \
                 SELECT n FROM compat_scroll_cursor_rows ORDER BY n",
            )
            .await
            .expect("declare a scrollable cursor over ordered rows");
        let metadata = client
            .query_one(
                "SELECT is_scrollable, is_holdable, is_binary FROM pg_cursors \
                 WHERE name = 'compat_scroll_cursor'",
                &[],
            )
            .await
            .expect("inspect the open cursor in pg_cursors");

        // Act
        let first: i32 = client
            .query_one("FETCH FIRST FROM compat_scroll_cursor", &[])
            .await
            .expect("fetch the first row")
            .get(0);
        let next: i32 = client
            .query_one("FETCH NEXT FROM compat_scroll_cursor", &[])
            .await
            .expect("fetch the next row")
            .get(0);
        let absolute: i32 = client
            .query_one("FETCH ABSOLUTE 3 FROM compat_scroll_cursor", &[])
            .await
            .expect("fetch an absolute row")
            .get(0);
        let prior: i32 = client
            .query_one("FETCH PRIOR FROM compat_scroll_cursor", &[])
            .await
            .expect("fetch the prior row")
            .get(0);
        let relative: i32 = client
            .query_one("FETCH RELATIVE -1 FROM compat_scroll_cursor", &[])
            .await
            .expect("fetch relative to the current cursor position")
            .get(0);
        client
            .batch_execute("MOVE FORWARD ALL FROM compat_scroll_cursor")
            .await
            .expect("move to the end of the scroll cursor");
        let backward: i32 = client
            .query_one("FETCH BACKWARD 1 FROM compat_scroll_cursor", &[])
            .await
            .expect("fetch backward from the end of the cursor")
            .get(0);
        client
            .batch_execute("CLOSE ALL")
            .await
            .expect("close every open cursor in the session");
        let remaining: i64 = client
            .query_one(
                "SELECT count(*) FROM pg_cursors WHERE name = 'compat_scroll_cursor'",
                &[],
            )
            .await
            .expect("check that CLOSE ALL removed the cursor")
            .get(0);
        client
            .batch_execute("COMMIT")
            .await
            .expect("finish the cursor transaction");

        // Assert
        assert!(metadata.get::<_, bool>(0), "SCROLL cursor is scrollable");
        assert!(!metadata.get::<_, bool>(1), "cursor is not holdable");
        assert!(!metadata.get::<_, bool>(2), "cursor is not binary");
        assert_eq!(
            (first, next, absolute, prior, relative, backward),
            (1, 2, 3, 2, 1, 3)
        );
        assert_eq!(remaining, 0);

        drop(client);
        server.shutdown(connection).await;
    });
}
#[test]
fn should_discard_sequence_session_state_without_rewinding_the_sequence() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("discard_sequence_state").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE SEQUENCE compat_discard_sequence")
            .await
            .expect("create a sequence for session-state reset");
        let first: i64 = client
            .query_one("SELECT nextval('compat_discard_sequence')", &[])
            .await
            .expect("read the first sequence value")
            .try_get(0)
            .expect("first sequence value");

        // Act
        client
            .batch_execute("DISCARD SEQUENCES")
            .await
            .expect("discard this session's sequence state");
        let forgotten = client
            .query_one("SELECT currval('compat_discard_sequence')", &[])
            .await
            .expect_err("currval must be unset after DISCARD SEQUENCES");
        let next: i64 = client
            .query_one("SELECT nextval('compat_discard_sequence')", &[])
            .await
            .expect("continue allocating from the sequence")
            .try_get(0)
            .expect("next sequence value");

        // Assert
        assert_eq!(first, 1);
        assert_eq!(db_error(&forgotten).code().code(), "55000");
        assert_eq!(next, 2);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_keep_extended_prepared_statement_after_discard_plans() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("discard_prepared_plans").await;
        let (client, connection) = server.connect().await;
        let statement = client
            .prepare("SELECT 7")
            .await
            .expect("prepare the extended-protocol plan-cache fixture");
        let before_discard: i32 = client
            .query_one(&statement, &[])
            .await
            .expect("execute the prepared statement before discard")
            .try_get(0)
            .expect("first result");

        // Act
        client
            .batch_execute("DISCARD PLANS")
            .await
            .expect("discard cached plans without deallocating statements");
        let after_discard: i32 = client
            .query_one(&statement, &[])
            .await
            .expect("execute the retained prepared statement after discard")
            .try_get(0)
            .expect("second result");

        // Assert
        assert_eq!(before_discard, 7);
        assert_eq!(after_discard, 7);

        drop(client);
        server.shutdown(connection).await;
    });
}
