use super::*;

#[test]
fn should_report_missing_trigger_function_before_trigger_return_type_resolution() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("trigger_or_replace_multievent").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE compat_trigger_parse_rows (id INT)")
            .await
            .expect("create the table for trigger syntax resolution");

        // PostgreSQL checks the referenced function while processing the trigger declaration.
        // This statement is the only trigger DDL; no RETURNS TRIGGER routine is created.
        // Act
        let error = client
            .batch_execute(
                "CREATE OR REPLACE TRIGGER compat_missing_trigger_function \
                 BEFORE INSERT OR UPDATE OR DELETE ON compat_trigger_parse_rows \
                 FOR EACH ROW EXECUTE PROCEDURE public.compat_missing_trigger_function()",
            )
            .await
            .expect_err("the unresolved trigger function should be rejected");
        let row_count: i64 = client
            .query_one("SELECT count(*) FROM compat_trigger_parse_rows", &[])
            .await
            .expect("the connection should execute after the rejected declaration")
            .get(0);

        // Assert
        assert_eq!(row_count, 0);
        assert_eq!(db_error(&error).code().code(), "42883");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_ignore_dropping_an_absent_trigger_with_if_exists() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("trigger_drop_if_exists").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE compat_trigger_drop_rows (id INT PRIMARY KEY)")
            .await
            .expect("create a table with no triggers");
        client
            .batch_execute("INSERT INTO compat_trigger_drop_rows VALUES (7)")
            .await
            .expect("populate the table before dropping an absent trigger");

        // Act
        client
            .batch_execute(
                "DROP TRIGGER IF EXISTS compat_absent_trigger \
                 ON compat_trigger_drop_rows",
            )
            .await
            .expect("dropping an absent trigger with IF EXISTS should succeed");
        let rows = client
            .query("SELECT id FROM compat_trigger_drop_rows ORDER BY id", &[])
            .await
            .expect("read the table after the no-op trigger drop");

        // Assert
        assert_eq!(
            rows.iter()
                .map(|row| row.get::<_, i32>(0))
                .collect::<Vec<_>>(),
            vec![7]
        );

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_process_row_semantics_for_a_multi_event_before_trigger() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("trigger_before_row_contract").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_before_trigger_rows ( \
                   id INT PRIMARY KEY, label TEXT NOT NULL, suppress BOOLEAN NOT NULL)",
                "CREATE TABLE compat_before_trigger_events ( \
                   id INT, operation TEXT, old_label TEXT, new_label TEXT)",
                "CREATE FUNCTION compat_before_trigger() RETURNS TRIGGER \
                   LANGUAGE plpgsql AS $function$ \
                   BEGIN \
                     IF TG_OP = 'INSERT' THEN \
                       IF NEW.suppress THEN RETURN NULL; END IF; \
                       NEW.label := NEW.label || TG_ARGV[0]; \
                       RETURN NEW; \
                     ELSIF TG_OP = 'UPDATE' THEN \
                       INSERT INTO compat_before_trigger_events \
                         VALUES (NEW.id, TG_OP, OLD.label, NEW.label); \
                       RETURN NEW; \
                     ELSE \
                       INSERT INTO compat_before_trigger_events \
                         VALUES (OLD.id, TG_OP, OLD.label, NULL); \
                       RETURN OLD; \
                     END IF; \
                   END; \
                   $function$",
                "CREATE TRIGGER compat_before_trigger_rows_biu \
                   BEFORE INSERT OR UPDATE OR DELETE ON compat_before_trigger_rows \
                   FOR EACH ROW EXECUTE FUNCTION compat_before_trigger('[checked]')",
            ],
        )
        .await
        .expect("create one multi-event BEFORE trigger with an argument");

        // Act
        client
            .batch_execute(
                "INSERT INTO compat_before_trigger_rows VALUES \
                   (1, 'one', FALSE), (2, 'two', TRUE)",
            )
            .await
            .expect("insert one row and suppress the other from the BEFORE trigger");
        let inserted = client
            .query(
                "SELECT id, label FROM compat_before_trigger_rows ORDER BY id",
                &[],
            )
            .await
            .expect("read the rows returned by the BEFORE trigger");
        client
            .batch_execute(
                "INSERT INTO compat_before_trigger_rows VALUES (1, 'replacement', FALSE) \
                 ON CONFLICT (id) DO UPDATE SET label = EXCLUDED.label",
            )
            .await
            .expect("run the BEFORE trigger for both the insert and update sides of an upsert");
        client
            .batch_execute(
                "UPDATE compat_before_trigger_rows SET label = 'final' WHERE id = 1; \
                 DELETE FROM compat_before_trigger_rows WHERE id = 1",
            )
            .await
            .expect("run the multi-event trigger for UPDATE and DELETE");
        let events = client
            .query(
                "SELECT id, operation, old_label, new_label \
                 FROM compat_before_trigger_events",
                &[],
            )
            .await
            .expect("read OLD and NEW values recorded by the trigger");

        // Assert
        assert_eq!(inserted.len(), 1, "the trigger suppresses the flagged row");
        assert_eq!(inserted[0].get::<_, i32>(0), 1);
        assert_eq!(inserted[0].get::<_, String>(1), "one[checked]");
        let mut actual_events = events
            .iter()
            .map(|event| {
                (
                    event.get::<_, i32>(0),
                    event.get::<_, String>(1),
                    event.get::<_, Option<String>>(2),
                    event.get::<_, Option<String>>(3),
                )
            })
            .collect::<Vec<_>>();
        actual_events.sort();
        assert_eq!(
            actual_events,
            vec![
                (1, "DELETE".to_owned(), Some("final".to_owned()), None),
                (
                    1,
                    "UPDATE".to_owned(),
                    Some("one[checked]".to_owned()),
                    Some("replacement[checked]".to_owned()),
                ),
                (
                    1,
                    "UPDATE".to_owned(),
                    Some("replacement[checked]".to_owned()),
                    Some("final".to_owned()),
                ),
            ]
        );

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_match_trigger_lifecycle_state() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("trigger_rename_enable_disable").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_trigger_toggle_rows (id INT PRIMARY KEY)",
                "CREATE TABLE compat_trigger_toggle_log (id INT)",
                "CREATE FUNCTION compat_trigger_toggle_log_row() RETURNS TRIGGER \
                   LANGUAGE plpgsql AS $function$ \
                   BEGIN \
                     INSERT INTO compat_trigger_toggle_log VALUES (NEW.id); \
                     RETURN NEW; \
                   END; \
                   $function$",
                "CREATE TRIGGER compat_trigger_toggle \
                   AFTER INSERT ON compat_trigger_toggle_rows FOR EACH ROW \
                   EXECUTE FUNCTION compat_trigger_toggle_log_row()",
            ],
        )
        .await
        .expect("create a row trigger and its audit table");

        // Act
        execute_statements(
            &client,
            &[
                "ALTER TABLE compat_trigger_toggle_rows \
                   DISABLE TRIGGER compat_trigger_toggle",
                "INSERT INTO compat_trigger_toggle_rows VALUES (1)",
                "ALTER TABLE compat_trigger_toggle_rows \
                   ENABLE TRIGGER compat_trigger_toggle",
                "ALTER TRIGGER compat_trigger_toggle \
                   ON compat_trigger_toggle_rows RENAME TO compat_trigger_toggle_renamed",
                "INSERT INTO compat_trigger_toggle_rows VALUES (2)",
                "ALTER TABLE compat_trigger_toggle_rows DISABLE TRIGGER ALL",
                "INSERT INTO compat_trigger_toggle_rows VALUES (3)",
                "ALTER TABLE compat_trigger_toggle_rows ENABLE TRIGGER ALL",
                "INSERT INTO compat_trigger_toggle_rows VALUES (4)",
            ],
        )
        .await
        .expect("disable, enable, rename and exercise the row trigger");
        let fired_ids = client
            .query("SELECT id FROM compat_trigger_toggle_log ORDER BY id", &[])
            .await
            .expect("read rows recorded while the trigger was enabled");
        let renamed_trigger: String = client
            .query_one(
                "SELECT tgname FROM pg_catalog.pg_trigger \
                 WHERE tgrelid = 'compat_trigger_toggle_rows'::regclass \
                   AND NOT tgisinternal",
                &[],
            )
            .await
            .expect("read the renamed trigger from the catalog")
            .try_get(0)
            .expect("trigger name");

        // Assert
        assert_eq!(fired_ids.len(), 2);
        assert_eq!(fired_ids[0].get::<_, i32>(0), 2);
        assert_eq!(fired_ids[1].get::<_, i32>(0), 4);
        assert_eq!(renamed_trigger, "compat_trigger_toggle_renamed");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_defer_a_constraint_trigger_until_constraints_are_checked() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("constraint_trigger_deferral").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_constraint_trigger_rows (id INT PRIMARY KEY)",
                "CREATE TABLE compat_constraint_trigger_log (id INT)",
                "CREATE FUNCTION compat_constraint_trigger_log_row() RETURNS TRIGGER \
                   LANGUAGE plpgsql AS $function$ \
                   BEGIN \
                     INSERT INTO compat_constraint_trigger_log VALUES (NEW.id); \
                     RETURN NEW; \
                   END; \
                   $function$",
                "CREATE CONSTRAINT TRIGGER compat_constraint_trigger_deferred \
                   AFTER INSERT ON compat_constraint_trigger_rows \
                   DEFERRABLE INITIALLY DEFERRED FOR EACH ROW \
                   EXECUTE FUNCTION compat_constraint_trigger_log_row()",
            ],
        )
        .await
        .expect("create a deferred constraint trigger");
        client
            .batch_execute("BEGIN")
            .await
            .expect("begin the transaction containing the trigger event");

        // Act
        client
            .batch_execute("INSERT INTO compat_constraint_trigger_rows VALUES (11)")
            .await
            .expect("insert a row whose constraint trigger is deferred");
        let before_check: i64 = client
            .query_one("SELECT count(*) FROM compat_constraint_trigger_log", &[])
            .await
            .expect("read trigger effects before checking constraints")
            .try_get(0)
            .expect("pre-check count");
        client
            .batch_execute("SET CONSTRAINTS compat_constraint_trigger_deferred IMMEDIATE")
            .await
            .expect("fire the deferred trigger when its constraint is checked");
        let after_check: i64 = client
            .query_one("SELECT count(*) FROM compat_constraint_trigger_log", &[])
            .await
            .expect("read trigger effects after checking constraints")
            .try_get(0)
            .expect("post-check count");
        client
            .batch_execute("COMMIT")
            .await
            .expect("commit the completed trigger event");

        // Assert
        assert_eq!(before_check, 0);
        assert_eq!(after_check, 1);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_match_statement_trigger_semantics() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("trigger_transition_and_truncate").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_transition_rows (id INT PRIMARY KEY)",
                "CREATE TABLE compat_transition_log (operation TEXT, rows_seen INT)",
                "CREATE FUNCTION compat_transition_log_statement() RETURNS TRIGGER \
                   LANGUAGE plpgsql AS $function$ \
                   BEGIN \
                     IF TG_OP = 'INSERT' THEN \
                       INSERT INTO compat_transition_log \
                         VALUES (TG_OP, (SELECT count(*) FROM inserted_rows)); \
                     ELSE \
                       INSERT INTO compat_transition_log VALUES (TG_OP, 0); \
                     END IF; \
                     RETURN NULL; \
                   END; \
                   $function$",
                "CREATE TRIGGER compat_transition_insert \
                   AFTER INSERT ON compat_transition_rows \
                   REFERENCING NEW TABLE AS inserted_rows FOR EACH STATEMENT \
                   EXECUTE FUNCTION compat_transition_log_statement()",
                "CREATE TRIGGER compat_transition_truncate \
                   AFTER TRUNCATE ON compat_transition_rows FOR EACH STATEMENT \
                   EXECUTE FUNCTION compat_transition_log_statement()",
            ],
        )
        .await
        .expect("create transition-table and TRUNCATE statement triggers");

        // Act
        client
            .batch_execute(
                "INSERT INTO compat_transition_rows VALUES (1), (2), (3); \
                 TRUNCATE compat_transition_rows",
            )
            .await
            .expect("insert a statement row set and then truncate the source table");
        let events = client
            .query(
                "SELECT operation, rows_seen FROM compat_transition_log ORDER BY operation",
                &[],
            )
            .await
            .expect("read transition-table and truncate trigger effects");

        // Assert
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].get::<_, String>(0), "INSERT");
        assert_eq!(events[0].get::<_, i32>(1), 3);
        assert_eq!(events[1].get::<_, String>(0), "TRUNCATE");
        assert_eq!(events[1].get::<_, i32>(1), 0);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_route_view_inserts_through_an_instead_of_trigger() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("trigger_instead_of_view").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_view_trigger_base (id INT PRIMARY KEY, label TEXT NOT NULL)",
                "CREATE VIEW compat_view_trigger AS \
                   SELECT id, label FROM compat_view_trigger_base",
                "CREATE FUNCTION compat_view_trigger_insert() RETURNS TRIGGER \
                   LANGUAGE plpgsql AS $function$ \
                   BEGIN \
                     INSERT INTO compat_view_trigger_base VALUES (NEW.id, NEW.label); \
                     RETURN NEW; \
                   END; \
                   $function$",
                "CREATE TRIGGER compat_view_trigger_insert_row \
                   INSTEAD OF INSERT ON compat_view_trigger FOR EACH ROW \
                   EXECUTE FUNCTION compat_view_trigger_insert()",
            ],
        )
        .await
        .expect("create a view with an INSTEAD OF INSERT trigger");

        // Act
        client
            .batch_execute("INSERT INTO compat_view_trigger VALUES (7, 'through view')")
            .await
            .expect("insert through the view trigger");
        let base_row = client
            .query_one(
                "SELECT id, label FROM compat_view_trigger_base WHERE id = 7",
                &[],
            )
            .await
            .expect("read the row written by the INSTEAD OF trigger");

        // Assert
        assert_eq!(base_row.get::<_, i32>(0), 7);
        assert_eq!(base_row.get::<_, String>(1), "through view");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_record_supported_ddl_from_an_event_trigger() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("event_trigger_ddl_command_end").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_event_trigger_log (command_tag TEXT, command_count BIGINT)",
                "CREATE FUNCTION compat_event_trigger_record() RETURNS EVENT_TRIGGER \
                   LANGUAGE plpgsql AS $function$ \
                   BEGIN \
                     INSERT INTO compat_event_trigger_log \
                       SELECT TG_TAG, count(*) FROM pg_event_trigger_ddl_commands(); \
                   END; \
                   $function$",
                "CREATE EVENT TRIGGER compat_event_trigger_ddl_command_end \
                   ON ddl_command_end WHEN TAG IN ('CREATE TABLE') \
                   EXECUTE FUNCTION compat_event_trigger_record()",
            ],
        )
        .await
        .expect("create a filtered DDL-command event trigger");

        // Act
        client
            .batch_execute("CREATE TABLE compat_event_trigger_target (id INT)")
            .await
            .expect("run the event trigger for the selected DDL tag");
        let event = client
            .query_one(
                "SELECT command_tag, command_count FROM compat_event_trigger_log",
                &[],
            )
            .await
            .expect("read the event trigger payload");

        // Assert
        assert_eq!(event.get::<_, String>(0), "CREATE TABLE");
        assert_eq!(event.get::<_, i64>(1), 1);

        drop(client);
        server.shutdown(connection).await;
    });
}
