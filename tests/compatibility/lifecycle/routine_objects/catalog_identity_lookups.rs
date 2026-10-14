use super::*;

#[test]
fn should_preserve_regclass_oid_across_catalog_lifecycle() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("regclass_search_path_lifecycle").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE SCHEMA compat_regclass_a",
                "CREATE SCHEMA compat_regclass_b",
                "CREATE TABLE compat_regclass_a.shared_relation (id INT)",
                "CREATE TABLE compat_regclass_b.shared_relation (id INT)",
                "CREATE TABLE compat_regclass_a.\"QuotedRelation\" (id INT)",
                "SET search_path TO compat_regclass_a, compat_regclass_b, public",
            ],
        )
        .await
        .expect("create shadowed relation names and select the first schema");

        // Act
        let first_schema = client
            .query_one(
                "SELECT 'shared_relation'::regclass = \
                          'compat_regclass_a.shared_relation'::regclass, \
                        to_regclass('shared_relation') = \
                          'compat_regclass_a.shared_relation'::regclass, \
                        to_regclass('\"compat_regclass_a\".\"QuotedRelation\"') = \
                          '\"compat_regclass_a\".\"QuotedRelation\"'::regclass, \
                        to_regclass('compat_regclass_a.missing_relation') IS NULL",
                &[],
            )
            .await
            .expect("resolve regclass names in the first search-path schema");
        let original_oid: u32 = client
            .query_one(
                "SELECT oid FROM pg_catalog.pg_class \
                 WHERE oid = 'compat_regclass_a.shared_relation'::regclass",
                &[],
            )
            .await
            .expect("read the relation's catalog OID")
            .get(0);
        client
            .batch_execute("SET search_path TO compat_regclass_b, compat_regclass_a, public")
            .await
            .expect("reverse search-path precedence");
        let second_schema = client
            .query_one(
                "SELECT to_regclass('shared_relation') = \
                          'compat_regclass_b.shared_relation'::regclass",
                &[],
            )
            .await
            .expect("resolve the shadowed relation from the new first schema");
        client
            .batch_execute("SET search_path TO compat_regclass_a, compat_regclass_b, public")
            .await
            .expect("restore the first schema for lifecycle checks");
        client
            .batch_execute(
                "BEGIN; ALTER TABLE compat_regclass_a.shared_relation \
                   RENAME TO rolled_back_relation; ROLLBACK",
            )
            .await
            .expect("roll back a relation rename");
        let after_rollback = client
            .query_one(
                "SELECT oid, to_regclass('compat_regclass_a.rolled_back_relation') IS NULL \
                 FROM pg_catalog.pg_class \
                 WHERE oid = 'compat_regclass_a.shared_relation'::regclass",
                &[],
            )
            .await
            .expect("resolve the original relation after rollback");
        client
            .batch_execute(
                "ALTER TABLE compat_regclass_a.shared_relation \
                   RENAME TO persisted_relation",
            )
            .await
            .expect("rename the relation while retaining its identity");
        let after_rename = client
            .query_one(
                "SELECT oid, to_regclass('compat_regclass_a.shared_relation') IS NULL \
                 FROM pg_catalog.pg_class \
                 WHERE oid = 'compat_regclass_a.persisted_relation'::regclass",
                &[],
            )
            .await
            .expect("resolve the renamed relation");
        client
            .batch_execute("DROP TABLE compat_regclass_a.persisted_relation")
            .await
            .expect("drop the renamed relation");
        let after_drop = client
            .query_one(
                "SELECT to_regclass('compat_regclass_a.persisted_relation') IS NULL",
                &[],
            )
            .await
            .expect("return NULL after dropping the relation");

        // Assert
        assert!(first_schema.get::<_, bool>(1));
        assert!(first_schema.get::<_, bool>(2));
        assert!(first_schema.get::<_, bool>(3));
        assert!(second_schema.get::<_, bool>(0));
        assert_eq!(after_rollback.get::<_, u32>(0), original_oid);
        assert!(after_rollback.get::<_, bool>(1));
        assert_eq!(after_rename.get::<_, u32>(0), original_oid);
        assert!(after_rename.get::<_, bool>(1));
        assert!(after_drop.get::<_, bool>(0));

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_resolve_regtype_by_search_path() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("regtype_regprocedure_search_path").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE SCHEMA compat_identity_a",
                "CREATE SCHEMA compat_identity_b",
                "CREATE TABLE compat_identity_a.shadow_kind (id INT)",
                "CREATE TABLE compat_identity_b.shadow_kind (id INT)",
                "CREATE TABLE compat_identity_a.\"CaseKind\" (id INT)",
                "SET search_path TO compat_identity_a, compat_identity_b, public",
            ],
        )
        .await
        .expect("create shadowed named types and select the first schema");

        // Act
        let first_schema = client
            .query_one(
                "SELECT 'shadow_kind'::regtype = \
                          'compat_identity_a.shadow_kind'::regtype, \
                        to_regtype('shadow_kind') = \
                          'compat_identity_a.shadow_kind'::regtype, \
                        to_regtype('\"compat_identity_a\".\"CaseKind\"') = \
                          '\"compat_identity_a\".\"CaseKind\"'::regtype",
                &[],
            )
            .await
            .expect("resolve type identities from the first schema");
        client
            .batch_execute("SET search_path TO compat_identity_b, compat_identity_a, public")
            .await
            .expect("reverse search-path precedence");
        let second_schema = client
            .query_one(
                "SELECT 'shadow_kind'::regtype = \
                          'compat_identity_b.shadow_kind'::regtype, \
                        to_regtype('shadow_kind') = \
                          'compat_identity_b.shadow_kind'::regtype",
                &[],
            )
            .await
            .expect("resolve type identities from the second schema");

        // Assert
        assert!(first_schema.get::<_, bool>(0));
        assert!(first_schema.get::<_, bool>(1));
        assert!(first_schema.get::<_, bool>(2));
        assert!(second_schema.get::<_, bool>(0));
        assert!(second_schema.get::<_, bool>(1));

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_refresh_prepared_regclass_lookup_after_relation_ddl() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("prepared_regclass_ddl_refresh").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE SCHEMA compat_regclass_cache_a",
                "CREATE SCHEMA compat_regclass_cache_b",
                "CREATE TABLE compat_regclass_cache_a.item (id INTEGER)",
                "CREATE TABLE compat_regclass_cache_b.item (id INTEGER)",
                "SET search_path TO compat_regclass_cache_b, \
                 compat_regclass_cache_a, public",
            ],
        )
        .await
        .expect("create shadowed relations and select schema B first");
        let lookup = client
            .prepare("SELECT to_regclass('item')::oid::bigint")
            .await
            .expect("prepare the regclass lookup once");

        // Act
        let first_lookup: i64 = client
            .query_one(&lookup, &[])
            .await
            .expect("resolve schema B through the prepared lookup")
            .get(0);
        let schema_b_oid: i64 = client
            .query_one(
                "SELECT 'compat_regclass_cache_b.item'::regclass::oid::bigint",
                &[],
            )
            .await
            .expect("read the original schema B relation identity")
            .get(0);
        client
            .batch_execute("ALTER TABLE compat_regclass_cache_b.item RENAME TO renamed_item")
            .await
            .expect("rename the relation currently found through search_path");
        let after_rename: i64 = client
            .query_one(&lookup, &[])
            .await
            .expect("resolve the next matching relation after rename")
            .get(0);
        let schema_a_oid: i64 = client
            .query_one(
                "SELECT 'compat_regclass_cache_a.item'::regclass::oid::bigint",
                &[],
            )
            .await
            .expect("read the fallback schema A relation identity")
            .get(0);
        client
            .batch_execute("ALTER TABLE compat_regclass_cache_b.renamed_item RENAME TO item")
            .await
            .expect("restore the schema B relation name");
        let after_rename_back: i64 = client
            .query_one(&lookup, &[])
            .await
            .expect("resolve the restored schema B relation name")
            .get(0);
        client
            .batch_execute("DROP TABLE compat_regclass_cache_b.item")
            .await
            .expect("drop the schema B relation");
        let after_drop: i64 = client
            .query_one(&lookup, &[])
            .await
            .expect("fall back after the schema B relation is dropped")
            .get(0);
        client
            .batch_execute("CREATE TABLE compat_regclass_cache_b.item (id INTEGER)")
            .await
            .expect("recreate the schema B relation name");
        let after_recreate: i64 = client
            .query_one(&lookup, &[])
            .await
            .expect("resolve the recreated relation through the same prepared query")
            .get(0);
        let recreated_schema_b_oid: i64 = client
            .query_one(
                "SELECT 'compat_regclass_cache_b.item'::regclass::oid::bigint",
                &[],
            )
            .await
            .expect("read the recreated schema B relation identity")
            .get(0);

        // Assert
        assert_eq!(first_lookup, schema_b_oid);
        assert_eq!(after_rename, schema_a_oid);
        assert_eq!(after_rename_back, schema_b_oid);
        assert_eq!(after_drop, schema_a_oid);
        assert_eq!(after_recreate, recreated_schema_b_oid);
        assert_ne!(recreated_schema_b_oid, schema_b_oid);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_resolve_builtin_regprocedure_overloads() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("regprocedure_builtin_overloads").await;
        let (client, connection) = server.connect().await;

        // Act
        let resolved = client
            .query_one(
                "SELECT to_regprocedure('abs(integer)') = \
                          'pg_catalog.abs(int4)'::regprocedure, \
                        to_regprocedure('abs(numeric)') = \
                          'pg_catalog.abs(numeric)'::regprocedure, \
                        to_regprocedure('abs(text)') IS NULL",
                &[],
            )
            .await
            .expect("resolve built-in routine overload identities");

        // Assert
        assert!(resolved.get::<_, bool>(0));
        assert!(resolved.get::<_, bool>(1));
        assert!(resolved.get::<_, bool>(2));

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_report_malformed_regprocedure_input_sqlstate() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("reg_identity_input_sqlstates").await;
        let (client, connection) = server.connect().await;
        // Act
        let error = client
            .query("SELECT 'compat_bad_signature('::regprocedure", &[])
            .await
            .expect_err("malformed regprocedure input must fail");

        // Assert
        assert_eq!(db_error(&error).code().code(), "22P02");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_report_oid_overflow_sqlstate() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("oid_overflow_sqlstate").await;
        let (client, connection) = server.connect().await;

        // Act
        let error = client
            .query("SELECT '4294967296'::oid", &[])
            .await
            .expect_err("out-of-range OID input must fail");

        // Assert
        assert_eq!(db_error(&error).code().code(), "22003");

        let recovered = client
            .query_one("SELECT 42", &[])
            .await
            .expect("connection must recover after OID overflow");
        assert_eq!(recovered.get::<_, i32>(0), 42);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_return_null_for_missing_regclass_lookup() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("reg_identity_nullable_missing").await;
        let (client, connection) = server.connect().await;

        // Act
        let missing = client
            .query_one("SELECT to_regclass('compat_missing_relation') IS NULL", &[])
            .await
            .expect("nullable to_reg* lookups must return a row for missing objects");
        let recovered = client
            .query_one("SELECT 42", &[])
            .await
            .expect("connection remains usable after nullable lookups");

        // Assert
        assert!(missing.get::<_, bool>(0));
        assert_eq!(recovered.get::<_, i32>(0), 42);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_return_null_for_missing_regtype_lookup() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("regtype_nullable_missing").await;
        let (client, connection) = server.connect().await;

        // Act
        let missing = client
            .query_one("SELECT to_regtype('compat_missing_type') IS NULL", &[])
            .await
            .expect("nullable to_regtype lookup returns a row for a missing type");

        // Assert
        assert!(missing.get::<_, bool>(0));

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_return_null_for_missing_regprocedure_lookup() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("regprocedure_nullable_missing").await;
        let (client, connection) = server.connect().await;

        // Act
        let missing = client
            .query_one(
                "SELECT to_regprocedure('compat_missing_function(integer)') IS NULL",
                &[],
            )
            .await
            .expect("nullable to_regprocedure lookup returns a row for a missing routine");

        // Assert
        assert!(missing.get::<_, bool>(0));

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_preserve_regtype_oid_through_persisted_lifecycle() {
    // Arrange
    support::use_local_storage();
    let path = support::data_dir("regtype_identity_restart");
    let _directory = DataDirectory(path.clone());
    let initial_identity = {
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie instance");
        cassie.startup().expect("start Cassie instance");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(&session, "CREATE SCHEMA compat_regtype_lifecycle", vec![])
            .expect("create schema for persistent identities");
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE compat_regtype_lifecycle.before_kind (id INT)",
                vec![],
            )
            .expect("create table-backed composite type for persistent identities");
        let type_oid = cassie
            .execute_sql(
                &session,
                "SELECT 'compat_regtype_lifecycle.before_kind'::regtype",
                vec![],
            )
            .expect("read original composite type identity")
            .rows()[0][0]
            .clone();
        let initial = type_oid;
        cassie
            .execute_sql(&session, "BEGIN", vec![])
            .expect("begin rename rollback");
        cassie
            .execute_sql(
                &session,
                "ALTER TABLE compat_regtype_lifecycle.before_kind RENAME TO rolled_back_kind",
                vec![],
            )
            .expect("rename type inside a transaction");
        cassie
            .execute_sql(&session, "ROLLBACK", vec![])
            .expect("roll back the type rename");
        let rolled_back_type_oid = cassie
            .execute_sql(
                &session,
                "SELECT 'compat_regtype_lifecycle.before_kind'::regtype",
                vec![],
            )
            .expect("resolve the original type identity after rollback")
            .rows()[0][0]
            .clone();
        cassie
            .execute_sql(
                &session,
                "ALTER TABLE compat_regtype_lifecycle.before_kind RENAME TO persisted_kind",
                vec![],
            )
            .expect("rename type while preserving its OID");
        let renamed_type_oid = cassie
            .execute_sql(
                &session,
                "SELECT 'compat_regtype_lifecycle.persisted_kind'::regtype",
                vec![],
            )
            .expect("read the renamed composite type OID")
            .rows()[0][0]
            .clone();
        assert_eq!(rolled_back_type_oid, initial);
        assert_eq!(renamed_type_oid, initial);
        drop(session);
        drop(cassie);
        initial
    };

    // Act
    let cassie = Cassie::new_with_data_dir(&path).expect("reopen Cassie instance");
    cassie.startup().expect("recover type identity");
    let session = cassie.create_session("tester", None);
    let after_restart_type_oid = cassie
        .execute_sql(
            &session,
            "SELECT 'compat_regtype_lifecycle.persisted_kind'::regtype",
            vec![],
        )
        .expect("resolve the renamed type after restart")
        .rows()[0][0]
        .clone();
    let after_restart = after_restart_type_oid;
    let regtype_lookup = cassie
        .execute_sql(
            &session,
            "SELECT to_regtype('compat_regtype_lifecycle.persisted_kind') = \
                    'compat_regtype_lifecycle.persisted_kind'::regtype",
            vec![],
        )
        .expect("resolve the persisted type through regtype input")
        .rows()
        .to_vec();
    cassie
        .execute_sql(
            &session,
            "DROP TABLE compat_regtype_lifecycle.persisted_kind",
            vec![],
        )
        .expect("drop the renamed type");
    let after_drop = cassie
        .execute_sql(
            &session,
            "SELECT to_regtype('compat_regtype_lifecycle.persisted_kind') IS NULL",
            vec![],
        )
        .expect("verify dropped-type and retained namespace lookups")
        .rows()
        .to_vec();

    // Assert
    assert_eq!(after_restart, initial_identity);
    assert_eq!(regtype_lookup, vec![vec![cassie::types::Value::Bool(true)]]);
    assert_eq!(after_drop, vec![vec![cassie::types::Value::Bool(true),]]);
}

#[test]
fn should_preserve_regnamespace_identity_across_restart() {
    // Arrange
    support::use_local_storage();
    let path = support::data_dir("regnamespace_identity_restart");
    let _directory = DataDirectory(path.clone());
    let original_namespace_oid = {
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie instance");
        cassie.startup().expect("start Cassie instance");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE SCHEMA compat_regnamespace_lifecycle",
                vec![],
            )
            .expect("create schema for persistent namespace identity");
        let identity = cassie
            .execute_sql(
                &session,
                "SELECT 'compat_regnamespace_lifecycle'::regnamespace",
                vec![],
            )
            .expect("read regnamespace identity before restart")
            .rows()[0][0]
            .clone();
        drop(session);
        drop(cassie);
        identity
    };

    // Act
    let cassie = Cassie::new_with_data_dir(&path).expect("reopen Cassie instance");
    cassie.startup().expect("recover namespace identity");
    let session = cassie.create_session("tester", None);
    let after_restart = cassie
        .execute_sql(
            &session,
            "SELECT 'compat_regnamespace_lifecycle'::regnamespace",
            vec![],
        )
        .expect("read regnamespace identity after restart")
        .rows()[0][0]
        .clone();

    // Assert
    assert_eq!(after_restart, original_namespace_oid);
}

#[test]
fn should_resolve_quoted_regprocedure_overloads_through_search_path() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("quoted_regprocedure_overloads").await;
        let (client, connection) = server.connect().await;

        // Act
        let selected = client
            .query_one(
                "SELECT to_regprocedure('\"abs\"(integer)')::text, \
                        to_regprocedure('\"abs\"(integer)')::oid = \
                          'pg_catalog.abs(int4)'::regprocedure::oid, \
                        to_regprocedure('\"pg_catalog\".\"abs\"(numeric)')::text, \
                        to_regprocedure('\"abs\"(integer)')::oid <> \
                          to_regprocedure('\"abs\"(numeric)')::oid, \
                        to_regprocedure('\"abs\"(text)') IS NULL",
                &[],
            )
            .await
            .expect("resolve quoted built-in overloads by exact argument type");

        // Assert
        assert_eq!(selected.get::<_, String>(0), "abs(integer)");
        assert!(selected.get::<_, bool>(1));
        assert_eq!(selected.get::<_, String>(2), "abs(numeric)");
        assert!(selected.get::<_, bool>(3));
        assert!(selected.get::<_, bool>(4));

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_report_missing_regclass_cast_with_undefined_table_sqlstate() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("missing_regclass_cast_sqlstate").await;
        let (client, connection) = server.connect().await;

        // Act
        let error = client
            .query("SELECT 'issue_818_missing_oid_relation'::regclass", &[])
            .await
            .expect_err("a missing regclass cast must raise undefined_table");

        // Assert
        assert_eq!(db_error(&error).code().code(), "42P01");
        let recovered = client
            .query_one("SELECT 42", &[])
            .await
            .expect("connection remains usable after a missing regclass input");
        assert_eq!(recovered.get::<_, i32>(0), 42);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_report_missing_regprocedure_cast_with_undefined_function_sqlstate() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("missing_regprocedure_cast_sqlstate").await;
        let (client, connection) = server.connect().await;

        // Act
        let error = client
            .query(
                "SELECT 'issue_818_missing_oid_function(integer)'::regprocedure",
                &[],
            )
            .await
            .expect_err("a missing regprocedure cast must raise undefined_function");

        // Assert
        assert_eq!(db_error(&error).code().code(), "42883");
        let recovered = client
            .query_one("SELECT 42", &[])
            .await
            .expect("connection remains usable after a missing regprocedure input");
        assert_eq!(recovered.get::<_, i32>(0), 42);

        drop(client);
        server.shutdown(connection).await;
    });
}
