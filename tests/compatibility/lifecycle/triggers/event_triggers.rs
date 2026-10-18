use super::*;

struct EventTriggerLifecycleObservations {
    metadata: tokio_postgres::Row,
    disabled_state: String,
    before_drop: i64,
    catalog_after_drop: i64,
    after_drop: i64,
}

async fn observe_event_trigger_lifecycle(
    client: &tokio_postgres::Client,
) -> EventTriggerLifecycleObservations {
    let metadata = client
        .query_one(
            "SELECT evtname::text, evtevent::text, evtenabled::text, \
                    evttags @> ARRAY['CREATE TABLE']::text[] \
             FROM pg_catalog.pg_event_trigger \
             WHERE evtname = 'compat_event_trigger_lifecycle'",
            &[],
        )
        .await
        .expect("read event-trigger catalog state");
    execute_statements(
        client,
        &[
            "CREATE TABLE compat_event_trigger_selected (id INT)",
            "CREATE INDEX compat_event_trigger_unmatched_idx \
               ON compat_event_trigger_selected (id)",
            "ALTER EVENT TRIGGER compat_event_trigger_lifecycle DISABLE",
            "CREATE TABLE compat_event_trigger_disabled (id INT)",
        ],
    )
    .await
    .expect("filter DDL commands and disable the event trigger");
    let disabled_state: String = client
        .query_one(
            "SELECT evtenabled::text FROM pg_catalog.pg_event_trigger \
             WHERE evtname = 'compat_event_trigger_lifecycle'",
            &[],
        )
        .await
        .expect("read disabled event-trigger state")
        .try_get(0)
        .expect("disabled event-trigger state");
    execute_statements(
        client,
        &[
            "ALTER EVENT TRIGGER compat_event_trigger_lifecycle ENABLE",
            "CREATE TABLE compat_event_trigger_resumed (id INT)",
        ],
    )
    .await
    .expect("enable the event trigger and fire it again");
    let before_drop: i64 = client
        .query_one(
            "SELECT count(*) FROM compat_event_trigger_lifecycle_log",
            &[],
        )
        .await
        .expect("count selected DDL events before dropping the trigger")
        .try_get(0)
        .expect("event count before trigger drop");
    client
        .batch_execute("DROP EVENT TRIGGER compat_event_trigger_lifecycle")
        .await
        .expect("drop the event trigger");
    let catalog_after_drop: i64 = client
        .query_one(
            "SELECT count(*) FROM pg_catalog.pg_event_trigger \
             WHERE evtname = 'compat_event_trigger_lifecycle'",
            &[],
        )
        .await
        .expect("verify event-trigger catalog removal")
        .try_get(0)
        .expect("event-trigger catalog count");
    client
        .batch_execute("CREATE TABLE compat_event_trigger_after_drop (id INT)")
        .await
        .expect("create matching DDL after dropping the event trigger");
    let after_drop: i64 = client
        .query_one(
            "SELECT count(*) FROM compat_event_trigger_lifecycle_log",
            &[],
        )
        .await
        .expect("count events after dropping the trigger")
        .try_get(0)
        .expect("event count after trigger drop");
    EventTriggerLifecycleObservations {
        metadata,
        disabled_state,
        before_drop,
        catalog_after_drop,
        after_drop,
    }
}

fn assert_event_trigger_lifecycle(observations: &EventTriggerLifecycleObservations) {
    assert_eq!(
        observations.metadata.get::<_, String>(0),
        "compat_event_trigger_lifecycle"
    );
    assert_eq!(observations.metadata.get::<_, String>(1), "ddl_command_end");
    assert_eq!(observations.metadata.get::<_, String>(2), "O");
    assert!(observations.metadata.get::<_, bool>(3));
    assert_eq!(observations.disabled_state, "D");
    assert_eq!(
        observations.before_drop, 2,
        "the filter and disabled state exclude other DDL"
    );
    assert_eq!(observations.catalog_after_drop, 0);
    assert_eq!(observations.after_drop, observations.before_drop);
}

#[test]
fn should_capture_dropped_table_metadata_for_sql_drop() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("event_trigger_sql_drop_payload").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_event_trigger_drop_log ( \
                   tag TEXT, object_type TEXT, schema_name TEXT, object_name TEXT, \
                   object_identity TEXT, original BOOLEAN, normal BOOLEAN, is_temporary BOOLEAN)",
                "CREATE FUNCTION compat_event_trigger_capture_drop() \
                   RETURNS EVENT_TRIGGER LANGUAGE plpgsql AS $function$ \
                   BEGIN \
                     INSERT INTO compat_event_trigger_drop_log \
                       SELECT TG_TAG, object_type, schema_name, object_name, \
                              object_identity, original, normal, is_temporary \
                       FROM pg_catalog.pg_event_trigger_dropped_objects() \
                       WHERE object_type = 'table'; \
                   END; \
                   $function$",
                "CREATE EVENT TRIGGER compat_event_trigger_sql_drop \
                   ON sql_drop WHEN TAG IN ('DROP TABLE') \
                   EXECUTE FUNCTION compat_event_trigger_capture_drop()",
                "CREATE TABLE public.compat_event_trigger_drop_target \
                   (id INT PRIMARY KEY, label TEXT)",
            ],
        )
        .await
        .expect("create the sql_drop observer and its target table");

        // Act
        client
            .batch_execute("DROP TABLE public.compat_event_trigger_drop_target")
            .await
            .expect("fire sql_drop while removing the target table");
        let entry = client
            .query_one(
                "SELECT tag, object_type, schema_name, object_name, object_identity, \
                        original, normal, is_temporary \
                 FROM compat_event_trigger_drop_log \
                 WHERE object_type = 'table' \
                   AND object_name = 'compat_event_trigger_drop_target'",
                &[],
            )
            .await
            .expect("read the table identity captured by sql_drop");

        // Assert
        assert_eq!(entry.get::<_, String>(0), "DROP TABLE");
        assert_eq!(entry.get::<_, String>(1), "table");
        assert_eq!(entry.get::<_, String>(2), "public");
        assert_eq!(
            entry.get::<_, String>(3),
            "compat_event_trigger_drop_target"
        );
        assert_eq!(
            entry.get::<_, String>(4),
            "public.compat_event_trigger_drop_target"
        );
        assert!(entry.get::<_, bool>(5));
        assert!(!entry.get::<_, bool>(6));
        assert!(!entry.get::<_, bool>(7));

        execute_statements(
            &client,
            &[
                "DROP EVENT TRIGGER compat_event_trigger_sql_drop",
                "DROP FUNCTION compat_event_trigger_capture_drop()",
                "DROP TABLE compat_event_trigger_drop_log",
            ],
        )
        .await
        .expect("remove the event trigger fixture objects");
        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_enforce_event_trigger_catalog_lifecycle() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("event_trigger_enable_disable_drop").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_event_trigger_lifecycle_log ( \
                   command_tag TEXT, command_count BIGINT)",
                "CREATE FUNCTION compat_event_trigger_lifecycle_record() \
                   RETURNS EVENT_TRIGGER LANGUAGE plpgsql AS $function$ \
                   BEGIN \
                     INSERT INTO compat_event_trigger_lifecycle_log \
                       SELECT TG_TAG, count(*) FROM pg_catalog.pg_event_trigger_ddl_commands(); \
                   END; \
                   $function$",
                "CREATE EVENT TRIGGER compat_event_trigger_lifecycle \
                   ON ddl_command_end WHEN TAG IN ('CREATE TABLE') \
                   EXECUTE FUNCTION compat_event_trigger_lifecycle_record()",
            ],
        )
        .await
        .expect("create a filtered DDL-command event trigger");
        // Act
        let observations = observe_event_trigger_lifecycle(&client).await;

        // Assert
        assert_event_trigger_lifecycle(&observations);

        execute_statements(
            &client,
            &[
                "DROP TABLE compat_event_trigger_selected, \
                   compat_event_trigger_disabled, compat_event_trigger_resumed, \
                   compat_event_trigger_after_drop",
                "DROP FUNCTION compat_event_trigger_lifecycle_record()",
                "DROP TABLE compat_event_trigger_lifecycle_log",
            ],
        )
        .await
        .expect("remove the event-trigger lifecycle fixture objects");
        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_reject_an_unknown_event_name_without_creating_a_trigger() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("event_trigger_invalid_event_name").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute(
                "CREATE FUNCTION compat_event_trigger_invalid_handler() \
                   RETURNS EVENT_TRIGGER LANGUAGE plpgsql AS $function$ \
                   BEGIN \
                     NULL; \
                   END; \
                   $function$",
            )
            .await
            .expect("create a valid handler for the invalid event-name test");

        // Act
        let error = client
            .batch_execute(
                "CREATE EVENT TRIGGER compat_event_trigger_invalid \
                 ON ddl_command_finish \
                 EXECUTE FUNCTION compat_event_trigger_invalid_handler()",
            )
            .await
            .expect_err("PostgreSQL does not define a ddl_command_finish event");
        let remaining: i64 = client
            .query_one(
                "SELECT count(*) FROM pg_catalog.pg_event_trigger \
                 WHERE evtname = 'compat_event_trigger_invalid'",
                &[],
            )
            .await
            .expect("inspect event-trigger state after rejected creation")
            .try_get(0)
            .expect("event-trigger count");

        // Assert
        assert_eq!(db_error(&error).code().code(), "42601");
        assert_eq!(
            remaining, 0,
            "rejected event names must leave no catalog row"
        );
        client
            .batch_execute("DROP FUNCTION compat_event_trigger_invalid_handler()")
            .await
            .expect("remove the unused event-trigger handler");
        drop(client);
        server.shutdown(connection).await;
    });
}
