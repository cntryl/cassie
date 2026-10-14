use super::*;

#[test]
fn should_preserve_enum_scalar_result_type_identity() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("enum_scalar_result_oid_contract").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE SCHEMA compat_enum_result_identity",
                "CREATE TYPE compat_enum_result_identity.state_code \
                 AS ENUM ('open', 'active', 'done')",
            ],
        )
        .await
        .expect("create a named enum whose scalar result identity is checked");
        let enum_oid = client
            .query_one(
                "SELECT oid::BIGINT \
                 FROM pg_catalog.pg_type \
                 WHERE oid = 'compat_enum_result_identity.state_code'::regtype",
                &[],
            )
            .await
            .expect("read the enum type OID from pg_type")
            .get::<_, i64>(0);

        // Act
        let descriptors = client
            .query_one(
                "SELECT 'open'::compat_enum_result_identity.state_code AS literal, \
                        $1::compat_enum_result_identity.state_code AS parameter, \
                        CAST('done' AS compat_enum_result_identity.state_code) AS cast_value",
                &[&"active"],
            )
            .await
            .expect("describe literal, parameter, and cast enum results");
        let rendered = client
            .query_one(
                "SELECT 'open'::compat_enum_result_identity.state_code::TEXT, \
                        $1::compat_enum_result_identity.state_code::TEXT, \
                        CAST('done' AS compat_enum_result_identity.state_code)::TEXT",
                &[&"active"],
            )
            .await
            .expect("render literal, parameter, and cast enum values as text");

        // Assert
        assert_eq!(
            descriptors
                .columns()
                .iter()
                .map(|column| i64::from(column.type_().oid()))
                .collect::<Vec<_>>(),
            vec![enum_oid, enum_oid, enum_oid]
        );
        assert_eq!(descriptors.columns()[0].type_().name(), "state_code");
        assert_eq!(
            descriptors.columns()[0].type_().schema(),
            "compat_enum_result_identity"
        );
        assert_eq!(rendered.get::<_, String>(0), "open");
        assert_eq!(rendered.get::<_, String>(1), "active");
        assert_eq!(rendered.get::<_, String>(2), "done");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_preserve_enum_array_result_type_identity() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("enum_array_result_oid_contract").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE SCHEMA compat_enum_array_identity",
                "CREATE TYPE compat_enum_array_identity.state_code \
                 AS ENUM ('open', 'active', 'done')",
            ],
        )
        .await
        .expect("create a named enum whose array result identity is checked");
        let array_oid = client
            .query_one(
                "SELECT typarray::BIGINT \
                 FROM pg_catalog.pg_type \
                 WHERE oid = 'compat_enum_array_identity.state_code'::regtype",
                &[],
            )
            .await
            .expect("read the enum array OID from pg_type")
            .get::<_, i64>(0);

        // Act
        let descriptor = client
            .query_one(
                "SELECT ARRAY['open'::compat_enum_array_identity.state_code, \
                              NULL::compat_enum_array_identity.state_code] AS states",
                &[],
            )
            .await
            .expect("construct an enum array retaining its named element type");
        let rendered = client
            .query_one(
                "SELECT ARRAY['open'::compat_enum_array_identity.state_code, \
                              NULL::compat_enum_array_identity.state_code]::TEXT",
                &[],
            )
            .await
            .expect("render an enum array containing a NULL element");

        // Assert
        assert_eq!(i64::from(descriptor.columns()[0].type_().oid()), array_oid);
        assert_eq!(descriptor.columns()[0].type_().name(), "_state_code");
        assert_eq!(
            descriptor.columns()[0].type_().schema(),
            "compat_enum_array_identity"
        );
        assert_eq!(rendered.get::<_, String>(0), "{open,NULL}");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_compare_enum_values_in_declared_order() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("enum_comparison_order_contract").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute(
                "CREATE TYPE compat_enum_comparison_order \
                 AS ENUM ('open', 'active', 'done')",
            )
            .await
            .expect("create an enum with a declared comparison order");

        // Act
        let comparison = client
            .query_one(
                "SELECT $1::compat_enum_comparison_order < \
                        'done'::compat_enum_comparison_order",
                &[&"active"],
            )
            .await
            .expect("compare enum values by their declared label order");

        // Assert
        assert_eq!(comparison.columns()[0].type_().oid(), 16);
        assert!(comparison.get::<_, bool>(0));

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_reject_unknown_enum_inputs_with_invalid_text_state() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("enum_invalid_input_sqlstate").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE SCHEMA compat_enum_invalid_input",
                "CREATE TYPE compat_enum_invalid_input.state_code \
                 AS ENUM ('ready', 'done')",
            ],
        )
        .await
        .expect("create the enum used for invalid literal and parameter input");

        // Act
        let invalid_literal = client
            .query_one(
                "SELECT 'missing'::compat_enum_invalid_input.state_code",
                &[],
            )
            .await
            .expect_err("reject a literal that is not an enum label");
        let invalid_parameter = client
            .query_one(
                "SELECT $1::compat_enum_invalid_input.state_code",
                &[&"missing"],
            )
            .await
            .expect_err("reject a bound string that is not an enum label");
        let recovered = client
            .query_one(
                "SELECT 'ready'::compat_enum_invalid_input.state_code::TEXT",
                &[],
            )
            .await
            .expect("accept a valid label after both invalid inputs");

        // Assert
        assert_eq!(db_error(&invalid_literal).code().code(), "22P02");
        assert_eq!(db_error(&invalid_parameter).code().code(), "22P02");
        assert_eq!(recovered.get::<_, String>(0), "ready");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_scope_enum_alterations_to_the_named_schema() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("enum_schema_scoped_lifecycle").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE SCHEMA compat_enum_lifecycle_a",
                "CREATE SCHEMA compat_enum_lifecycle_b",
                "CREATE TYPE compat_enum_lifecycle_a.state_code \
                 AS ENUM ('queued', 'active', 'closed')",
                "CREATE TYPE compat_enum_lifecycle_b.state_code \
                 AS ENUM ('waiting', 'done')",
                "CREATE TABLE compat_enum_lifecycle_a.jobs \
                 (id INT PRIMARY KEY, state compat_enum_lifecycle_a.state_code)",
                "INSERT INTO compat_enum_lifecycle_a.jobs VALUES \
                 (1, 'queued'), (2, 'closed')",
                "SET search_path TO compat_enum_lifecycle_a, \
                 compat_enum_lifecycle_b, public",
            ],
        )
        .await
        .expect("create same-named schema enums and a dependent table");
        let initial_lookup = client
            .query_one(
                "SELECT to_regtype('state_code') = \
                          'compat_enum_lifecycle_a.state_code'::regtype",
                &[],
            )
            .await
            .expect("resolve the first same-named enum through search_path");

        // Act
        execute_statements(
            &client,
            &[
                "ALTER TYPE compat_enum_lifecycle_a.state_code \
                 ADD VALUE 'paused' BEFORE 'active'",
                "ALTER TYPE compat_enum_lifecycle_a.state_code \
                 RENAME VALUE 'closed' TO 'complete'",
                "ALTER TYPE compat_enum_lifecycle_a.state_code \
                 RENAME TO workflow_state",
                "INSERT INTO compat_enum_lifecycle_a.jobs VALUES (3, 'paused')",
            ],
        )
        .await
        .expect("alter only the schema-qualified enum and use its new label");
        let labels = client
            .query_one(
                "SELECT enum_range(NULL::compat_enum_lifecycle_a.workflow_state)::TEXT",
                &[],
            )
            .await
            .expect("read the schema-qualified enum labels after alteration");
        let rows = client
            .query(
                "SELECT id, state::TEXT FROM compat_enum_lifecycle_a.jobs \
                 ORDER BY state",
                &[],
            )
            .await
            .expect("read stored labels in their enum sort order");
        let lookups_after_rename = client
            .query_one(
                "SELECT to_regtype('state_code') = \
                          'compat_enum_lifecycle_b.state_code'::regtype, \
                        to_regtype('workflow_state') = \
                          'compat_enum_lifecycle_a.workflow_state'::regtype, \
                        enum_range(NULL::compat_enum_lifecycle_b.state_code)::TEXT",
                &[],
            )
            .await
            .expect("resolve both schemas after renaming one enum type");
        // Assert
        assert!(initial_lookup.get::<_, bool>(0));
        assert_eq!(
            labels.get::<_, String>(0),
            "{queued,paused,active,complete}"
        );
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].get::<_, i32>(0), 1);
        assert_eq!(rows[0].get::<_, String>(1), "queued");
        assert_eq!(rows[1].get::<_, i32>(0), 3);
        assert_eq!(rows[1].get::<_, String>(1), "paused");
        assert_eq!(rows[2].get::<_, i32>(0), 2);
        assert_eq!(rows[2].get::<_, String>(1), "complete");
        assert!(lookups_after_rename.get::<_, bool>(0));
        assert!(lookups_after_rename.get::<_, bool>(1));
        assert_eq!(lookups_after_rename.get::<_, String>(2), "{waiting,done}");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_preserve_enum_dependency_on_type_drop() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("enum_type_dependency_drop").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE SCHEMA compat_enum_drop_a",
                "CREATE SCHEMA compat_enum_drop_b",
                "CREATE TYPE compat_enum_drop_a.state_code AS ENUM ('ready', 'done')",
                "CREATE TYPE compat_enum_drop_b.state_code AS ENUM ('waiting', 'done')",
                "CREATE TABLE compat_enum_drop_a.jobs \
                 (id INT PRIMARY KEY, state compat_enum_drop_a.state_code)",
            ],
        )
        .await
        .expect("create a schema-scoped enum dependency and its same-named neighbor");

        // Act
        let blocked_drop = client
            .batch_execute("DROP TYPE compat_enum_drop_a.state_code")
            .await
            .expect_err("the dependent table column must block enum type deletion");
        client
            .batch_execute("DROP TYPE compat_enum_drop_a.state_code CASCADE")
            .await
            .expect("drop the enum and its dependent column with CASCADE");
        let cascade_state = client
            .query_one(
                "SELECT to_regtype('compat_enum_drop_a.state_code') IS NULL, \
                        to_regclass('compat_enum_drop_a.jobs') IS NOT NULL, \
                        (SELECT count(*) FROM pg_catalog.pg_attribute \
                         WHERE attrelid = 'compat_enum_drop_a.jobs'::regclass \
                           AND attnum > 0 AND NOT attisdropped), \
                        to_regtype('compat_enum_drop_b.state_code') IS NOT NULL",
                &[],
            )
            .await
            .expect("inspect dropped type and surviving relation state");

        // Assert
        assert_eq!(db_error(&blocked_drop).code().code(), "2BP01");
        assert!(cascade_state.get::<_, bool>(0));
        assert!(cascade_state.get::<_, bool>(1));
        assert_eq!(cascade_state.get::<_, i64>(2), 1);
        assert!(cascade_state.get::<_, bool>(3));

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_preserve_schema_enum_identity_after_local_storage_restart() {
    // Arrange
    support::use_local_storage();
    let path = support::data_dir("enum-identity-restart");
    let _directory = DataDirectory(path.clone());
    let before_restart = {
        let cassie = Cassie::new_with_data_dir(&path).expect("create local Cassie instance");
        cassie.startup().expect("start local Cassie instance");
        let session = cassie.create_session("tester", None);
        for statement in [
            "CREATE SCHEMA compat_enum_restart",
            "CREATE TYPE compat_enum_restart.state_code \
             AS ENUM ('waiting', 'ready', 'done')",
            "CREATE TABLE compat_enum_restart.jobs \
             (id INT PRIMARY KEY, state compat_enum_restart.state_code)",
            "INSERT INTO compat_enum_restart.jobs VALUES (1, 'waiting'), (2, 'done')",
            "ALTER TYPE compat_enum_restart.state_code \
             ADD VALUE 'paused' BEFORE 'ready'",
            "ALTER TYPE compat_enum_restart.state_code \
             RENAME VALUE 'done' TO 'complete'",
            "ALTER TYPE compat_enum_restart.state_code RENAME TO task_state",
            "INSERT INTO compat_enum_restart.jobs VALUES (3, 'paused')",
        ] {
            cassie
                .execute_sql(&session, statement, vec![])
                .unwrap_or_else(|error| {
                    panic!("execute enum lifecycle setup {statement}: {error}")
                });
        }
        let type_oid = cassie
            .execute_sql(
                &session,
                "SELECT oid::BIGINT FROM pg_catalog.pg_type \
                 WHERE oid = 'compat_enum_restart.task_state'::regtype",
                vec![],
            )
            .expect("read enum type identity before restart")
            .rows()[0][0]
            .clone();
        let array_oid = cassie
            .execute_sql(
                &session,
                "SELECT typarray::BIGINT FROM pg_catalog.pg_type \
                 WHERE oid = 'compat_enum_restart.task_state'::regtype",
                vec![],
            )
            .expect("read enum array type identity before restart")
            .rows()[0][0]
            .clone();
        let labels = cassie
            .execute_sql(
                &session,
                "SELECT enum_range(NULL::compat_enum_restart.task_state)::TEXT",
                vec![],
            )
            .expect("read enum labels before restart")
            .rows()
            .to_vec();
        let values = cassie
            .execute_sql(
                &session,
                "SELECT id, state::TEXT FROM compat_enum_restart.jobs ORDER BY state",
                vec![],
            )
            .expect("read persisted enum values before restart")
            .rows()
            .to_vec();
        drop(session);
        drop(cassie);
        (type_oid, array_oid, labels, values)
    };

    // Act
    let reopened = Cassie::new_with_data_dir(&path).expect("reopen local Cassie instance");
    reopened.startup().expect("recover local Cassie instance");
    let session = reopened.create_session("tester", None);
    let type_oid_after_restart = reopened
        .execute_sql(
            &session,
            "SELECT oid::BIGINT FROM pg_catalog.pg_type \
             WHERE oid = 'compat_enum_restart.task_state'::regtype",
            vec![],
        )
        .expect("read enum type identity after restart")
        .rows()[0][0]
        .clone();
    let array_oid_after_restart = reopened
        .execute_sql(
            &session,
            "SELECT typarray::BIGINT FROM pg_catalog.pg_type \
             WHERE oid = 'compat_enum_restart.task_state'::regtype",
            vec![],
        )
        .expect("read enum array type identity after restart")
        .rows()[0][0]
        .clone();
    let labels_after_restart = reopened
        .execute_sql(
            &session,
            "SELECT enum_range(NULL::compat_enum_restart.task_state)::TEXT",
            vec![],
        )
        .expect("read enum labels after restart")
        .rows()
        .to_vec();
    let values_after_restart = reopened
        .execute_sql(
            &session,
            "SELECT id, state::TEXT FROM compat_enum_restart.jobs ORDER BY state",
            vec![],
        )
        .expect("read persisted enum values after restart")
        .rows()
        .to_vec();

    // Assert
    assert_eq!(type_oid_after_restart, before_restart.0);
    assert_eq!(array_oid_after_restart, before_restart.1);
    assert_eq!(labels_after_restart, before_restart.2);
    assert_eq!(values_after_restart, before_restart.3);
    assert_eq!(
        labels_after_restart,
        vec![vec![cassie::types::Value::String(
            "{waiting,paused,ready,complete}".to_owned()
        )]]
    );
    assert_eq!(
        values_after_restart,
        vec![
            vec![
                cassie::types::Value::Int64(1),
                cassie::types::Value::String("waiting".to_owned()),
            ],
            vec![
                cassie::types::Value::Int64(3),
                cassie::types::Value::String("paused".to_owned()),
            ],
            vec![
                cassie::types::Value::Int64(2),
                cassie::types::Value::String("complete".to_owned()),
            ],
        ]
    );
}

#[test]
fn should_preserve_enum_identity_through_snapshot_restore() {
    // Arrange
    support::use_local_storage();
    let source_path = support::data_dir("enum-identity-snapshot-source");
    let snapshot_path = support::data_dir("enum-identity-snapshot");
    let restored_path = support::data_dir("enum-identity-snapshot-restored");
    let _source_directory = DataDirectory(source_path.clone());
    let _snapshot_directory = DataDirectory(snapshot_path.clone());
    let _restored_directory = DataDirectory(restored_path.clone());
    let identity_before_snapshot = {
        let cassie = Cassie::new_with_data_dir(&source_path).expect("create snapshot source");
        cassie.startup().expect("start snapshot source");
        let session = cassie.create_session("tester", None);
        for statement in [
            "CREATE SCHEMA compat_enum_snapshot",
            "CREATE TYPE compat_enum_snapshot.state_code \
             AS ENUM ('waiting', 'ready', 'done')",
            "CREATE TABLE compat_enum_snapshot.jobs \
             (id INT PRIMARY KEY, state compat_enum_snapshot.state_code)",
            "INSERT INTO compat_enum_snapshot.jobs VALUES (1, 'waiting'), (2, 'done')",
            "ALTER TYPE compat_enum_snapshot.state_code \
             ADD VALUE 'paused' BEFORE 'ready'",
            "ALTER TYPE compat_enum_snapshot.state_code \
             RENAME VALUE 'done' TO 'complete'",
            "ALTER TYPE compat_enum_snapshot.state_code RENAME TO task_state",
            "INSERT INTO compat_enum_snapshot.jobs VALUES (3, 'paused')",
        ] {
            cassie
                .execute_sql(&session, statement, vec![])
                .unwrap_or_else(|error| {
                    panic!("execute snapshot enum lifecycle setup {statement}: {error}")
                });
        }
        let identity = cassie
            .execute_sql(
                &session,
                "SELECT oid::BIGINT, typarray::BIGINT \
                 FROM pg_catalog.pg_type \
                 WHERE oid = 'compat_enum_snapshot.task_state'::regtype",
                vec![],
            )
            .expect("read enum and array identities before snapshot")
            .rows()[0]
            .clone();
        drop(session);
        drop(cassie);
        identity
    };
    cassie::app::Cassie::create_snapshot_from_data_dir(
        &source_path,
        &snapshot_path,
        cassie::app::CassieSnapshotOptions::default(),
    )
    .expect("export the enum catalog and values in a snapshot");
    cassie::app::Cassie::restore_snapshot(&snapshot_path, &restored_path)
        .expect("restore the enum snapshot into a new data directory");
    let restored = Cassie::new_with_data_dir(&restored_path).expect("open restored snapshot");
    restored.startup().expect("start restored snapshot");
    let session = restored.create_session("tester", None);

    // Act
    let identity_after_restore = restored
        .execute_sql(
            &session,
            "SELECT oid::BIGINT, typarray::BIGINT \
             FROM pg_catalog.pg_type \
             WHERE oid = 'compat_enum_snapshot.task_state'::regtype",
            vec![],
        )
        .expect("read enum and array identities after snapshot restore")
        .rows()[0]
        .clone();
    let labels_after_restore = restored
        .execute_sql(
            &session,
            "SELECT enum_range(NULL::compat_enum_snapshot.task_state)::TEXT",
            vec![],
        )
        .expect("read restored enum label order")
        .rows()
        .to_vec();
    let values_after_restore = restored
        .execute_sql(
            &session,
            "SELECT id, state::TEXT FROM compat_enum_snapshot.jobs ORDER BY state",
            vec![],
        )
        .expect("read restored rows in enum order")
        .rows()
        .to_vec();

    // Assert
    assert_eq!(identity_after_restore, identity_before_snapshot);
    assert_eq!(
        labels_after_restore,
        vec![vec![cassie::types::Value::String(
            "{waiting,paused,ready,complete}".to_owned()
        )]]
    );
    assert_eq!(
        values_after_restore,
        vec![
            vec![
                cassie::types::Value::Int64(1),
                cassie::types::Value::String("waiting".to_owned()),
            ],
            vec![
                cassie::types::Value::Int64(3),
                cassie::types::Value::String("paused".to_owned()),
            ],
            vec![
                cassie::types::Value::Int64(2),
                cassie::types::Value::String("complete".to_owned()),
            ],
        ]
    );
}
