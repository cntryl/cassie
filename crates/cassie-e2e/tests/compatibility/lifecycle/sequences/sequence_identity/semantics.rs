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
