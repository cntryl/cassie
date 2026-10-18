use super::*;

#[derive(Debug, PartialEq)]
struct EnumIdentityState {
    type_oid: cassie::types::Value,
    array_oid: cassie::types::Value,
    labels: Vec<Vec<cassie::types::Value>>,
    values: Vec<Vec<cassie::types::Value>>,
}

fn seed_enum_identity_fixture(
    cassie: &cassie::app::Cassie,
    session: &cassie::app::CassieSession,
    schema: &str,
) {
    for statement in [
        format!("CREATE SCHEMA {schema}"),
        format!("CREATE TYPE {schema}.state_code AS ENUM ('waiting', 'ready', 'done')"),
        format!("CREATE TABLE {schema}.jobs (id INT PRIMARY KEY, state {schema}.state_code)"),
        format!("INSERT INTO {schema}.jobs VALUES (1, 'waiting'), (2, 'done')"),
        format!("ALTER TYPE {schema}.state_code ADD VALUE 'paused' BEFORE 'ready'"),
        format!("ALTER TYPE {schema}.state_code RENAME VALUE 'done' TO 'complete'"),
        format!("ALTER TYPE {schema}.state_code RENAME TO task_state"),
        format!("INSERT INTO {schema}.jobs VALUES (3, 'paused')"),
    ] {
        cassie
            .execute_sql(session, &statement, vec![])
            .unwrap_or_else(|error| panic!("execute enum lifecycle setup {statement}: {error}"));
    }
}

fn read_enum_identity_state(
    cassie: &cassie::app::Cassie,
    session: &cassie::app::CassieSession,
    schema: &str,
) -> EnumIdentityState {
    let identity = cassie
        .execute_sql(
            session,
            &format!(
                "SELECT oid::BIGINT, typarray::BIGINT FROM pg_catalog.pg_type \
                 WHERE oid = '{schema}.task_state'::regtype"
            ),
            vec![],
        )
        .expect("read enum and array identities")
        .rows()[0]
        .clone();
    let labels = cassie
        .execute_sql(
            session,
            &format!("SELECT enum_range(NULL::{schema}.task_state)::TEXT"),
            vec![],
        )
        .expect("read enum label order")
        .rows()
        .to_vec();
    let values = cassie
        .execute_sql(
            session,
            &format!("SELECT id, state::TEXT FROM {schema}.jobs ORDER BY state"),
            vec![],
        )
        .expect("read persisted values in enum order")
        .rows()
        .to_vec();
    EnumIdentityState {
        type_oid: identity[0].clone(),
        array_oid: identity[1].clone(),
        labels,
        values,
    }
}

fn collect_enum_identity_state(path: &str, schema: &str, initialize: bool) -> EnumIdentityState {
    let cassie = Cassie::new_with_data_dir(path).expect("open local Cassie instance");
    cassie.startup().expect("start local Cassie instance");
    let session = cassie.create_session("tester", None);
    if initialize {
        seed_enum_identity_fixture(&cassie, &session, schema);
    }
    let state = read_enum_identity_state(&cassie, &session, schema);
    drop(session);
    drop(cassie);
    state
}

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
    let before_restart = collect_enum_identity_state(&path, "compat_enum_restart", true);

    // Act
    let after_restart = collect_enum_identity_state(&path, "compat_enum_restart", false);

    // Assert
    assert_eq!(after_restart, before_restart);
    assert_eq!(
        after_restart.labels,
        vec![vec![cassie::types::Value::String(
            "{waiting,paused,ready,complete}".to_owned()
        )]]
    );
    assert_eq!(
        after_restart.values,
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
    let before_snapshot = collect_enum_identity_state(&source_path, "compat_enum_snapshot", true);

    // Act
    cassie::app::Cassie::create_snapshot_from_data_dir(
        &source_path,
        &snapshot_path,
        cassie::app::CassieSnapshotOptions::default(),
    )
    .expect("export the enum catalog and values in a snapshot");
    cassie::app::Cassie::restore_snapshot(&snapshot_path, &restored_path)
        .expect("restore the enum snapshot into a new data directory");
    let after_restore = collect_enum_identity_state(&restored_path, "compat_enum_snapshot", false);

    // Assert
    assert_eq!(after_restore.type_oid, before_snapshot.type_oid);
    assert_eq!(after_restore.array_oid, before_snapshot.array_oid);
    assert_eq!(
        after_restore.labels,
        vec![vec![cassie::types::Value::String(
            "{waiting,paused,ready,complete}".to_owned()
        )]]
    );
    assert_eq!(
        after_restore.values,
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
