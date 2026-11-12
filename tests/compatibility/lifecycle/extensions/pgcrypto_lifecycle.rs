use super::*;

#[test]
fn should_publish_pgcrypto_extension_transactionally() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("pgcrypto_transactional_install").await;
        let (client, connection) = server.connect().await;
        let (reader, reader_connection) = server.connect().await;
        client.batch_execute("CREATE SCHEMA ext_target; BEGIN; CREATE EXTENSION pgcrypto SCHEMA ext_target").await.expect("stage extension");

        // Act
        let own = client.query("SELECT extname, extversion, extrelocatable FROM pg_catalog.pg_extension WHERE extname = 'pgcrypto'", &[]).await.expect("own extension");
        let before = reader.query("SELECT extname FROM pg_catalog.pg_extension WHERE extname = 'pgcrypto'", &[]).await.expect("other session before commit");
        let digest = client.query_one("SELECT encode(ext_target.digest('abc', 'sha256'), 'hex'), ext_target.digest(NULL::text, 'sha256') IS NULL", &[]).await.expect("digest");
        client.batch_execute("COMMIT").await.expect("commit extension");
        let after = reader.query("SELECT extname FROM pg_catalog.pg_extension WHERE extname = 'pgcrypto'", &[]).await.expect("other session after commit");

        // Assert
        assert_eq!(own.len(), 1);
        assert_eq!(own[0].get::<_, String>(0), "pgcrypto");
        assert_eq!(own[0].get::<_, String>(1), "1.4");
        assert!(own[0].get::<_, bool>(2));
        assert!(before.is_empty());
        assert_eq!(after.len(), 1);
        assert_eq!(digest.get::<_, String>(0), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert!(digest.get::<_, bool>(1));
        drop(reader);
        drop(client);
        server.shutdown(connection).await;
        reader_connection.abort();
        let _ = reader_connection.await;
    });
}

#[test]
fn should_restore_pgcrypto_namespace_after_savepoint_rollback() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("pgcrypto_relocation_rollback").await;
        let (client, connection) = server.connect().await;
        execute_statements(&client, &[
            "CREATE SCHEMA ext_first", "CREATE SCHEMA ext_second", "CREATE EXTENSION pgcrypto SCHEMA ext_first",
            "BEGIN", "SAVEPOINT before_move", "ALTER EXTENSION pgcrypto SET SCHEMA ext_second",
        ]).await.expect("move extension");

        // Act
        let moved = client.query("SELECT extname FROM pg_catalog.pg_extension e JOIN pg_catalog.pg_namespace n ON n.oid = e.extnamespace WHERE e.extname = 'pgcrypto' AND n.nspname = 'ext_second'", &[]).await.expect("moved namespace");
        client.batch_execute("ROLLBACK TO before_move; COMMIT").await.expect("restore namespace");
        let restored = client.query("SELECT extname FROM pg_catalog.pg_extension e JOIN pg_catalog.pg_namespace n ON n.oid = e.extnamespace WHERE e.extname = 'pgcrypto' AND n.nspname = 'ext_first'", &[]).await.expect("restored namespace");
        client.batch_execute("DROP EXTENSION pgcrypto").await.expect("drop extension");
        let missing = client.query("SELECT ext_first.digest('abc', 'sha256')", &[]).await.expect_err("dropped routine");

        // Assert
        assert_eq!(moved.len(), 1);
        assert_eq!(restored.len(), 1);
        assert_eq!(db_error(&missing).code().code(), "42883");
        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_expose_only_selected_extension_versions() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("extension_selected_versions").await;
        let (client, connection) = server.connect().await;

        // Act
        let available = client.query("SELECT name, default_version, installed_version FROM pg_catalog.pg_available_extensions ORDER BY name", &[]).await.expect("available extensions");
        client.batch_execute("CREATE EXTENSION vector").await.expect("install vector identity");
        let installed = client.query("SELECT extname, extversion FROM pg_catalog.pg_extension WHERE extname = 'vector'", &[]).await.expect("vector identity");

        // Assert
        let rows = available.iter().map(|row| (row.get::<_, String>(0), row.get::<_, String>(1), row.get::<_, Option<String>>(2))).collect::<Vec<_>>();
        assert_eq!(rows, vec![("pgcrypto".to_owned(), "1.4".to_owned(), None), ("vector".to_owned(), "0.8.7".to_owned(), None)]);
        assert_eq!(installed.len(), 1);
        assert_eq!(installed[0].get::<_, String>(1), "0.8.7");
        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_track_extension_identity_through_relocation_lifecycle() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("extension_identity_dependency").await;
        let (client, connection) = server.connect().await;
        execute_statements(&client, &["CREATE SCHEMA ext_a", "CREATE SCHEMA ext_b", "CREATE EXTENSION pgcrypto SCHEMA ext_a"]).await.expect("install");
        let before = client.query_one("SELECT oid FROM pg_catalog.pg_extension WHERE extname = 'pgcrypto'", &[]).await.expect("extension OID").get::<_, u32>(0);
        let members = client.query("SELECT p.oid FROM pg_catalog.pg_depend d JOIN pg_catalog.pg_proc p ON p.oid = d.objid JOIN pg_catalog.pg_extension e ON e.oid = d.refobjid WHERE e.extname = 'pgcrypto' AND p.proname = 'digest' AND d.deptype = 'e'", &[]).await.expect("digest members");

        // Act
        client.batch_execute("ALTER EXTENSION pgcrypto SET SCHEMA ext_b").await.expect("relocate");
        let after = client.query_one("SELECT oid FROM pg_catalog.pg_extension WHERE extname = 'pgcrypto'", &[]).await.expect("same extension OID").get::<_, u32>(0);
        client.batch_execute("DROP EXTENSION pgcrypto").await.expect("drop");
        let remaining = client.query("SELECT p.oid FROM pg_catalog.pg_depend d JOIN pg_catalog.pg_proc p ON p.oid = d.objid WHERE p.proname = 'digest' AND d.deptype = 'e'", &[]).await.expect("remaining members");

        // Assert
        assert_ne!(before, 0);
        assert_eq!(after, before);
        assert_eq!(members.len(), 2);
        assert_ne!(members[0].get::<_, u32>(0), members[1].get::<_, u32>(0));
        assert!(remaining.is_empty());
        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_resolve_digest_bytea_overload_for_selected_algorithms() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("pgcrypto_digest_bytea").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE EXTENSION pgcrypto")
            .await
            .expect("install");

        // Act
        let statement = client
            .prepare("SELECT digest('\\x00ff'::bytea, 'sha256')")
            .await
            .expect("resolve bytea overload");
        let digest = client
            .query_one(
                "SELECT encode(digest('\\x00ff'::bytea, 'sha256'), 'hex')",
                &[],
            )
            .await
            .expect("digest bytes");
        let invalid = client
            .query("SELECT digest('abc', 'unsupported')", &[])
            .await
            .expect_err("unselected algorithm");

        // Assert
        assert_eq!(statement.columns()[0].type_().oid(), 17);
        assert_eq!(
            digest.get::<_, String>(0),
            "06eb7d6a69ee19e5fbdf749018d3d2abfa04bcbd1365db312eb86dc7169389b8"
        );
        assert_eq!(db_error(&invalid).code().code(), "22023");
        drop(client);
        server.shutdown(connection).await;
    });
}
