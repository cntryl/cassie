use super::*;

#[test]
fn should_expose_update_transition_table_contents() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("trigger_update_transition_tables").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat837_transition_rows (\
                   id INT PRIMARY KEY, amount INT NOT NULL)",
                "CREATE TABLE compat837_transition_log (\
                   operation TEXT, old_count INT, new_count INT, \
                   old_sum BIGINT, new_sum BIGINT)",
                "INSERT INTO compat837_transition_rows VALUES \
                   (1, 10), (2, 20), (3, 30)",
                "CREATE FUNCTION compat837_log_update_transition() RETURNS TRIGGER \
                   LANGUAGE plpgsql AS $function$ \
                   BEGIN \
                     INSERT INTO compat837_transition_log \
                     SELECT TG_OP, \
                       (SELECT count(*) FROM prior_rows), \
                       (SELECT count(*) FROM updated_rows), \
                       (SELECT sum(amount) FROM prior_rows), \
                       (SELECT sum(amount) FROM updated_rows); \
                     RETURN NULL; \
                   END; \
                   $function$",
                "CREATE TRIGGER compat837_transition_update \
                   AFTER UPDATE ON compat837_transition_rows \
                   REFERENCING OLD TABLE AS prior_rows NEW TABLE AS updated_rows \
                   FOR EACH STATEMENT \
                   EXECUTE FUNCTION compat837_log_update_transition()",
            ],
        )
        .await
        .expect("create and seed an UPDATE transition-table trigger fixture");

        // Act
        client
            .batch_execute(
                "UPDATE compat837_transition_rows \
                 SET amount = amount + 1 WHERE id <= 2",
            )
            .await
            .expect("update two rows and fire the statement trigger once");
        let events = client
            .query(
                "SELECT operation, old_count, new_count, old_sum, new_sum \
                 FROM compat837_transition_log",
                &[],
            )
            .await
            .expect("read the old and new transition-table aggregates");

        // Assert
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].get::<_, String>(0), "UPDATE");
        assert_eq!(events[0].get::<_, i32>(1), 2);
        assert_eq!(events[0].get::<_, i32>(2), 2);
        assert_eq!(events[0].get::<_, i64>(3), 30);
        assert_eq!(events[0].get::<_, i64>(4), 32);

        drop(client);
        server.shutdown(connection).await;
    });
}
