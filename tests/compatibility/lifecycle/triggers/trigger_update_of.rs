use super::*;

#[test]
fn should_fire_update_of_trigger_for_targeted_columns() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("trigger_update_of_target_columns").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_update_of_rows ( \
                   id INT PRIMARY KEY, state TEXT NOT NULL, note TEXT NOT NULL)",
                "CREATE TABLE compat_update_of_log ( \
                   id INT, old_state TEXT, new_state TEXT)",
                "CREATE FUNCTION compat_update_of_capture() RETURNS TRIGGER \
                   LANGUAGE plpgsql AS $function$ \
                   BEGIN \
                     INSERT INTO compat_update_of_log VALUES (NEW.id, OLD.state, NEW.state); \
                     RETURN NEW; \
                   END; \
                   $function$",
                "CREATE TRIGGER compat_update_of_state \
                   AFTER UPDATE OF state ON compat_update_of_rows FOR EACH ROW \
                   EXECUTE FUNCTION compat_update_of_capture()",
                "INSERT INTO compat_update_of_rows VALUES \
                   (1, 'same', 'first'), (2, 'before', 'second')",
            ],
        )
        .await
        .expect("create a row trigger filtered by its UPDATE target column");

        // Act
        client
            .batch_execute(
                "UPDATE compat_update_of_rows SET note = 'changed' WHERE id = 1; \
                 UPDATE compat_update_of_rows SET state = state WHERE id = 1; \
                 UPDATE compat_update_of_rows SET state = 'after' WHERE id = 2",
            )
            .await
            .expect("update both filtered and unfiltered columns");
        let events = client
            .query(
                "SELECT id, old_state, new_state FROM compat_update_of_log ORDER BY id",
                &[],
            )
            .await
            .expect("read effects from the column-filtered trigger");

        // Assert
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].get::<_, i32>(0), 1);
        assert_eq!(events[0].get::<_, String>(1), "same");
        assert_eq!(events[0].get::<_, String>(2), "same");
        assert_eq!(events[1].get::<_, i32>(0), 2);
        assert_eq!(events[1].get::<_, String>(1), "before");
        assert_eq!(events[1].get::<_, String>(2), "after");

        drop(client);
        server.shutdown(connection).await;
    });
}
