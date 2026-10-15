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
