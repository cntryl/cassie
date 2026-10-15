#[test]
fn should_preserve_create_sequence_options_when_if_not_exists_matches() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("sequence_create_option_forms").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute(
                "CREATE SEQUENCE compat_wave7_create_options AS INTEGER \
                 INCREMENT 2 START 5 NO MINVALUE NO MAXVALUE CACHE 3 NO CYCLE",
            )
            .await
            .expect("create a sequence with SQL-standard optional keywords");

        // Act
        client
            .batch_execute(
                "CREATE SEQUENCE IF NOT EXISTS compat_wave7_create_options \
                 START WITH 100 INCREMENT BY 10 CYCLE",
            )
            .await
            .expect("skip an existing sequence without replacing its options");
        let metadata = client
            .query_one(
                "SELECT seqtypid, seqstart, seqincrement, seqmin, seqmax, seqcache, seqcycle \
                 FROM pg_catalog.pg_sequence \
                 WHERE seqrelid = 'compat_wave7_create_options'::regclass",
                &[],
            )
            .await
            .expect("read the sequence's effective CREATE options");
        let first = client
            .query_one("SELECT nextval('compat_wave7_create_options')", &[])
            .await
            .expect("read the configured start value");
        let second = client
            .query_one("SELECT nextval('compat_wave7_create_options')", &[])
            .await
            .expect("advance by the configured increment");

        // Assert
        assert_eq!(metadata.get::<_, u32>(0), 23);
        assert_eq!(metadata.get::<_, i64>(1), 5);
        assert_eq!(metadata.get::<_, i64>(2), 2);
        assert_eq!(metadata.get::<_, i64>(3), 1);
        assert_eq!(metadata.get::<_, i64>(4), 2_147_483_647);
        assert_eq!(metadata.get::<_, i64>(5), 3);
        assert!(!metadata.get::<_, bool>(6));
        assert_eq!(metadata.columns()[0].type_().oid(), 26);
        assert_eq!(metadata.columns()[1].type_().oid(), 20);
        assert_eq!(first.get::<_, i64>(0), 5);
        assert_eq!(second.get::<_, i64>(0), 7);
        assert_eq!(first.columns()[0].type_().oid(), 20);
        assert_eq!(second.columns()[0].type_().oid(), 20);

        drop(client);
        server.shutdown(connection).await;
    });
}
#[test]
fn should_retain_a_sequence_after_removing_its_owned_by_dependency() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("sequence_owned_by_none").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_wave7_owned_rows (id INTEGER)",
                "CREATE SEQUENCE compat_wave7_detached_sequence",
                "ALTER SEQUENCE compat_wave7_detached_sequence \
                 OWNED BY compat_wave7_owned_rows.id",
            ],
        )
        .await
        .expect("make the sequence dependent on a table column");
        let before_detach: bool = client
            .query_one(
                "SELECT pg_get_serial_sequence( \
                   'compat_wave7_owned_rows', 'id') IS NOT NULL",
                &[],
            )
            .await
            .expect("read the ownership dependency")
            .try_get(0)
            .expect("ownership presence");

        // Act
        client
            .batch_execute("ALTER SEQUENCE compat_wave7_detached_sequence OWNED BY NONE")
            .await
            .expect("remove the sequence's table-column dependency");
        let after_detach: bool = client
            .query_one(
                "SELECT pg_get_serial_sequence( \
                   'compat_wave7_owned_rows', 'id') IS NULL",
                &[],
            )
            .await
            .expect("confirm that the table no longer owns a sequence")
            .try_get(0)
            .expect("ownership absence");
        client
            .batch_execute("DROP TABLE compat_wave7_owned_rows")
            .await
            .expect("drop the former owner table");
        let surviving = client
            .query_one(
                "SELECT to_regclass('compat_wave7_detached_sequence') IS NOT NULL, \
                        nextval('compat_wave7_detached_sequence')",
                &[],
            )
            .await
            .expect("use the sequence after dropping its former owner");

        // Assert
        assert!(before_detach);
        assert!(after_detach);
        assert!(surviving.get::<_, bool>(0));
        assert_eq!(surviving.get::<_, i64>(1), 1);
        assert_eq!(surviving.columns()[1].type_().oid(), 20);

        drop(client);
        server.shutdown(connection).await;
    });
}
#[test]
fn should_apply_altered_options_to_an_identity_sequence() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("identity_sequence_altered_options").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute(
                "CREATE TABLE compat_wave7_identity_options ( \
                   id BIGINT GENERATED BY DEFAULT AS IDENTITY \
                     (START WITH 5 INCREMENT BY 2))",
            )
            .await
            .expect("create an identity with initial sequence options");
        let first = client
            .query_one(
                "INSERT INTO compat_wave7_identity_options DEFAULT VALUES RETURNING id",
                &[],
            )
            .await
            .expect("generate the first identity value");

        // Act
        client
            .batch_execute(
                "ALTER TABLE compat_wave7_identity_options \
                 ALTER COLUMN id SET INCREMENT BY 3",
            )
            .await
            .expect("change the identity sequence increment");
        client
            .batch_execute(
                "ALTER TABLE compat_wave7_identity_options \
                 ALTER COLUMN id RESTART WITH 20",
            )
            .await
            .expect("restart the identity sequence at a chosen value");
        let restarted = client
            .query_one(
                "INSERT INTO compat_wave7_identity_options DEFAULT VALUES RETURNING id",
                &[],
            )
            .await
            .expect("generate from the altered identity sequence");
        let advanced = client
            .query_one(
                "INSERT INTO compat_wave7_identity_options DEFAULT VALUES RETURNING id",
                &[],
            )
            .await
            .expect("advance by the altered identity increment");

        // Assert
        assert_eq!(first.get::<_, i64>(0), 5);
        assert_eq!(restarted.get::<_, i64>(0), 20);
        assert_eq!(advanced.get::<_, i64>(0), 23);
        assert!([&first, &restarted, &advanced]
            .iter()
            .all(|row| row.columns()[0].type_().oid() == 20));

        drop(client);
        server.shutdown(connection).await;
    });
}
#[test]
fn should_merge_an_explicit_value_into_an_always_identity() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("merge_override_identity").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_wave7_merge_identity ( \
                   id INTEGER GENERATED ALWAYS AS IDENTITY PRIMARY KEY, label TEXT)",
                "CREATE TABLE compat_wave7_merge_identity_source (id INTEGER, label TEXT)",
                "INSERT INTO compat_wave7_merge_identity_source VALUES (50, 'explicit')",
            ],
        )
        .await
        .expect("create the identity target and explicit source row");

        // Act
        let merged = client
            .query_one(
                "MERGE INTO compat_wave7_merge_identity AS target \
                 USING compat_wave7_merge_identity_source AS source ON false \
                 WHEN NOT MATCHED THEN \
                   INSERT (id, label) OVERRIDING SYSTEM VALUE \
                   VALUES (source.id, source.label) \
                 RETURNING merge_action(), target.id, target.label",
                &[],
            )
            .await
            .expect("merge a row with an explicit system identity value");
        let generated = client
            .query_one(
                "INSERT INTO compat_wave7_merge_identity (label) \
                 VALUES ('generated') RETURNING id",
                &[],
            )
            .await
            .expect("generate a later identity value");

        // Assert
        assert_eq!(merged.get::<_, String>(0), "INSERT");
        assert_eq!(merged.columns()[0].type_().oid(), 25);
        assert_eq!(merged.get::<_, i32>(1), 50);
        assert_eq!(merged.columns()[1].type_().oid(), 23);
        assert_eq!(merged.get::<_, String>(2), "explicit");
        assert_eq!(merged.columns()[2].type_().oid(), 25);
        assert_eq!(generated.get::<_, i32>(0), 1);
        assert_eq!(generated.columns()[0].type_().oid(), 23);

        drop(client);
        server.shutdown(connection).await;
    });
}
