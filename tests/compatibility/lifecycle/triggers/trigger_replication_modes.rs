use super::*;

#[test]
fn should_apply_replication_trigger_enable_modes() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("trigger_replication_enable_modes").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_replication_trigger_rows (id INT PRIMARY KEY)",
                "CREATE TABLE compat_replication_trigger_log \
                   (trigger_name TEXT, source_id INT)",
                "CREATE FUNCTION compat_replication_trigger_log_insert() RETURNS TRIGGER \
                   LANGUAGE plpgsql AS $function$ \
                   BEGIN \
                     INSERT INTO compat_replication_trigger_log VALUES (TG_NAME, NEW.id); \
                     RETURN NEW; \
                   END; \
                   $function$",
                "CREATE TRIGGER compat_always_insert \
                   AFTER INSERT ON compat_replication_trigger_rows FOR EACH ROW \
                   EXECUTE FUNCTION compat_replication_trigger_log_insert()",
                "CREATE TRIGGER compat_replica_insert \
                   AFTER INSERT ON compat_replication_trigger_rows FOR EACH ROW \
                   EXECUTE FUNCTION compat_replication_trigger_log_insert()",
                "ALTER TABLE compat_replication_trigger_rows \
                   ENABLE ALWAYS TRIGGER compat_always_insert",
                "ALTER TABLE compat_replication_trigger_rows \
                   ENABLE REPLICA TRIGGER compat_replica_insert",
            ],
        )
        .await
        .expect("create row triggers with PostgreSQL replication enable modes");

        // Act
        client
            .batch_execute(
                "SET session_replication_role = origin; \
                 INSERT INTO compat_replication_trigger_rows VALUES (1); \
                 SET session_replication_role = replica; \
                 INSERT INTO compat_replication_trigger_rows VALUES (2); \
                 SET session_replication_role = origin; \
                 INSERT INTO compat_replication_trigger_rows VALUES (3)",
            )
            .await
            .expect("fire ALWAYS and REPLICA triggers under both session roles");
        let fired = client
            .query(
                "SELECT trigger_name, source_id FROM compat_replication_trigger_log \
                 ORDER BY source_id, trigger_name",
                &[],
            )
            .await
            .expect("read effects from the enabled trigger modes");
        let catalog_modes = client
            .query(
                "SELECT tgname, tgenabled::TEXT \
                 FROM pg_catalog.pg_trigger \
                 WHERE tgrelid = 'compat_replication_trigger_rows'::regclass \
                   AND NOT tgisinternal ORDER BY tgname",
                &[],
            )
            .await
            .expect("read trigger enable modes from pg_trigger");

        // Assert
        assert_eq!(
            fired
                .iter()
                .map(|row| (row.get::<_, String>(0), row.get::<_, i32>(1)))
                .collect::<Vec<_>>(),
            vec![
                ("compat_always_insert".to_owned(), 1),
                ("compat_always_insert".to_owned(), 2),
                ("compat_replica_insert".to_owned(), 2),
                ("compat_always_insert".to_owned(), 3),
            ]
        );
        assert_eq!(
            catalog_modes
                .iter()
                .map(|row| (row.get::<_, String>(0), row.get::<_, String>(1)))
                .collect::<Vec<_>>(),
            vec![
                ("compat_always_insert".to_owned(), "A".to_owned()),
                ("compat_replica_insert".to_owned(), "R".to_owned()),
            ]
        );

        drop(client);
        server.shutdown(connection).await;
    });
}
