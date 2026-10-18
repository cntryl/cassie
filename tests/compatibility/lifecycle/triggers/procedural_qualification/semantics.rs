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
