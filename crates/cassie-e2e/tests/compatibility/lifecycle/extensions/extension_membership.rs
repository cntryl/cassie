//! Selected extension membership in `pg_depend` (#1273).
use super::*;
use cassie::app::CassieSession;
use cassie::types::Value;

const DIGEST_MEMBERS: &str = "SELECT d.objid FROM pg_catalog.pg_depend d \
    JOIN pg_catalog.pg_extension e ON e.oid = d.refobjid \
    WHERE e.extname = 'pgcrypto' AND d.deptype = 'e' \
    AND d.objid IN (SELECT oid FROM pg_catalog.pg_proc WHERE proname = 'digest') \
    ORDER BY d.objid";

const EXTENSION_OID: &str = "SELECT oid FROM pg_catalog.pg_extension WHERE extname = 'pgcrypto'";

async fn oids(client: &tokio_postgres::Client, sql: &str) -> Vec<u32> {
    client
        .query(sql, &[])
        .await
        .expect("read catalog OIDs")
        .iter()
        .map(|row| row.get::<_, u32>(0))
        .collect()
}

fn run(cassie: &Cassie, session: &CassieSession, sql: &str) -> Vec<Vec<Value>> {
    match cassie.execute_sql(session, sql, vec![]) {
        Ok(output) => output.rows().to_vec(),
        Err(error) => panic!("statement failed with SQLSTATE {}", error.sql_state()),
    }
}

#[test]
fn should_publish_both_digest_overloads_as_extension_members() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("extension_membership_digest").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE SCHEMA ext_a",
                "CREATE EXTENSION pgcrypto SCHEMA ext_a",
            ],
        )
        .await
        .expect("install");

        // Act
        let members = oids(&client, DIGEST_MEMBERS).await;
        let shape = client
            .query(
                "SELECT d.classid, d.refclassid, d.objsubid, d.refobjsubid \
             FROM pg_catalog.pg_depend d WHERE d.deptype = 'e'",
                &[],
            )
            .await
            .expect("membership shape");

        // Assert
        assert_eq!(members.len(), 2);
        assert_ne!(members[0], members[1]);
        assert_eq!(shape.len(), 2);
        for row in &shape {
            assert_eq!(row.get::<_, u32>(0), 1255);
            assert_eq!(row.get::<_, u32>(1), 3079);
            assert_eq!(row.get::<_, i32>(2), 0);
            assert_eq!(row.get::<_, i32>(3), 0);
        }
        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_keep_extension_member_oids_stable_after_relocation() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("extension_membership_relocation").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE SCHEMA ext_a",
                "CREATE SCHEMA ext_b",
                "CREATE EXTENSION pgcrypto SCHEMA ext_a",
            ],
        )
        .await
        .expect("install");
        let members_before = oids(&client, DIGEST_MEMBERS).await;
        let extension_before = oids(&client, EXTENSION_OID).await;

        // Act
        client
            .batch_execute("ALTER EXTENSION pgcrypto SET SCHEMA ext_b")
            .await
            .expect("relocate");
        let members_after = oids(&client, DIGEST_MEMBERS).await;
        let extension_after = oids(&client, EXTENSION_OID).await;

        // Assert
        assert_eq!(members_before.len(), 2);
        assert_eq!(members_after, members_before);
        assert_eq!(extension_after, extension_before);
        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_allocate_fresh_member_identities_when_extension_is_recreated() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("extension_membership_drop_recreate").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE EXTENSION pgcrypto")
            .await
            .expect("install");
        let first = oids(&client, DIGEST_MEMBERS).await;

        // Act
        client
            .batch_execute("DROP EXTENSION pgcrypto")
            .await
            .expect("drop");
        let dropped = oids(
            &client,
            "SELECT objid FROM pg_catalog.pg_depend WHERE deptype = 'e'",
        )
        .await;
        client
            .batch_execute("CREATE EXTENSION pgcrypto")
            .await
            .expect("recreate");
        let second = oids(&client, DIGEST_MEMBERS).await;

        // Assert
        assert_eq!(first.len(), 2);
        assert_eq!(dropped, Vec::<u32>::new());
        assert_eq!(second.len(), 2);
        assert!(second.iter().all(|oid| !first.contains(oid)));
        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_roll_back_extension_members_with_create_extension() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("extension_membership_create_rollback").await;
        let (client, connection) = server.connect().await;
        let (reader, reader_connection) = server.connect().await;
        execute_statements(&client, &["BEGIN", "CREATE EXTENSION pgcrypto"])
            .await
            .expect("stage install");

        // Act
        let own = oids(&client, DIGEST_MEMBERS).await;
        let other = oids(&reader, DIGEST_MEMBERS).await;
        client
            .batch_execute("ROLLBACK")
            .await
            .expect("rollback install");
        let after = oids(&client, DIGEST_MEMBERS).await;

        // Assert
        assert_eq!(own.len(), 2);
        assert_eq!(other, Vec::<u32>::new());
        assert_eq!(after, Vec::<u32>::new());
        drop(reader);
        drop(client);
        reader_connection.abort();
        let _ = reader_connection.await;
        server.shutdown(connection).await;
    });
}

#[test]
fn should_restore_extension_members_after_drop_extension_rollback() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("extension_membership_drop_rollback").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE EXTENSION pgcrypto")
            .await
            .expect("install");
        let before = oids(&client, DIGEST_MEMBERS).await;
        execute_statements(&client, &["BEGIN", "DROP EXTENSION pgcrypto"])
            .await
            .expect("stage drop");

        // Act
        let during = oids(&client, DIGEST_MEMBERS).await;
        client
            .batch_execute("ROLLBACK")
            .await
            .expect("rollback drop");
        let after = oids(&client, DIGEST_MEMBERS).await;

        // Assert
        assert_eq!(before.len(), 2);
        assert_eq!(during, Vec::<u32>::new());
        assert_eq!(after, before);
        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_list_extension_members_for_driver_introspection() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("extension_membership_driver").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE SCHEMA ext_a",
                "CREATE EXTENSION pgcrypto SCHEMA ext_a",
            ],
        )
        .await
        .expect("install");

        // Act
        let rows = client
            .query(
                "SELECT e.extname, n.nspname, p.proname, d.deptype \
             FROM pg_catalog.pg_depend d \
             JOIN pg_catalog.pg_extension e ON d.refclassid = 3079 AND d.refobjid = e.oid \
             JOIN pg_catalog.pg_proc p ON d.classid = 1255 AND d.objid = p.oid \
             JOIN pg_catalog.pg_namespace n ON n.oid = p.pronamespace \
             WHERE d.deptype = 'e' ORDER BY p.oid",
                &[],
            )
            .await
            .expect("driver-style member listing");

        // Assert
        let listed = rows
            .iter()
            .map(|row| {
                (
                    row.get::<_, String>(0),
                    row.get::<_, String>(1),
                    row.get::<_, String>(2),
                    row.get::<_, String>(3),
                )
            })
            .collect::<Vec<_>>();
        let expected = (
            "pgcrypto".to_owned(),
            "ext_a".to_owned(),
            "digest".to_owned(),
            "e".to_owned(),
        );
        assert_eq!(listed, vec![expected.clone(), expected]);
        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_publish_vector_extension_members_that_join_their_catalog_rows() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("extension_membership_vector").await;
        let (client, connection) = server.connect().await;
        client.batch_execute("CREATE EXTENSION vector").await.expect("install");

        // Act
        let types = oids(&client, "SELECT t.oid FROM pg_catalog.pg_depend d JOIN pg_catalog.pg_type t ON d.classid = 1247 AND t.oid = d.objid JOIN pg_catalog.pg_extension e ON e.oid = d.refobjid WHERE e.extname = 'vector' AND t.typname = 'vector' AND d.deptype = 'e'").await;
        let operators = client.query("SELECT o.oprname FROM pg_catalog.pg_depend d JOIN pg_catalog.pg_operator o ON d.classid = 2617 AND o.oid = d.objid JOIN pg_catalog.pg_extension e ON e.oid = d.refobjid WHERE e.extname = 'vector' AND d.deptype = 'e' ", &[]).await.expect("operator members");
        let unjoined = client.query_one("SELECT count(*) FROM pg_catalog.pg_depend d WHERE d.deptype = 'e' AND d.refobjid NOT IN (SELECT oid FROM pg_catalog.pg_extension)", &[]).await.expect("dangling members").get::<_, i64>(0);

        // Assert
        let mut names = operators.iter().map(|row| row.get::<_, String>(0)).collect::<Vec<_>>();
        names.sort();
        assert_eq!(types.len(), 1);
        assert_eq!(names, vec!["<#>".to_owned(), "<->".to_owned(), "<=>".to_owned()]);
        assert_eq!(unjoined, 0);
        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_isolate_extension_members_between_databases() {
    // Arrange
    let cassie = support::new_in_memory_cassie().expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let postgres = cassie.create_session("root", Some("postgres".to_owned()));
    run(&cassie, &postgres, "CREATE DATABASE tenant_two");
    let tenant = cassie.create_session("root", Some("tenant_two".to_owned()));

    // Act
    run(&cassie, &postgres, "CREATE EXTENSION pgcrypto");
    let in_postgres = run(&cassie, &postgres, DIGEST_MEMBERS);
    let in_tenant = run(&cassie, &tenant, DIGEST_MEMBERS);
    run(&cassie, &tenant, "CREATE EXTENSION pgcrypto");
    let tenant_installed = run(&cassie, &tenant, DIGEST_MEMBERS);
    run(&cassie, &postgres, "DROP EXTENSION pgcrypto");
    let tenant_after_drop = run(&cassie, &tenant, DIGEST_MEMBERS);

    // Assert
    assert_eq!(in_postgres.len(), 2);
    assert_eq!(in_tenant, Vec::<Vec<Value>>::new());
    assert_eq!(tenant_installed.len(), 2);
    assert_ne!(tenant_installed, in_postgres);
    assert_eq!(tenant_after_drop, tenant_installed);
}

#[test]
fn should_keep_extension_members_after_restart() {
    // Arrange
    support::use_local_storage();
    let path = support::data_dir("extension_membership_restart");
    let _directory = DataDirectory(path.clone());
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie instance");
    cassie.startup().expect("start Cassie instance");
    let session = cassie.create_session("root", None);
    run(&cassie, &session, "CREATE EXTENSION pgcrypto");
    let before = run(&cassie, &session, DIGEST_MEMBERS);
    drop(session);
    drop(cassie);

    // Act
    let restarted = Cassie::new_with_data_dir(&path).expect("reopen Cassie instance");
    restarted.startup().expect("recover extension metadata");
    let session = restarted.create_session("root", None);
    let after = run(&restarted, &session, DIGEST_MEMBERS);

    // Assert
    assert_eq!(before.len(), 2);
    assert_eq!(after, before);
}
