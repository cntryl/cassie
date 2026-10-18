use super::*;

#[test]
fn should_replace_trigger_handler_created_with_procedure_syntax() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("trigger_replace_legacy_syntax").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE SCHEMA compat_trigger_replace",
                "CREATE TABLE compat_trigger_replace.source_rows (id INT PRIMARY KEY)",
                "CREATE TABLE compat_trigger_replace.trigger_log ( \
                   event_id INT GENERATED ALWAYS AS IDENTITY, source_id INT, handler TEXT)",
                "CREATE FUNCTION compat_trigger_replace.record_legacy() RETURNS TRIGGER \
                   LANGUAGE plpgsql AS $function$ \
                   BEGIN \
                     INSERT INTO compat_trigger_replace.trigger_log (source_id, handler) \
                       VALUES (NEW.id, 'legacy'); \
                     RETURN NEW; \
                   END; \
                   $function$",
                "CREATE FUNCTION compat_trigger_replace.record_replacement() RETURNS TRIGGER \
                   LANGUAGE plpgsql AS $function$ \
                   BEGIN \
                     INSERT INTO compat_trigger_replace.trigger_log (source_id, handler) \
                       VALUES (NEW.id, 'replacement'); \
                     RETURN NEW; \
                   END; \
                   $function$",
                "CREATE TRIGGER compat_replaceable_trigger \
                   AFTER INSERT ON compat_trigger_replace.source_rows FOR EACH ROW \
                   EXECUTE PROCEDURE compat_trigger_replace.record_legacy()",
                "INSERT INTO compat_trigger_replace.source_rows VALUES (1)",
            ],
        )
        .await
        .expect("create and fire a trigger using legacy EXECUTE PROCEDURE syntax");
        let trigger_oid_before: i64 = client
            .query_one(
                "SELECT oid::BIGINT FROM pg_catalog.pg_trigger \
                 WHERE tgname = 'compat_replaceable_trigger' \
                   AND tgrelid = 'compat_trigger_replace.source_rows'::regclass",
                &[],
            )
            .await
            .expect("read the trigger identity before replacing its handler")
            .get(0);

        // Act
        client
            .batch_execute(
                "CREATE OR REPLACE TRIGGER compat_replaceable_trigger \
                   AFTER INSERT ON compat_trigger_replace.source_rows FOR EACH ROW \
                   EXECUTE FUNCTION compat_trigger_replace.record_replacement(); \
                 INSERT INTO compat_trigger_replace.source_rows VALUES (2)",
            )
            .await
            .expect("replace the trigger handler and fire the replacement");
        let events = client
            .query(
                "SELECT source_id, handler FROM compat_trigger_replace.trigger_log \
                 ORDER BY event_id",
                &[],
            )
            .await
            .expect("read effects from the original and replacement handlers");
        let metadata = client
            .query_one(
                "SELECT trigger_row.oid::BIGINT, trigger_row.tgname, \
                        trigger_row.tgfoid = \
                          'compat_trigger_replace.record_replacement()'::regprocedure::oid, \
                        trigger_row.tgenabled::TEXT, trigger_row.tgtype::INT, \
                        count(*) OVER () \
                 FROM pg_catalog.pg_trigger AS trigger_row \
                 WHERE trigger_row.tgrelid = \
                   'compat_trigger_replace.source_rows'::regclass \
                   AND NOT trigger_row.tgisinternal",
                &[],
            )
            .await
            .expect("read the replacement trigger's catalog definition");

        // Assert
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].get::<_, i32>(0), 1);
        assert_eq!(events[0].get::<_, String>(1), "legacy");
        assert_eq!(events[1].get::<_, i32>(0), 2);
        assert_eq!(events[1].get::<_, String>(1), "replacement");
        assert_eq!(metadata.get::<_, i64>(0), trigger_oid_before);
        assert_eq!(metadata.get::<_, String>(1), "compat_replaceable_trigger");
        assert!(metadata.get::<_, bool>(2));
        assert_eq!(metadata.get::<_, String>(3), "O");
        assert_eq!(metadata.get::<_, i32>(4), 5);
        assert_eq!(metadata.get::<_, i64>(5), 1);

        drop(client);
        server.shutdown(connection).await;
    });
}
