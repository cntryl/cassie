use super::*;

#[test]
fn should_redefer_constraint_trigger_events_after_flushing() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("constraint_trigger_redefer").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_redefer_trigger_rows (id INT PRIMARY KEY)",
                "CREATE TABLE compat_redefer_trigger_log (id INT)",
                "CREATE FUNCTION compat_redefer_trigger_log_row() RETURNS TRIGGER \
                   LANGUAGE plpgsql AS $function$ \
                   BEGIN \
                     INSERT INTO compat_redefer_trigger_log VALUES (NEW.id); \
                     RETURN NEW; \
                   END; \
                   $function$",
                "CREATE CONSTRAINT TRIGGER compat_redefer_trigger \
                   AFTER INSERT ON compat_redefer_trigger_rows \
                   DEFERRABLE INITIALLY DEFERRED FOR EACH ROW \
                   EXECUTE FUNCTION compat_redefer_trigger_log_row()",
                "BEGIN",
            ],
        )
        .await
        .expect("create the deferred constraint trigger and begin a transaction");
        client
            .batch_execute("INSERT INTO compat_redefer_trigger_rows VALUES (1)")
            .await
            .expect("queue the first deferred trigger event");

        // Act
        let before_immediate = client
            .query("SELECT id FROM compat_redefer_trigger_log ORDER BY id", &[])
            .await
            .expect("read trigger effects before the constraint is checked")
            .iter()
            .map(|row| row.get::<_, i32>(0))
            .collect::<Vec<_>>();
        client
            .batch_execute("SET CONSTRAINTS compat_redefer_trigger IMMEDIATE")
            .await
            .expect("flush the first queued trigger event");
        let after_immediate = client
            .query("SELECT id FROM compat_redefer_trigger_log ORDER BY id", &[])
            .await
            .expect("read the effect flushed by SET CONSTRAINTS IMMEDIATE")
            .iter()
            .map(|row| row.get::<_, i32>(0))
            .collect::<Vec<_>>();
        client
            .batch_execute("SET CONSTRAINTS compat_redefer_trigger DEFERRED")
            .await
            .expect("defer later events again in the same transaction");
        client
            .batch_execute("INSERT INTO compat_redefer_trigger_rows VALUES (2)")
            .await
            .expect("queue a second event after returning to deferred mode");
        let after_redeferred_insert = client
            .query("SELECT id FROM compat_redefer_trigger_log ORDER BY id", &[])
            .await
            .expect("read effects before committing the redeferred event")
            .iter()
            .map(|row| row.get::<_, i32>(0))
            .collect::<Vec<_>>();
        client
            .batch_execute("COMMIT")
            .await
            .expect("fire the second deferred event at commit");
        let after_commit = client
            .query("SELECT id FROM compat_redefer_trigger_log ORDER BY id", &[])
            .await
            .expect("read both trigger effects after commit")
            .iter()
            .map(|row| row.get::<_, i32>(0))
            .collect::<Vec<_>>();

        // Assert
        drop(client);
        server.shutdown(connection).await;
        assert!(before_immediate.is_empty());
        assert_eq!(after_immediate, vec![1]);
        assert_eq!(after_redeferred_insert, vec![1]);
        assert_eq!(after_commit, vec![1, 2]);
    });
}
