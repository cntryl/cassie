use super::super::{Cassie, CassieError};
use crate::config::CassieRuntimeConfig;

const ISSUER: &str = "https://example-account.tokens.sts.global.api.aws";
const ROLE: &str = "arn:aws:iam::123456789012:role/projector";

fn cassie() -> Cassie {
    let storage = Cassie::new_in_memory_with_config(CassieRuntimeConfig {
        pgwire_aws_iam_trust_file: Some("fixture-trust.json".into()),
        ..CassieRuntimeConfig::default()
    })
    .expect("memory storage");
    storage.startup().expect("startup");
    storage
}

fn bind_sql(user: &str, operation: &str) -> String {
    format!("ALTER ROLE {user} {operation} AWS IAM ROLE '{ROLE}' ISSUER '{ISSUER}'")
}

#[test]
fn should_bind_external_login_to_one_immutable_role_and_revoke_active_access() {
    // Arrange
    let cassie = cassie();
    let admin = cassie.create_session("root", None);
    for sql in [
        "CREATE ROLE service EXTERNAL LOGIN",
        "CREATE ROLE other_service EXTERNAL LOGIN",
        "CREATE TABLE observations (value INTEGER)",
        "GRANT USAGE ON SCHEMA public TO service",
        "GRANT SELECT ON TABLE observations TO service",
    ] {
        cassie.execute_sql(&admin, sql, vec![]).expect(sql);
    }
    cassie
        .execute_sql(&admin, &bind_sql("service", "BIND"), vec![])
        .expect("bind service");
    let attempted_duplicate =
        cassie.execute_sql(&admin, &bind_sql("other_service", "BIND"), vec![]);
    let active = cassie
        .complete_aws_iam_login(
            cassie
                .begin_aws_iam_login(std::net::Ipv4Addr::LOCALHOST.into())
                .unwrap(),
            ISSUER,
            ROLE,
            None,
        )
        .expect("open mapped session");

    // Act
    let allowed = cassie.execute_sql(&active, "SELECT value FROM observations", vec![]);
    let denied = cassie.execute_sql(&active, "INSERT INTO observations VALUES (1)", vec![]);
    cassie
        .execute_sql(&admin, &bind_sql("service", "UNBIND"), vec![])
        .expect("revoke service binding");
    let revoked = cassie.execute_sql(&active, "SELECT value FROM observations", vec![]);

    // Assert
    assert_eq!(active.user, "service");
    assert!(attempted_duplicate.is_err());
    assert!(allowed.is_ok());
    assert!(matches!(denied, Err(CassieError::InsufficientPrivilege)));
    assert!(matches!(revoked, Err(CassieError::Unauthorized)));
}

#[test]
fn should_reject_reserved_selector_password_fallback_disabled_role_and_name_reuse() {
    // Arrange
    let cassie = cassie();
    let admin = cassie.create_session("root", None);
    let selector = cassie.execute_sql(
        &admin,
        "CREATE ROLE cassie_aws_iam LOGIN PASSWORD 'ignored'",
        vec![],
    );
    cassie
        .execute_sql(&admin, "CREATE ROLE service EXTERNAL LOGIN", vec![])
        .expect("create service");
    let bind = bind_sql("service", "BIND");
    cassie
        .execute_sql(&admin, &bind, vec![])
        .expect("bind service");
    let active = cassie
        .complete_aws_iam_login(
            cassie
                .begin_aws_iam_login(std::net::Ipv4Addr::LOCALHOST.into())
                .unwrap(),
            ISSUER,
            ROLE,
            None,
        )
        .expect("open mapped session");
    let login = || {
        cassie.complete_aws_iam_login(
            cassie
                .begin_aws_iam_login(std::net::Ipv4Addr::LOCALHOST.into())
                .unwrap(),
            ISSUER,
            ROLE,
            None,
        )
    };

    // Act
    let password = cassie.authenticate_role("service", None, None);
    cassie
        .execute_sql(&admin, "ALTER ROLE service NOLOGIN", vec![])
        .expect("disable service");
    let disabled = login();
    for sql in ["DROP ROLE service", "CREATE ROLE service EXTERNAL LOGIN"] {
        cassie.execute_sql(&admin, sql, vec![]).expect(sql);
    }
    let unsigned_rebind = login();
    cassie
        .execute_sql(&admin, &bind, vec![])
        .expect("explicitly bind the replacement");
    let old_session = cassie.ensure_session_database_access(&active);

    // Assert
    assert!(selector.is_err());
    assert!(matches!(password, Err(CassieError::Unauthorized)));
    assert!(matches!(disabled, Err(CassieError::Unauthorized)));
    assert!(matches!(unsigned_rebind, Err(CassieError::Unauthorized)));
    assert!(matches!(old_session, Err(CassieError::Unauthorized)));
    assert!(
        login().is_ok(),
        "explicit binding authorizes the new role identity"
    );
}

#[test]
fn should_resolve_object_grants_against_the_iam_session_original_role_id() {
    // Arrange
    let cassie = cassie();
    let admin = cassie.create_session("root", None);
    let bind = bind_sql("service", "BIND");
    for sql in [
        "CREATE ROLE service EXTERNAL LOGIN",
        &bind,
        "CREATE TABLE private_rows (secret INTEGER)",
    ] {
        cassie.execute_sql(&admin, sql, vec![]).expect(sql);
    }
    let stale = cassie
        .complete_aws_iam_login(
            cassie
                .begin_aws_iam_login(std::net::Ipv4Addr::LOCALHOST.into())
                .unwrap(),
            ISSUER,
            ROLE,
            None,
        )
        .expect("open original role session");
    for sql in [
        "DROP ROLE service",
        "CREATE ROLE service EXTERNAL LOGIN",
        "GRANT USAGE ON SCHEMA public TO service",
        "GRANT SELECT ON TABLE private_rows TO service",
        &bind,
    ] {
        cassie.execute_sql(&admin, sql, vec![]).expect(sql);
    }
    let replacement = cassie
        .complete_aws_iam_login(
            cassie
                .begin_aws_iam_login(std::net::Ipv4Addr::LOCALHOST.into())
                .unwrap(),
            ISSUER,
            ROLE,
            None,
        )
        .expect("open replacement role session");

    // Act
    let read = |session| {
        let context = cassie.binding_context_for_session(Some(session));
        super::super::access::require_table_read(
            &cassie.catalog,
            session,
            &context,
            "postgres.public.private_rows",
        )
    };
    let stale_read = read(&stale);
    let replacement_read = read(&replacement);

    // Assert
    assert!(matches!(
        stale_read,
        Err(CassieError::InsufficientPrivilege)
    ));
    replacement_read.expect("the explicitly bound replacement can use its own grants");
}
