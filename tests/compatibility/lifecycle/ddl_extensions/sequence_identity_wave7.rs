use super::*;

mod remaining_contracts;
mod session_local_value_history;

#[test]
fn should_preserve_smallint_sequence_type_limits() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("smallint_sequence_contract").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute(
                "CREATE SEQUENCE compat_wave7_smallint_sequence AS SMALLINT \
                 START WITH 32766 MINVALUE 32766 MAXVALUE 32767 NO CYCLE",
            )
            .await
            .expect("create a bounded smallint sequence");

        // Act
        let first = client
            .query_one("SELECT nextval('compat_wave7_smallint_sequence')", &[])
            .await
            .expect("read the first smallint sequence value");
        let second: i64 = client
            .query_one("SELECT nextval('compat_wave7_smallint_sequence')", &[])
            .await
            .expect("read the maximum smallint sequence value")
            .try_get(0)
            .expect("second sequence value");
        let exhausted = client
            .query_one("SELECT nextval('compat_wave7_smallint_sequence')", &[])
            .await
            .expect_err("a noncycling sequence must stop at its maximum");
        let metadata = client
            .query_one(
                "SELECT data_type, start_value::text, minimum_value::text, \
                        maximum_value::text, increment::text, cycle_option \
                 FROM information_schema.sequences \
                 WHERE sequence_schema = 'public' \
                   AND sequence_name = 'compat_wave7_smallint_sequence'",
                &[],
            )
            .await
            .expect("read sequence type and option metadata");

        // Assert
        // PostgreSQL 18.6 keeps nextval's result as bigint (OID 20) for a smallint sequence.
        assert_eq!(first.get::<_, i64>(0), 32766);
        assert_eq!(first.columns()[0].type_().oid(), 20);
        assert_eq!(second, 32767);
        assert_eq!(db_error(&exhausted).code().code(), "2200H");
        assert_eq!(metadata.get::<_, String>(0), "smallint");
        assert_eq!(metadata.get::<_, String>(1), "32766");
        assert_eq!(metadata.get::<_, String>(2), "32766");
        assert_eq!(metadata.get::<_, String>(3), "32767");
        assert_eq!(metadata.get::<_, String>(4), "1");
        assert_eq!(metadata.get::<_, String>(5), "NO");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_change_an_existing_sequence_to_smallint_with_postgres_default_bounds() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("alter_sequence_type").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE SEQUENCE compat_wave7_alter_smallint_sequence")
            .await
            .expect("create a default bigint sequence");

        // Act
        client
            .batch_execute("ALTER SEQUENCE compat_wave7_alter_smallint_sequence AS SMALLINT")
            .await
            .expect("change the sequence data type to smallint");
        let metadata = client
            .query_one(
                "SELECT data_type, minimum_value::text, maximum_value::text \
                 FROM information_schema.sequences \
                 WHERE sequence_schema = 'public' \
                   AND sequence_name = 'compat_wave7_alter_smallint_sequence'",
                &[],
            )
            .await
            .expect("read the new type and default bounds");
        let next = client
            .query_one(
                "SELECT nextval('compat_wave7_alter_smallint_sequence')",
                &[],
            )
            .await
            .expect("read the sequence after changing its data type");

        // Assert
        assert_eq!(metadata.get::<_, String>(0), "smallint");
        assert_eq!(metadata.get::<_, String>(1), "1");
        assert_eq!(metadata.get::<_, String>(2), "32767");
        assert_eq!(next.get::<_, i64>(0), 1);
        assert_eq!(next.columns()[0].type_().oid(), 20);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_add_an_identity_to_an_existing_not_null_column() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("add_identity_to_column").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute(
                "CREATE TABLE compat_wave7_add_identity_rows (id INTEGER NOT NULL, label TEXT)",
            )
            .await
            .expect("create a table with an existing not-null key column");

        // Act
        client
            .batch_execute(
                "ALTER TABLE compat_wave7_add_identity_rows \
                 ALTER COLUMN id ADD GENERATED BY DEFAULT AS IDENTITY \
                 (START WITH 5 INCREMENT BY 3)",
            )
            .await
            .expect("add a configured identity to the existing column");
        let first: i32 = client
            .query_one(
                "INSERT INTO compat_wave7_add_identity_rows (label) \
                 VALUES ('generated-first') RETURNING id",
                &[],
            )
            .await
            .expect("generate the first identity value")
            .try_get(0)
            .expect("first identity value");
        let explicit: i32 = client
            .query_one(
                "INSERT INTO compat_wave7_add_identity_rows (id, label) \
                 VALUES (50, 'explicit') RETURNING id",
                &[],
            )
            .await
            .expect("accept an explicit value for BY DEFAULT identity")
            .try_get(0)
            .expect("explicit identity value");
        let second: i32 = client
            .query_one(
                "INSERT INTO compat_wave7_add_identity_rows (label) \
                 VALUES ('generated-second') RETURNING id",
                &[],
            )
            .await
            .expect("continue the identity sequence independently of explicit input")
            .try_get(0)
            .expect("second generated identity value");
        let descriptor = client
            .query_one(
                "SELECT is_identity, identity_generation \
                 FROM information_schema.columns \
                 WHERE table_schema = 'public' \
                   AND table_name = 'compat_wave7_add_identity_rows' \
                   AND column_name = 'id'",
                &[],
            )
            .await
            .expect("inspect the altered column identity descriptor");

        // Assert
        assert_eq!([first, explicit, second], [5, 50, 8]);
        assert_eq!(descriptor.get::<_, String>(0), "YES");
        assert_eq!(descriptor.get::<_, String>(1), "BY DEFAULT");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_accept_overriding_user_value_without_identity_columns() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("overriding_user_identity").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE compat_wave7_user_override_rows (id INTEGER, label TEXT)")
            .await
            .expect("create a table with ordinary columns");

        // Act
        let inserted = client
            .query_one(
                "INSERT INTO compat_wave7_user_override_rows (id, label) \
                 OVERRIDING USER VALUE VALUES (90, 'provided') RETURNING id, label",
                &[],
            )
            .await
            .expect("accept the SQL-standard override clause when no identity column exists");

        // Assert
        assert_eq!(inserted.get::<_, i32>(0), 90);
        assert_eq!(inserted.get::<_, String>(1), "provided");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_skip_drop_identity_if_exists_for_a_nonidentity_column() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("drop_identity_if_exists").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE compat_wave7_drop_identity_rows (id INTEGER, label TEXT)")
            .await
            .expect("create a table with an ordinary integer column");
        client
            .batch_execute("INSERT INTO compat_wave7_drop_identity_rows VALUES (7, 'kept')")
            .await
            .expect("insert a row before the conditional identity drop");

        // Act
        client
            .batch_execute(
                "ALTER TABLE compat_wave7_drop_identity_rows \
                 ALTER COLUMN id DROP IDENTITY IF EXISTS",
            )
            .await
            .expect("drop the identity property if it exists");
        client
            .batch_execute(
                "ALTER TABLE compat_wave7_drop_identity_rows \
                 ALTER COLUMN id DROP IDENTITY IF EXISTS",
            )
            .await
            .expect("allow the conditional drop to be repeated");
        let descriptor = client
            .query_one(
                "SELECT is_identity, is_nullable, identity_generation \
                 FROM information_schema.columns \
                 WHERE table_schema = 'public' \
                   AND table_name = 'compat_wave7_drop_identity_rows' \
                   AND column_name = 'id'",
                &[],
            )
            .await
            .expect("inspect the column after identity removal");
        let row = client
            .query_one("SELECT id, label FROM compat_wave7_drop_identity_rows", &[])
            .await
            .expect("read the row after the conditional identity drop");
        let sequence_name: Option<String> = client
            .query_one(
                "SELECT pg_get_serial_sequence(\
                   'compat_wave7_drop_identity_rows', 'id')",
                &[],
            )
            .await
            .expect("inspect the former identity sequence dependency")
            .try_get(0)
            .expect("sequence name or NULL");

        // Assert
        assert_eq!(descriptor.get::<_, String>(0), "NO");
        assert_eq!(descriptor.get::<_, String>(1), "YES");
        assert_eq!(descriptor.get::<_, Option<String>>(2), None);
        assert_eq!(sequence_name, None);
        assert_eq!(row.get::<_, i32>(0), 7);
        assert_eq!(row.get::<_, String>(1), "kept");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_rollback_sequence_restart_but_keep_setval_across_transaction_rollback() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("sequence_restart_setval_rollback").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE SEQUENCE compat_wave7_transactional_sequence START WITH 10")
            .await
            .expect("create a sequence with a known starting value");
        let first = client
            .query_one("SELECT nextval('compat_wave7_transactional_sequence')", &[])
            .await
            .expect("allocate the initial sequence value");

        // Act
        client
            .batch_execute("BEGIN")
            .await
            .expect("begin a transaction for ALTER SEQUENCE RESTART");
        client
            .batch_execute("ALTER SEQUENCE compat_wave7_transactional_sequence RESTART WITH 50")
            .await
            .expect("restart the sequence inside the transaction");
        let restarted = client
            .query_one("SELECT nextval('compat_wave7_transactional_sequence')", &[])
            .await
            .expect("read the transactional restart value");
        client
            .batch_execute("ROLLBACK")
            .await
            .expect("roll back the sequence restart");
        let last_after_restart_rollback = client
            .query_one(
                "SELECT currval('compat_wave7_transactional_sequence'), lastval()",
                &[],
            )
            .await
            .expect("retain the session's returned value after rollback");
        let next_after_restart_rollback = client
            .query_one("SELECT nextval('compat_wave7_transactional_sequence')", &[])
            .await
            .expect("continue from the pre-transaction sequence state");

        client
            .batch_execute("BEGIN")
            .await
            .expect("begin a transaction for setval");
        let set_value = client
            .query_one(
                "SELECT setval('compat_wave7_transactional_sequence', 80, true)",
                &[],
            )
            .await
            .expect("set the sequence value inside the transaction");
        client
            .batch_execute("ROLLBACK")
            .await
            .expect("roll back the transaction containing setval");
        let after_setval_rollback = client
            .query_one(
                "SELECT currval('compat_wave7_transactional_sequence'), \
                        lastval(), nextval('compat_wave7_transactional_sequence')",
                &[],
            )
            .await
            .expect("observe the nontransactional setval state");

        // Assert
        assert_eq!(first.get::<_, i64>(0), 10);
        assert_eq!(first.columns()[0].type_().oid(), 20);
        assert_eq!(restarted.get::<_, i64>(0), 50);
        assert_eq!(restarted.columns()[0].type_().oid(), 20);
        assert_eq!(
            (
                last_after_restart_rollback.get::<_, i64>(0),
                last_after_restart_rollback.get::<_, i64>(1),
                next_after_restart_rollback.get::<_, i64>(0),
            ),
            (50, 50, 11),
            "ALTER SEQUENCE RESTART rolls back while session value history remains"
        );
        assert_eq!(set_value.get::<_, i64>(0), 80);
        assert_eq!(set_value.columns()[0].type_().oid(), 20);
        assert_eq!(
            (
                after_setval_rollback.get::<_, i64>(0),
                after_setval_rollback.get::<_, i64>(1),
                after_setval_rollback.get::<_, i64>(2),
            ),
            (80, 80, 81),
            "setval remains effective after transaction rollback"
        );
        assert!(after_setval_rollback
            .columns()
            .iter()
            .all(|column| column.type_().oid() == 20));

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_resolve_regclass_sequence_names_through_the_active_search_path() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("sequence_regclass_search_path").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE SCHEMA compat_wave7_sequence_left",
                "CREATE SCHEMA compat_wave7_sequence_right",
                "CREATE SEQUENCE compat_wave7_sequence_left.counter START WITH 11",
                "CREATE SEQUENCE compat_wave7_sequence_right.counter START WITH 91",
                "SET search_path = compat_wave7_sequence_left, compat_wave7_sequence_right, public",
            ],
        )
        .await
        .expect("create same-named sequences and select the left schema first");
        let sequence_oids = client
            .query_one(
                "SELECT 'compat_wave7_sequence_left.counter'::regclass::oid, \
                        'compat_wave7_sequence_right.counter'::regclass::oid",
                &[],
            )
            .await
            .expect("resolve both explicitly qualified sequences");
        let left_oid: u32 = sequence_oids.try_get(0).expect("left sequence OID");
        let right_oid: u32 = sequence_oids.try_get(1).expect("right sequence OID");

        // Act
        let left = client
            .query_one(
                "SELECT 'counter'::regclass::oid, nextval('counter'::regclass), \
                        to_regclass('compat_wave7_missing_sequence') IS NULL",
                &[],
            )
            .await
            .expect("resolve and advance the first sequence on the search path");
        client
            .batch_execute(
                "SET search_path = compat_wave7_sequence_right, compat_wave7_sequence_left, public",
            )
            .await
            .expect("put the right schema first on the search path");
        let right = client
            .query_one(
                "SELECT 'counter'::regclass::oid, nextval('counter'::regclass), \
                        nextval('compat_wave7_sequence_left.counter'::regclass)",
                &[],
            )
            .await
            .expect("resolve the right sequence and explicitly address the left");
        let missing_cast = client
            .query_one("SELECT 'compat_wave7_missing_sequence'::regclass", &[])
            .await
            .expect_err("an unresolved regclass cast must fail");
        let missing_lookup = client
            .query_one(
                "SELECT to_regclass('compat_wave7_missing_sequence'), \
                        to_regclass('compat_wave7_missing_sequence') IS NULL",
                &[],
            )
            .await
            .expect("to_regclass returns NULL for a missing relation");

        // Assert
        assert_eq!(left.get::<_, u32>(0), left_oid);
        assert_eq!(left.columns()[0].type_().oid(), 26);
        assert_eq!(left.get::<_, i64>(1), 11);
        assert_eq!(left.columns()[1].type_().oid(), 20);
        assert!(left.get::<_, bool>(2));
        assert_eq!(right.get::<_, u32>(0), right_oid);
        assert_eq!(right.get::<_, i64>(1), 91);
        assert_eq!(right.get::<_, i64>(2), 12);
        assert_eq!(db_error(&missing_cast).code().code(), "42P01");
        assert_eq!(missing_lookup.columns()[0].type_().oid(), 2205);
        assert!(missing_lookup.get::<_, bool>(1));

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_enforce_sequence_function_privilege_requirements() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("sequence_privilege_matrix").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE ROLE compat_wave7_sequence_usage NOLOGIN",
                "CREATE ROLE compat_wave7_sequence_update NOLOGIN",
                "CREATE ROLE compat_wave7_sequence_select NOLOGIN",
                "CREATE SEQUENCE compat_wave7_privilege_sequence",
                "GRANT USAGE ON SEQUENCE compat_wave7_privilege_sequence TO compat_wave7_sequence_usage",
                "GRANT UPDATE ON SEQUENCE compat_wave7_privilege_sequence TO compat_wave7_sequence_update",
                "GRANT SELECT ON SEQUENCE compat_wave7_privilege_sequence TO compat_wave7_sequence_select",
            ],
        )
        .await
        .expect("create roles and grant distinct sequence privileges");

        // Act
        client
            .batch_execute("SET ROLE compat_wave7_sequence_usage")
            .await
            .expect("assume the USAGE-only role");
        let usage_next = client
            .query_one("SELECT nextval('compat_wave7_privilege_sequence')", &[])
            .await
            .expect("USAGE permits nextval");
        let usage_curr = client
            .query_one("SELECT currval('compat_wave7_privilege_sequence')", &[])
            .await
            .expect("USAGE permits currval");
        let usage_set_denied = client
            .query_one(
                "SELECT setval('compat_wave7_privilege_sequence', 40, true)",
                &[],
            )
            .await
            .expect_err("USAGE alone does not permit setval");
        client
            .batch_execute("RESET ROLE")
            .await
            .expect("restore the administrative role");
        client
            .batch_execute("SET ROLE compat_wave7_sequence_update")
            .await
            .expect("assume the UPDATE-only role");
        let update_next = client
            .query_one("SELECT nextval('compat_wave7_privilege_sequence')", &[])
            .await
            .expect("UPDATE permits nextval");
        let update_curr_denied = client
            .query_one("SELECT currval('compat_wave7_privilege_sequence')", &[])
            .await
            .expect_err("UPDATE alone does not permit currval");
        let update_set = client
            .query_one(
                "SELECT setval('compat_wave7_privilege_sequence', 20, true)",
                &[],
            )
            .await
            .expect("UPDATE permits setval");
        client
            .batch_execute("RESET ROLE")
            .await
            .expect("restore the administrative role");
        client
            .batch_execute("SET ROLE compat_wave7_sequence_select")
            .await
            .expect("assume the SELECT-only role");
        let select_next_denied = client
            .query_one("SELECT nextval('compat_wave7_privilege_sequence')", &[])
            .await
            .expect_err("SELECT alone does not permit nextval");
        let select_set_denied = client
            .query_one(
                "SELECT setval('compat_wave7_privilege_sequence', 40, true)",
                &[],
            )
            .await
            .expect_err("SELECT alone does not permit setval");
        client
            .batch_execute("RESET ROLE")
            .await
            .expect("restore the sequence owner");
        let owner_next = client
            .query_one("SELECT nextval('compat_wave7_privilege_sequence')", &[])
            .await
            .expect("read the sequence after role checks");

        // Assert
        assert_eq!(usage_next.get::<_, i64>(0), 1);
        assert_eq!(usage_next.columns()[0].type_().oid(), 20);
        assert_eq!(usage_curr.get::<_, i64>(0), 1);
        assert_eq!(usage_curr.columns()[0].type_().oid(), 20);
        assert_eq!(db_error(&usage_set_denied).code().code(), "42501");
        assert_eq!(update_next.get::<_, i64>(0), 2);
        assert_eq!(update_next.columns()[0].type_().oid(), 20);
        assert_eq!(db_error(&update_curr_denied).code().code(), "42501");
        assert_eq!(update_set.get::<_, i64>(0), 20);
        assert_eq!(update_set.columns()[0].type_().oid(), 20);
        assert_eq!(db_error(&select_next_denied).code().code(), "42501");
        assert_eq!(db_error(&select_set_denied).code().code(), "42501");
        assert_eq!(owner_next.get::<_, i64>(0), 21);
        assert_eq!(owner_next.columns()[0].type_().oid(), 20);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_allocate_distinct_sequence_values_across_concurrent_sessions() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("concurrent_sequence_allocation").await;
        let (left, left_connection) = server.connect().await;
        let (right, right_connection) = server.connect().await;
        left.batch_execute("CREATE SEQUENCE compat_wave7_concurrent_sequence")
            .await
            .expect("create one sequence shared by both sessions");

        // Act
        let left_allocations = async {
            let mut values = Vec::with_capacity(32);
            for _ in 0..32 {
                let row = left
                    .query_one("SELECT nextval('compat_wave7_concurrent_sequence')", &[])
                    .await?;
                assert_eq!(row.columns()[0].type_().oid(), 20);
                values.push(row.get::<_, i64>(0));
            }
            Ok::<_, tokio_postgres::Error>(values)
        };
        let right_allocations = async {
            let mut values = Vec::with_capacity(32);
            for _ in 0..32 {
                let row = right
                    .query_one("SELECT nextval('compat_wave7_concurrent_sequence')", &[])
                    .await?;
                assert_eq!(row.columns()[0].type_().oid(), 20);
                values.push(row.get::<_, i64>(0));
            }
            Ok::<_, tokio_postgres::Error>(values)
        };
        let (left_result, right_result) = tokio::join!(left_allocations, right_allocations);
        let mut values = left_result.expect("collect the first session's sequence values");
        values.extend(right_result.expect("collect the second session's sequence values"));
        values.sort_unstable();

        // Assert
        assert_eq!(values, (1_i64..=64).collect::<Vec<_>>());

        drop(left);
        drop(right);
        left_connection.abort();
        let _ = left_connection.await;
        server.shutdown(right_connection).await;
    });
}

#[test]
fn should_restore_sequence_state_after_server_restart() {
    // Arrange
    support::use_local_storage();
    let path = support::data_dir("sequence-allocation-restart");
    let _directory = DataDirectory(path.clone());
    let first_values = {
        let cassie = Cassie::new_with_data_dir(&path).expect("create local Cassie instance");
        cassie.startup().expect("start local Cassie instance");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE SEQUENCE compat_wave7_durable_sequence \
                 START WITH 41 INCREMENT BY 3 CACHE 1",
                vec![],
            )
            .expect("create a sequence with persistent options");
        let values = cassie
            .execute_sql(
                &session,
                "SELECT nextval('compat_wave7_durable_sequence'), \
                        nextval('compat_wave7_durable_sequence')",
                vec![],
            )
            .expect("allocate two sequence values before restart")
            .rows()
            .to_vec();
        drop(session);
        drop(cassie);
        values
    };

    // Act
    let reopened = Cassie::new_with_data_dir(&path).expect("reopen local Cassie instance");
    reopened.startup().expect("recover local Cassie instance");
    let session = reopened.create_session("tester", None);
    let recovered_value = reopened
        .execute_sql(
            &session,
            "SELECT nextval('compat_wave7_durable_sequence')",
            vec![],
        )
        .expect("allocate the next sequence value after restart");
    let recovered_options = reopened
        .execute_sql(
            &session,
            "SELECT seqstart, seqincrement, seqcache \
             FROM pg_catalog.pg_sequence \
             WHERE seqrelid = 'compat_wave7_durable_sequence'::regclass",
            vec![],
        )
        .expect("read sequence options after restart");

    // Assert
    assert_eq!(
        first_values,
        vec![vec![
            cassie::types::Value::Int64(41),
            cassie::types::Value::Int64(44),
        ]]
    );
    assert_eq!(
        recovered_value.rows(),
        &[vec![cassie::types::Value::Int64(47)]]
    );
    assert_eq!(
        recovered_options.rows(),
        &[vec![
            cassie::types::Value::Int64(41),
            cassie::types::Value::Int64(3),
            cassie::types::Value::Int64(1),
        ]]
    );
}
