use super::{cassie, create_table_and_role, root_session};
use cassie::app::{Cassie, CassieError, CassieSession};
use cassie::config::CassieRuntimeConfig;
use cassie::types::Value;

#[test]
fn should_reject_object_grants_inside_explicit_transactions() {
    // Arrange
    let cassie = cassie();
    let root = root_session(&cassie);
    create_table_and_role(&cassie, &root, "reader");
    cassie
        .execute_sql(&root, "BEGIN", Vec::new())
        .expect("begin explicit transaction");

    // Act
    let result = cassie.execute_sql(&root, "GRANT USAGE ON SCHEMA public TO reader", Vec::new());
    cassie
        .execute_sql(&root, "ROLLBACK", Vec::new())
        .expect("rollback explicit transaction");
    let privileges = cassie
        .execute_sql(
            &root,
            "SELECT has_schema_privilege('reader', 'public', 'USAGE')",
            Vec::new(),
        )
        .expect("inspect unchanged grant state after rollback");

    // Assert
    assert!(
        matches!(
            result,
            Err(CassieError::Transaction {
                kind: cassie::app::TransactionErrorKind::ActiveTransaction,
                ..
            })
        ),
        "expected object-grant transaction rejection, got {result:?}"
    );
    assert_eq!(privileges.rows(), vec![vec![Value::Bool(false)]]);
}

#[test]
fn should_revoke_existing_sessions_without_role_switching() {
    // Arrange
    let cassie = cassie();
    let root = root_session(&cassie);
    let reader = create_table_and_role(&cassie, &root, "reader");
    cassie
        .execute_sql(&root, "GRANT USAGE ON SCHEMA public TO reader", Vec::new())
        .expect("grant schema usage");
    cassie
        .execute_sql(
            &root,
            "GRANT SELECT ON TABLE public.authorization_documents TO reader",
            Vec::new(),
        )
        .expect("grant select");

    // Act
    let before_revoke = cassie.execute_sql(
        &reader,
        "SELECT id FROM public.authorization_documents",
        Vec::new(),
    );
    cassie
        .execute_sql(
            &root,
            "REVOKE SELECT ON TABLE public.authorization_documents FROM reader",
            Vec::new(),
        )
        .expect("revoke select");
    let after_revoke = cassie.execute_sql(
        &reader,
        "SELECT id FROM public.authorization_documents",
        Vec::new(),
    );
    let role_switch = cassie.execute_sql(&reader, "SET ROLE root", Vec::new());

    // Assert
    assert!(before_revoke.is_ok());
    assert!(matches!(
        after_revoke,
        Err(CassieError::InsufficientPrivilege)
    ));
    assert!(role_switch.is_err());
}

#[test]
fn should_remove_table_grants_only_after_transactional_drop_commits() {
    // Arrange
    let cassie = cassie();
    let root = root_session(&cassie);
    let reader = create_table_and_role(&cassie, &root, "reader");
    cassie
        .execute_sql(&root, "GRANT USAGE ON SCHEMA public TO reader", Vec::new())
        .expect("grant schema usage");
    cassie
        .execute_sql(
            &root,
            "GRANT SELECT ON TABLE public.authorization_documents TO reader",
            Vec::new(),
        )
        .expect("grant table select");
    let database_family = cassie
        .catalog
        .get_database("postgres")
        .expect("database metadata")
        .physical_family;
    let table_id = cassie
        .catalog
        .get_collection("public.authorization_documents")
        .expect("table metadata")
        .storage_id;

    // Act
    cassie
        .execute_sql(&root, "BEGIN", Vec::new())
        .expect("begin rollback transaction");
    cassie
        .execute_sql(
            &root,
            "DROP TABLE public.authorization_documents",
            Vec::new(),
        )
        .expect("stage table drop before rollback");
    let reader_before_rollback = cassie.execute_sql(
        &reader,
        "SELECT body FROM public.authorization_documents WHERE id = 1",
        Vec::new(),
    );
    let grants_before_rollback = cassie
        .midge
        .list_role_object_grants()
        .expect("list grants before rollback");
    cassie
        .execute_sql(&root, "ROLLBACK", Vec::new())
        .expect("rollback table drop");
    let reader_after_rollback = cassie.execute_sql(
        &reader,
        "SELECT body FROM public.authorization_documents WHERE id = 1",
        Vec::new(),
    );

    cassie
        .execute_sql(&root, "BEGIN", Vec::new())
        .expect("begin commit transaction");
    cassie
        .execute_sql(
            &root,
            "DROP TABLE public.authorization_documents",
            Vec::new(),
        )
        .expect("stage table drop before commit");
    cassie
        .execute_sql(&root, "COMMIT", Vec::new())
        .expect("commit table drop");
    let grants_after_commit = cassie
        .midge
        .list_role_object_grants()
        .expect("list grants after commit");
    cassie
        .execute_sql(
            &root,
            "CREATE TABLE public.authorization_documents (id INTEGER PRIMARY KEY, body TEXT)",
            Vec::new(),
        )
        .expect("create a new table with the dropped name");
    let reader_after_recreate = cassie.execute_sql(
        &reader,
        "SELECT body FROM public.authorization_documents",
        Vec::new(),
    );

    // Assert
    assert!(reader_before_rollback.is_ok());
    assert!(grants_before_rollback.iter().any(|grant| {
        grant.database_family == database_family
            && grant.target == cassie::catalog::RoleGrantTarget::Table(table_id)
    }));
    assert_eq!(
        reader_after_rollback
            .expect("reader access remains after rollback")
            .rows(),
        vec![vec![Value::String("original".to_owned())]]
    );
    assert!(grants_after_commit.iter().all(|grant| {
        grant.database_family != database_family
            || grant.target != cassie::catalog::RoleGrantTarget::Table(table_id)
    }));
    assert!(matches!(
        reader_after_recreate,
        Err(CassieError::InsufficientPrivilege)
    ));
}

#[test]
fn should_remove_cascaded_object_grants_only_after_schema_drop_commits() {
    // Arrange
    let SchemaGrantFixture {
        cassie,
        root,
        reader,
        database_family,
        schema_id,
        table_id,
    } = schema_grant_fixture();

    // Act
    cassie
        .execute_sql(&root, "BEGIN", Vec::new())
        .expect("begin rollback transaction");
    cassie
        .execute_sql(&root, "DROP SCHEMA grant_cleanup CASCADE", Vec::new())
        .expect("stage schema drop before rollback");
    let grants_before_rollback = cassie
        .midge
        .list_role_object_grants()
        .expect("list grants before rollback");
    cassie
        .execute_sql(&root, "ROLLBACK", Vec::new())
        .expect("rollback schema drop");
    let reader_after_rollback = cassie.execute_sql(
        &reader,
        "SELECT body FROM grant_cleanup.documents WHERE id = 1",
        Vec::new(),
    );

    cassie
        .execute_sql(&root, "BEGIN", Vec::new())
        .expect("begin commit transaction");
    cassie
        .execute_sql(&root, "DROP SCHEMA grant_cleanup CASCADE", Vec::new())
        .expect("stage schema drop before commit");
    cassie
        .execute_sql(&root, "COMMIT", Vec::new())
        .expect("commit schema drop");
    let grants_after_commit = cassie
        .midge
        .list_role_object_grants()
        .expect("list grants after commit");
    cassie
        .execute_sql(&root, "CREATE SCHEMA grant_cleanup", Vec::new())
        .expect("recreate schema with the dropped name");
    cassie
        .execute_sql(
            &root,
            "CREATE TABLE grant_cleanup.documents (id INTEGER PRIMARY KEY, body TEXT)",
            Vec::new(),
        )
        .expect("recreate table with the dropped name");
    let reader_after_recreate = cassie.execute_sql(
        &reader,
        "SELECT body FROM grant_cleanup.documents",
        Vec::new(),
    );

    // Assert
    assert!(grants_before_rollback.iter().any(|grant| {
        grant.database_family == database_family
            && grant.target == cassie::catalog::RoleGrantTarget::Schema(schema_id)
    }));
    assert!(grants_before_rollback.iter().any(|grant| {
        grant.database_family == database_family
            && grant.target == cassie::catalog::RoleGrantTarget::Table(table_id)
    }));
    assert_eq!(
        reader_after_rollback
            .expect("reader access remains after rollback")
            .rows(),
        vec![vec![Value::String("original".to_owned())]]
    );
    assert!(grants_after_commit.iter().all(|grant| {
        grant.database_family != database_family
            || (grant.target != cassie::catalog::RoleGrantTarget::Schema(schema_id)
                && grant.target != cassie::catalog::RoleGrantTarget::Table(table_id))
    }));
    assert!(matches!(
        reader_after_recreate,
        Err(CassieError::InsufficientPrivilege)
    ));
}

struct SchemaGrantFixture {
    cassie: Cassie,
    root: CassieSession,
    reader: CassieSession,
    database_family: String,
    schema_id: u64,
    table_id: u64,
}

fn schema_grant_fixture() -> SchemaGrantFixture {
    let cassie = cassie();
    let root = root_session(&cassie);
    cassie
        .execute_sql(&root, "CREATE SCHEMA grant_cleanup", Vec::new())
        .expect("create grant cleanup schema");
    cassie
        .execute_sql(
            &root,
            "CREATE TABLE grant_cleanup.documents (id INTEGER PRIMARY KEY, body TEXT)",
            Vec::new(),
        )
        .expect("create grant cleanup table");
    cassie
        .execute_sql(
            &root,
            "INSERT INTO grant_cleanup.documents VALUES (1, 'original')",
            Vec::new(),
        )
        .expect("insert grant cleanup fixture row");
    cassie
        .execute_sql(
            &root,
            "CREATE ROLE schema_reader LOGIN PASSWORD 'schema-reader-secret'",
            Vec::new(),
        )
        .expect("create schema reader");
    let reader = cassie
        .authenticate_role(
            "schema_reader",
            Some("schema-reader-secret"),
            Some("postgres".to_owned()),
        )
        .expect("authenticate schema reader");
    cassie
        .execute_sql(
            &root,
            "GRANT USAGE ON SCHEMA grant_cleanup TO schema_reader",
            Vec::new(),
        )
        .expect("grant schema usage");
    cassie
        .execute_sql(
            &root,
            "GRANT SELECT ON TABLE grant_cleanup.documents TO schema_reader",
            Vec::new(),
        )
        .expect("grant table select");
    let database_family = cassie
        .midge
        .get_database("postgres")
        .expect("database lookup")
        .expect("database metadata")
        .physical_family;
    let schema_id = cassie
        .catalog
        .get_namespace("grant_cleanup")
        .expect("schema metadata")
        .object_id;
    let table_id = cassie
        .catalog
        .get_collection("grant_cleanup.documents")
        .expect("table metadata")
        .storage_id;
    SchemaGrantFixture {
        cassie,
        root,
        reader,
        database_family,
        schema_id,
        table_id,
    }
}

#[test]
fn should_reconcile_stale_table_grants_after_restart() {
    // Arrange
    let directory = crate::support_temp_dirs::IsolatedDataDir::new("stale-role-grant-restart");
    let config = crate::support_sql::with_test_root_password(CassieRuntimeConfig::default());
    let cassie = Cassie::new_local_with_data_dir_and_config(directory.path(), config.clone())
        .expect("local Cassie");
    cassie.startup().expect("startup");
    let root = root_session(&cassie);
    create_table_and_role(&cassie, &root, "reader");
    cassie
        .execute_sql(&root, "GRANT USAGE ON SCHEMA public TO reader", Vec::new())
        .expect("grant schema usage");
    cassie
        .execute_sql(
            &root,
            "GRANT SELECT ON TABLE public.authorization_documents TO reader",
            Vec::new(),
        )
        .expect("grant table select");
    let table_grant = cassie
        .midge
        .list_role_object_grants()
        .expect("list current grants")
        .into_iter()
        .find(|grant| matches!(grant.target, cassie::catalog::RoleGrantTarget::Table(_)))
        .expect("table grant");
    cassie
        .execute_sql(
            &root,
            "DROP TABLE public.authorization_documents",
            Vec::new(),
        )
        .expect("drop table before stale grant fixture");

    // Act
    cassie
        .midge
        .update_role_object_grant(
            &table_grant,
            &[cassie::catalog::RoleObjectPrivilege::Select],
            true,
        )
        .expect("seed an inert grant left by interrupted postcommit cleanup");
    assert_eq!(
        cassie
            .midge
            .list_role_object_grants()
            .expect("observe stale grant")
            .len(),
        2
    );
    cassie.shutdown();
    drop(cassie);

    let restarted = Cassie::new_local_with_data_dir_and_config(directory.path(), config)
        .expect("reopened Cassie");
    restarted.startup().expect("restart startup");
    let grants_after_restart = restarted
        .midge
        .list_role_object_grants()
        .expect("list reconciled grants");

    // Assert
    assert_eq!(grants_after_restart.len(), 1);
    assert!(matches!(
        grants_after_restart[0].target,
        cassie::catalog::RoleGrantTarget::Schema(_)
    ));
}

#[test]
fn should_recover_grants_after_local_restart() {
    // Arrange
    let directory = crate::support_temp_dirs::IsolatedDataDir::new("role-grant-restart");
    let config = crate::support_sql::with_test_root_password(CassieRuntimeConfig::default());
    let cassie = Cassie::new_local_with_data_dir_and_config(directory.path(), config.clone())
        .expect("local Cassie");
    cassie.startup().expect("startup");
    let root = root_session(&cassie);
    let reader = create_table_and_role(&cassie, &root, "reader");
    cassie
        .execute_sql(&root, "GRANT USAGE ON SCHEMA public TO reader", Vec::new())
        .expect("grant schema usage");
    cassie
        .execute_sql(
            &root,
            "GRANT SELECT ON TABLE public.authorization_documents TO reader",
            Vec::new(),
        )
        .expect("grant select");
    drop(reader);
    cassie.shutdown();
    drop(cassie);

    // Act
    let restarted = Cassie::new_local_with_data_dir_and_config(directory.path(), config)
        .expect("reopened Cassie");
    restarted.startup().expect("restart startup");
    let reader = restarted
        .authenticate_role("reader", Some("reader-secret"), Some("postgres".to_owned()))
        .expect("reader login after restart");
    let result = restarted.execute_sql(
        &reader,
        "SELECT body FROM public.authorization_documents WHERE id = 1",
        Vec::new(),
    );
    let acl = restarted
        .execute_sql(
            &reader,
            "SELECT namespace.nspacl, relation.relacl \
             FROM pg_catalog.pg_namespace AS namespace \
             CROSS JOIN pg_catalog.pg_class AS relation \
             WHERE namespace.nspname = 'public' \
               AND relation.relname = 'authorization_documents'",
            Vec::new(),
        )
        .expect("read restored ACL catalog values");

    // Assert
    assert_eq!(
        result.expect("reader select after restart").rows(),
        vec![vec![Value::String("original".to_owned())]]
    );
    assert_eq!(
        acl.rows(),
        vec![vec![
            Value::String("{reader=U/root}".to_owned()),
            Value::String("{reader=r/root}".to_owned()),
        ]]
    );
}

#[test]
fn should_retain_table_grant_identity_until_drop() {
    // Arrange
    let cassie = cassie();
    let root = root_session(&cassie);
    let reader = create_table_and_role(&cassie, &root, "reader");
    cassie
        .execute_sql(&root, "GRANT USAGE ON SCHEMA public TO reader", Vec::new())
        .expect("grant schema usage");
    cassie
        .execute_sql(
            &root,
            "GRANT SELECT ON TABLE public.authorization_documents TO reader",
            Vec::new(),
        )
        .expect("grant table select");

    // Act
    cassie
        .execute_sql(
            &root,
            "ALTER TABLE public.authorization_documents RENAME TO renamed_authorization_documents",
            Vec::new(),
        )
        .expect("rename granted table");
    let after_rename = cassie.execute_sql(
        &reader,
        "SELECT body FROM public.renamed_authorization_documents",
        Vec::new(),
    );
    cassie
        .execute_sql(
            &root,
            "DROP TABLE public.renamed_authorization_documents",
            Vec::new(),
        )
        .expect("drop granted table");
    let grants_after_drop = cassie
        .midge
        .list_role_object_grants()
        .expect("list grant records after drop");

    // Assert
    assert_eq!(
        after_rename
            .expect("table grant follows stable identity")
            .rows(),
        vec![vec![Value::String("original".to_owned())]]
    );
    assert!(grants_after_drop
        .iter()
        .all(|grant| !matches!(&grant.target, cassie::catalog::RoleGrantTarget::Table(_))));
}

#[test]
fn should_remove_schema_role_grants_on_drop() {
    // Arrange
    let cassie = cassie();
    let root = root_session(&cassie);
    cassie
        .execute_sql(
            &root,
            "CREATE ROLE grant_cleanup LOGIN PASSWORD 'grant-cleanup-secret'",
            Vec::new(),
        )
        .expect("create role");
    cassie
        .execute_sql(&root, "CREATE SCHEMA grant_cleanup_schema", Vec::new())
        .expect("create schema");
    cassie
        .execute_sql(
            &root,
            "GRANT USAGE ON SCHEMA grant_cleanup_schema TO grant_cleanup",
            Vec::new(),
        )
        .expect("grant schema usage");
    let schema_grants = cassie
        .midge
        .list_role_object_grants()
        .expect("list schema grants");
    assert_eq!(schema_grants.len(), 1);

    // Act
    cassie
        .execute_sql(&root, "DROP SCHEMA grant_cleanup_schema", Vec::new())
        .expect("drop schema");
    let grants_after_schema_drop = cassie
        .midge
        .list_role_object_grants()
        .expect("list grants after schema drop");
    cassie
        .execute_sql(
            &root,
            "GRANT USAGE ON SCHEMA public TO grant_cleanup",
            Vec::new(),
        )
        .expect("grant public schema usage");
    cassie
        .execute_sql(&root, "DROP ROLE grant_cleanup", Vec::new())
        .expect("drop role");
    let grants_after_role_drop = cassie
        .midge
        .list_role_object_grants()
        .expect("list grants after role drop");

    // Assert
    assert_eq!(grants_after_schema_drop, Vec::new());
    assert_eq!(grants_after_role_drop, Vec::new());
}

#[test]
fn should_remove_database_grants_on_drop() {
    // Arrange
    let cassie = cassie();
    let root = root_session(&cassie);
    cassie
        .execute_sql(
            &root,
            "CREATE ROLE database_grant_reader LOGIN PASSWORD 'database-grant-secret'",
            Vec::new(),
        )
        .expect("create database reader");
    cassie
        .execute_sql(&root, "CREATE DATABASE grant_cleanup_database", Vec::new())
        .expect("create database");
    cassie
        .execute_sql(
            &root,
            "GRANT CONNECT ON DATABASE grant_cleanup_database TO database_grant_reader",
            Vec::new(),
        )
        .expect("grant database connect");
    let database_root = cassie
        .authenticate_role(
            "root",
            Some(crate::support_sql::test_root_password()),
            Some("grant_cleanup_database".to_owned()),
        )
        .expect("authenticate into grant cleanup database");
    cassie
        .execute_sql(
            &database_root,
            "GRANT USAGE ON SCHEMA public TO database_grant_reader",
            Vec::new(),
        )
        .expect("grant database schema usage");
    let database_family = cassie
        .catalog
        .get_database("grant_cleanup_database")
        .expect("database metadata")
        .physical_family;
    assert!(cassie
        .midge
        .list_role_object_grants()
        .expect("list database grants")
        .iter()
        .any(|grant| grant.database_family == database_family));

    // Act
    cassie
        .execute_sql(&root, "DROP DATABASE grant_cleanup_database", Vec::new())
        .expect("drop database");
    let grants_after_database_drop = cassie
        .midge
        .list_role_object_grants()
        .expect("list grants after database drop");

    // Assert
    assert!(grants_after_database_drop
        .iter()
        .all(|grant| grant.database_family != database_family));
}
