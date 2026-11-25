use super::*;

#[test]
fn should_publish_pgcrypto_extension_identity_after_installation() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("extension_catalog_publication").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE EXTENSION pgcrypto WITH SCHEMA public VERSION '1.4'")
            .await
            .expect("install the selected pgcrypto profile");
        client
            .batch_execute("CREATE EXTENSION IF NOT EXISTS pgcrypto VERSION '1.4'")
            .await
            .expect("repeat the selected pgcrypto installation");

        // Act
        let extension = client
            .query_one(
                "SELECT oid, extname, extowner::BIGINT, extnamespace::BIGINT, extrelocatable, extversion \
                 FROM pg_catalog.pg_extension WHERE extname = 'pgcrypto'",
                &[],
            )
            .await
            .expect("installed pgcrypto catalog row");
        let public_oid = client
            .query_one(
                "SELECT oid::BIGINT FROM pg_catalog.pg_namespace WHERE nspname = 'public'",
                &[],
            )
            .await
            .expect("public namespace identity")
            .get::<_, i64>(0);
        let available = client
            .query_one(
                "SELECT installed_version FROM pg_catalog.pg_available_extensions \
                 WHERE name = 'pgcrypto'",
                &[],
            )
            .await
            .expect("pgcrypto availability after install");

        // Assert
        assert_eq!(extension.get::<_, String>(1), "pgcrypto");
        assert_eq!(extension.get::<_, i64>(2), 10);
        assert_eq!(extension.get::<_, i64>(3), public_oid);
        assert!(extension.get::<_, bool>(4));
        assert_eq!(extension.get::<_, String>(5), "1.4");
        assert_eq!(available.get::<_, String>(0), "1.4");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_publish_digest_routine_catalog_rows_after_pgcrypto_installation() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("pgcrypto_routine_catalog_rows").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE EXTENSION pgcrypto WITH SCHEMA public VERSION '1.4'")
            .await
            .expect("install the selected pgcrypto profile");

        // Act
        let public_oid = client
            .query_one(
                "SELECT oid::BIGINT FROM pg_catalog.pg_namespace WHERE nspname = 'public'",
                &[],
            )
            .await
            .expect("public namespace identity")
            .get::<_, i64>(0);
        let routines = client
            .query(
                "SELECT oid::BIGINT, proname, pronamespace::BIGINT, prorettype::BIGINT \
                 FROM pg_catalog.pg_proc WHERE proname = 'digest' ORDER BY oid",
                &[],
            )
            .await
            .expect("registered pgcrypto digest identities");

        // Assert
        assert_eq!(routines.len(), 2);
        assert!(routines
            .iter()
            .all(|row| row.get::<_, String>(1) == "digest"));
        assert!(routines
            .iter()
            .all(|row| row.get::<_, i64>(2) == public_oid));
        assert!(routines.iter().all(|row| row.get::<_, i64>(3) == 17));
        assert_ne!(routines[0].get::<_, i64>(0), routines[1].get::<_, i64>(0));

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_install_pgcrypto_in_the_session_current_schema_when_schema_is_omitted() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("pgcrypto_default_schema").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE SCHEMA crypto_schema")
            .await
            .expect("create the selected current schema");
        client
            .batch_execute("SET search_path TO crypto_schema, public")
            .await
            .expect("select the extension's installation schema");

        // Act
        client
            .batch_execute("CREATE EXTENSION pgcrypto")
            .await
            .expect("install pgcrypto without an explicit schema");
        let installed_schema = client
            .query_one(
                "SELECT namespace.nspname \
                 FROM pg_catalog.pg_extension AS extension \
                 JOIN pg_catalog.pg_namespace AS namespace \
                   ON namespace.oid = extension.extnamespace \
                 WHERE extension.extname = 'pgcrypto'",
                &[],
            )
            .await
            .expect("read pgcrypto's selected installation schema")
            .get::<_, String>(0);
        let digest = client
            .query_one("SELECT encode(digest('abc', 'sha256'), 'hex')", &[])
            .await
            .expect("resolve unqualified digest through the selected search path")
            .get::<_, String>(0);

        // Assert
        assert_eq!(installed_schema, "crypto_schema");
        assert_eq!(
            digest,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_reject_duplicate_pgcrypto_installation_without_if_not_exists() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("duplicate_pgcrypto_extension").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE EXTENSION pgcrypto")
            .await
            .expect("install pgcrypto");

        // Act
        let error = client
            .batch_execute("CREATE EXTENSION pgcrypto")
            .await
            .expect_err("duplicate installation needs IF NOT EXISTS");
        let count = client
            .query_one(
                "SELECT count(*) FROM pg_catalog.pg_extension WHERE extname = 'pgcrypto'",
                &[],
            )
            .await
            .expect("read extension identity after duplicate install")
            .get::<_, i64>(0);

        // Assert
        assert_eq!(db_error(&error).code().code(), "42710");
        assert_eq!(count, 1);
        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_rollback_uncommitted_extension_catalog_state() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("extension_catalog_transaction").await;
        let (owner, owner_connection) = server.connect().await;
        let (observer, observer_connection) = server.connect().await;
        owner
            .batch_execute("BEGIN")
            .await
            .expect("begin the extension transaction");
        owner
            .batch_execute("CREATE EXTENSION pgcrypto")
            .await
            .expect("stage pgcrypto installation");

        // Act
        let owner_count = owner
            .query_one(
                "SELECT count(*) FROM pg_catalog.pg_extension WHERE extname = 'pgcrypto'",
                &[],
            )
            .await
            .expect("read the owner's staged extension")
            .get::<_, i64>(0);
        let observer_count = observer
            .query_one(
                "SELECT count(*) FROM pg_catalog.pg_extension WHERE extname = 'pgcrypto'",
                &[],
            )
            .await
            .expect("read the committed extension catalog")
            .get::<_, i64>(0);
        let owner_digest = owner
            .query_one("SELECT encode(public.digest('abc', 'sha256'), 'hex')", &[])
            .await
            .expect("resolve digest from the staged installation")
            .get::<_, String>(0);
        owner
            .batch_execute("ROLLBACK")
            .await
            .expect("discard the extension transaction");
        let after_rollback = observer
            .query_one(
                "SELECT count(*) FROM pg_catalog.pg_extension WHERE extname = 'pgcrypto'",
                &[],
            )
            .await
            .expect("read extension catalog after rollback")
            .get::<_, i64>(0);
        let digest_after_rollback = observer
            .query("SELECT public.digest('abc', 'sha256')", &[])
            .await
            .expect_err("rolled-back routine must no longer resolve");

        // Assert
        assert_eq!((owner_count, observer_count, after_rollback), (1, 0, 0));
        assert_eq!(
            owner_digest,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(db_error(&digest_after_rollback).code().code(), "42883");

        drop(owner);
        drop(observer);
        observer_connection.abort();
        let _ = observer_connection.await;
        server.shutdown(owner_connection).await;
    });
}

#[test]
fn should_resolve_sha256_for_selected_digest_inputs() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("pgcrypto_digest_sha256").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE EXTENSION pgcrypto")
            .await
            .expect("install pgcrypto");
        let statement = client
            .prepare(
                "SELECT public.digest('abc'::text, 'sha256'), \
                        public.digest(decode('616263', 'hex'), 'sha256'), \
                        public.digest(NULL::text, 'sha256'), \
                        public.digest(NULL::bytea, 'sha256')",
            )
            .await
            .expect("prepare selected digest overloads");

        // Act
        let row = client
            .query_one(&statement, &[])
            .await
            .expect("evaluate selected digest overloads");

        // Assert
        assert!(statement
            .columns()
            .iter()
            .all(|column| column.type_().oid() == 17));
        assert_eq!(
            row.get::<_, Vec<u8>>(0),
            vec![
                0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea, 0x41, 0x41, 0x40, 0xde, 0x5d, 0xae,
                0x22, 0x23, 0xb0, 0x03, 0x61, 0xa3, 0x96, 0x17, 0x7a, 0x9c, 0xb4, 0x10, 0xff, 0x61,
                0xf2, 0x00, 0x15, 0xad,
            ]
        );
        assert_eq!(row.get::<_, Vec<u8>>(0), row.get::<_, Vec<u8>>(1));
        assert_eq!(row.get::<_, Option<Vec<u8>>>(2), None);
        assert_eq!(row.get::<_, Option<Vec<u8>>>(3), None);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_reject_unselected_digest_algorithms_with_sqlstates() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("pgcrypto_digest_algorithm_boundary").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE EXTENSION pgcrypto")
            .await
            .expect("install pgcrypto");

        // Act
        for (sql, expected_state) in [
            ("SELECT public.digest('abc', 'md5')", "0A000"),
            ("SELECT public.digest('abc', 'not_a_hash')", "22023"),
        ] {
            let error = client
                .query(sql, &[])
                .await
                .expect_err("digest must reject algorithms outside the selected profile");

            // Assert
            assert_eq!(db_error(&error).code().code(), expected_state, "{sql}");
        }
        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_preserve_registered_digest_identity_after_extension_relocation() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("pgcrypto_schema_relocation").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE SCHEMA crypto_schema")
            .await
            .expect("create the target schema");
        client
            .batch_execute("CREATE EXTENSION pgcrypto")
            .await
            .expect("install pgcrypto");
        let extension_oid = client
            .query_one(
                "SELECT oid::BIGINT FROM pg_catalog.pg_extension WHERE extname = 'pgcrypto'",
                &[],
            )
            .await
            .expect("extension identity before relocation")
            .get::<_, i64>(0);

        // Act
        client
            .batch_execute("ALTER EXTENSION pgcrypto SET SCHEMA crypto_schema")
            .await
            .expect("relocate the extension to the selected schema");
        client
            .batch_execute("ALTER EXTENSION pgcrypto UPDATE TO '1.4'")
            .await
            .expect("same-version update is a no-op");
        let extension = client
            .query_one(
                "SELECT oid::BIGINT, extnamespace::BIGINT, extversion FROM pg_catalog.pg_extension \
                 WHERE extname = 'pgcrypto'",
                &[],
            )
            .await
            .expect("extension identity after relocation");
        let routines = client
            .query(
                "SELECT pronamespace::BIGINT FROM pg_catalog.pg_proc \
                 WHERE proname = 'digest' ORDER BY oid",
                &[],
            )
            .await
            .expect("relocated digest routine identities");
        let schema_oid = client
            .query_one(
                "SELECT oid::BIGINT FROM pg_catalog.pg_namespace WHERE nspname = 'crypto_schema'",
                &[],
            )
            .await
            .expect("relocated schema identity")
            .get::<_, i64>(0);
        let digest = client
            .query_one(
                "SELECT encode(crypto_schema.digest('abc', 'sha256'), 'hex')",
                &[],
            )
            .await
            .expect("resolve digest from its new schema")
            .get::<_, String>(0);

        // Assert
        assert_eq!(extension.get::<_, i64>(0), extension_oid);
        assert_eq!(extension.get::<_, i64>(1), schema_oid);
        assert_eq!(extension.get::<_, String>(2), "1.4");
        assert_eq!(routines.len(), 2);
        assert!(routines
            .iter()
            .all(|row| row.get::<_, i64>(0) == schema_oid));
        assert_eq!(
            digest,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_preserve_pgcrypto_dependencies_until_cascade() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("pgcrypto_drop_dependencies").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE EXTENSION pgcrypto")
            .await
            .expect("install pgcrypto");
        client
            .batch_execute(
                "CREATE VIEW digest_dependent AS \
                 SELECT encode(public.digest('abc', 'sha256'), 'hex') AS hash",
            )
            .await
            .expect("create a dependent view");

        // Act
        let restrict_error = client
            .batch_execute("DROP EXTENSION pgcrypto RESTRICT")
            .await
            .expect_err("RESTRICT must preserve the extension while a dependent exists");
        let extension_count = client
            .query_one(
                "SELECT count(*) FROM pg_catalog.pg_extension WHERE extname = 'pgcrypto'",
                &[],
            )
            .await
            .expect("read extension row after failed RESTRICT")
            .get::<_, i64>(0);
        let dependent_count = client
            .query_one(
                "SELECT count(*) FROM pg_catalog.pg_class WHERE relname = 'digest_dependent'",
                &[],
            )
            .await
            .expect("read dependent view after failed RESTRICT")
            .get::<_, i64>(0);
        // Assert
        assert_eq!(db_error(&restrict_error).code().code(), "2BP01");
        assert_eq!((extension_count, dependent_count), (1, 1));
        client
            .batch_execute("BEGIN")
            .await
            .expect("begin cascade rollback check");
        client
            .batch_execute("DROP EXTENSION pgcrypto CASCADE")
            .await
            .expect("stage extension and dependent removal");
        let staged = client
            .query_one(
                "SELECT (SELECT count(*) FROM pg_catalog.pg_extension WHERE extname = 'pgcrypto') + \
                        (SELECT count(*) FROM pg_catalog.pg_class WHERE relname = 'digest_dependent')",
                &[],
            )
            .await
            .expect("read staged extension and dependent visibility")
            .get::<_, i64>(0);
        client
            .batch_execute("ROLLBACK")
            .await
            .expect("roll back staged dependent removal");
        let restored = client
            .query_one(
                "SELECT (SELECT count(*) FROM pg_catalog.pg_extension WHERE extname = 'pgcrypto') + \
                        (SELECT count(*) FROM pg_catalog.pg_class WHERE relname = 'digest_dependent')",
                &[],
            )
            .await
            .expect("read restored extension and dependent visibility")
            .get::<_, i64>(0);
        assert_eq!((staged, restored), (0, 2));
        client
            .batch_execute("DROP EXTENSION pgcrypto CASCADE")
            .await
            .expect("CASCADE removes the actual dependent view");
        let after_cascade = client
            .query_one(
                "SELECT count(*) FROM pg_catalog.pg_class WHERE relname = 'digest_dependent'",
                &[],
            )
            .await
            .expect("read dependent catalog after CASCADE")
            .get::<_, i64>(0);
        let routine_count = client
            .query_one(
                "SELECT count(*) FROM pg_catalog.pg_proc WHERE proname = 'digest'",
                &[],
            )
            .await
            .expect("read routine catalog after CASCADE")
            .get::<_, i64>(0);
        assert_eq!((after_cascade, routine_count), (0, 0));

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_retain_digest_view_dependency_identity_after_pgcrypto_schema_relocation() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("pgcrypto_relocated_view_dependency").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE SCHEMA crypto_schema")
            .await
            .expect("create the extension relocation target");
        client
            .batch_execute("CREATE EXTENSION pgcrypto WITH SCHEMA public")
            .await
            .expect("install pgcrypto in public");
        client
            .batch_execute(
                "CREATE VIEW relocated_digest_dependent AS \
                 SELECT encode(public.digest('abc', 'sha256'), 'hex') AS hash",
            )
            .await
            .expect("create a view bound to the original digest identity");
        client
            .batch_execute("ALTER EXTENSION pgcrypto SET SCHEMA crypto_schema")
            .await
            .expect("relocate pgcrypto while preserving its routine identity");

        // Act
        let restrict_error = client
            .batch_execute("DROP EXTENSION pgcrypto RESTRICT")
            .await
            .expect_err("RESTRICT must retain dependencies resolved before relocation");
        let retained = client
            .query_one(
                "SELECT (SELECT count(*) FROM pg_catalog.pg_extension WHERE extname = 'pgcrypto') + \
                        (SELECT count(*) FROM pg_catalog.pg_class WHERE relname = 'relocated_digest_dependent')",
                &[],
            )
            .await
            .expect("read extension and view after RESTRICT")
            .get::<_, i64>(0);
        client
            .batch_execute("DROP EXTENSION pgcrypto CASCADE")
            .await
            .expect("CASCADE removes the relocated digest dependency");
        let view_count = client
            .query_one(
                "SELECT count(*) FROM pg_catalog.pg_class WHERE relname = 'relocated_digest_dependent'",
                &[],
            )
            .await
            .expect("read the view catalog after CASCADE")
            .get::<_, i64>(0);

        // Assert
        assert_eq!(db_error(&restrict_error).code().code(), "2BP01");
        assert_eq!(retained, 2);
        assert_eq!(view_count, 0);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_restore_pgcrypto_view_dependency_identity_after_local_restart() {
    // Arrange
    let path = support::data_dir("pgcrypto_view_dependency_restart");
    let _directory = DataDirectory(path.clone());
    {
        let cassie = Cassie::new_with_data_dir(&path).expect("create local Cassie");
        cassie.startup().expect("start local Cassie");
        let session = cassie.create_session("root", None);
        cassie
            .execute_sql(&session, "CREATE SCHEMA crypto_schema", Vec::new())
            .expect("create the extension relocation target");
        cassie
            .execute_sql(
                &session,
                "CREATE EXTENSION pgcrypto WITH SCHEMA public",
                Vec::new(),
            )
            .expect("install pgcrypto in public");
        cassie
            .execute_sql(
                &session,
                "CREATE VIEW relocated_digest_restart AS \
                 SELECT encode(public.digest('abc', 'sha256'), 'hex') AS hash",
                Vec::new(),
            )
            .expect("create a view bound to the original digest identity");
        cassie
            .execute_sql(
                &session,
                "ALTER EXTENSION pgcrypto SET SCHEMA crypto_schema",
                Vec::new(),
            )
            .expect("relocate pgcrypto before restart");
    }

    // Act
    let reopened = Cassie::new_with_data_dir(&path).expect("reopen local Cassie");
    reopened
        .startup()
        .expect("restore extension and view metadata");
    let session = reopened.create_session("root", None);
    let restrict_error = reopened
        .execute_sql(&session, "DROP EXTENSION pgcrypto RESTRICT", Vec::new())
        .expect_err("restored dependency identity must protect the extension");
    reopened
        .execute_sql(&session, "DROP EXTENSION pgcrypto CASCADE", Vec::new())
        .expect("cascade removes the restored dependent view");
    let view_count = reopened
        .execute_sql(
            &session,
            "SELECT count(*) FROM pg_catalog.pg_class WHERE relname = 'relocated_digest_restart'",
            Vec::new(),
        )
        .expect("read the dependent view after CASCADE")
        .rows()[0][0]
        .clone();

    // Assert
    assert!(matches!(
        restrict_error,
        cassie::app::CassieError::Diagnostic(error) if error.sql_state == "2BP01"
    ));
    assert_eq!(view_count, cassie::types::Value::Int64(0));
}

#[test]
fn should_restore_pgcrypto_catalog_state_after_local_restart() {
    // Arrange
    let path = support::data_dir("pgcrypto_extension_restart");
    let _directory = DataDirectory(path.clone());
    let extension_before_restart = {
        let cassie = Cassie::new_with_data_dir(&path).expect("create local Cassie");
        cassie.startup().expect("start local Cassie");
        let session = cassie.create_session("root", None);
        cassie
            .execute_sql(&session, "CREATE EXTENSION pgcrypto", Vec::new())
            .expect("install pgcrypto before restart");
        let extension = cassie
            .execute_sql(
                &session,
                "SELECT oid, extversion FROM pg_catalog.pg_extension \
                 WHERE extname = 'pgcrypto'",
                Vec::new(),
            )
            .expect("read persisted extension identity")
            .rows()
            .to_vec();
        drop(session);
        drop(cassie);
        extension
    };

    // Act
    let reopened = Cassie::new_with_data_dir(&path).expect("reopen local Cassie");
    reopened.startup().expect("restore extension metadata");
    let session = reopened.create_session("root", None);
    let after_restart = reopened
        .execute_sql(
            &session,
            "SELECT oid, extversion FROM pg_catalog.pg_extension \
             WHERE extname = 'pgcrypto'",
            Vec::new(),
        )
        .expect("read extension identity after restart")
        .rows()
        .to_vec();
    let digest = reopened
        .execute_sql(
            &session,
            "SELECT public.digest('abc', 'sha256')",
            Vec::new(),
        )
        .expect("resolve digest after restart")
        .rows()
        .to_vec();

    // Assert
    assert_eq!(after_restart, extension_before_restart);
    assert_eq!(
        digest,
        vec![vec![cassie::types::Value::String(
            "\\xba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad".to_owned()
        )]]
    );
}
