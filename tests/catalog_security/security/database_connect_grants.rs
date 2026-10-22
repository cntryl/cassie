use cassie::app::{Cassie, CassieError, CatalogObjectKind};
use cassie::sql::ast::QueryStatement;

fn cassie(_label: &str) -> Cassie {
    let cassie = Cassie::new_in_memory_with_config(
        cassie::config::CassieRuntimeConfig::from_env().expect("runtime config"),
    )
    .expect("cassie");
    cassie.startup().expect("startup");
    cassie
}

fn setup_reader(cassie: &Cassie) -> cassie::app::CassieSession {
    let admin = cassie
        .authenticate_role("root", Some("postgres"), None)
        .expect("admin");
    cassie
        .execute_sql(&admin, "CREATE DATABASE analytics", Vec::new())
        .expect("database");
    cassie
        .execute_sql(
            &admin,
            "CREATE ROLE reader LOGIN PASSWORD 'reader-secret'",
            Vec::new(),
        )
        .expect("reader");
    admin
}

#[test]
fn should_grant_connect_on_the_creating_sessions_database() {
    // Arrange
    let cassie = cassie("create-role-session-database");
    let admin = cassie
        .authenticate_role("root", Some("postgres"), None)
        .expect("admin");
    cassie
        .execute_sql(&admin, "CREATE DATABASE tenant_b", Vec::new())
        .expect("database");
    let tenant_admin = cassie
        .authenticate_role("root", Some("postgres"), Some("tenant_b".to_string()))
        .expect("tenant admin");

    // Act
    cassie
        .execute_sql(
            &tenant_admin,
            "CREATE ROLE app_b LOGIN PASSWORD 'app-secret'",
            Vec::new(),
        )
        .expect("create role from tenant_b");

    // Assert
    assert!(cassie
        .authenticate_role("app_b", Some("app-secret"), Some("tenant_b".to_string()))
        .is_ok());
    assert!(matches!(
        cassie.authenticate_role("app_b", Some("app-secret"), Some("postgres".to_string())),
        Err(CassieError::InsufficientPrivilege)
    ));
}

#[test]
fn should_parse_exact_database_connect_grant_forms() {
    // Arrange
    let grant_sql = "GRANT CONNECT ON DATABASE analytics TO reader";
    let revoke_sql = "REVOKE CONNECT ON DATABASE analytics FROM reader";

    // Act
    let grant = cassie::sql::parse_statement(grant_sql).expect("grant");
    let revoke = cassie::sql::parse_statement(revoke_sql).expect("revoke");

    // Assert
    assert!(matches!(
        grant.statement,
        QueryStatement::GrantDatabaseConnect(statement)
            if statement.database == "analytics" && statement.role == "reader"
    ));
    assert!(matches!(
        revoke.statement,
        QueryStatement::RevokeDatabaseConnect(statement)
            if statement.database == "analytics" && statement.role == "reader"
    ));
}

#[test]
fn should_revoke_live_database_access_given_a_connect_grant_removal() {
    // Arrange
    let cassie = cassie("live-revalidation");
    let admin = setup_reader(&cassie);
    for _ in 0..2 {
        cassie
            .execute_sql(
                &admin,
                "GRANT CONNECT ON DATABASE analytics TO reader",
                Vec::new(),
            )
            .expect("grant");
    }
    let reader = cassie
        .authenticate_role(
            "reader",
            Some("reader-secret"),
            Some("analytics".to_string()),
        )
        .expect("reader session");
    let hits_before_warm = cassie.metrics()["execution_result_cache"]["hits"]
        .as_u64()
        .expect("cache hits before warmup");
    cassie
        .execute_sql(&reader, "SELECT 1", Vec::new())
        .expect("granted query");
    cassie
        .execute_sql(&reader, "SELECT 1", Vec::new())
        .expect("repeated granted query");
    let hits_after_warm = cassie.metrics()["execution_result_cache"]["hits"]
        .as_u64()
        .expect("cache hits after warmup");
    assert_eq!(hits_after_warm, hits_before_warm + 1);

    // Act
    for _ in 0..2 {
        cassie
            .execute_sql(
                &admin,
                "REVOKE CONNECT ON DATABASE analytics FROM reader",
                Vec::new(),
            )
            .expect("revoke");
    }
    let revoked = cassie
        .execute_sql(&reader, "SELECT 1", Vec::new())
        .expect_err("active session must lose access");
    let hits_after_revoked_query = cassie.metrics()["execution_result_cache"]["hits"]
        .as_u64()
        .expect("cache hits after revoked query");
    cassie
        .execute_sql(
            &admin,
            "GRANT CONNECT ON DATABASE analytics TO reader",
            Vec::new(),
        )
        .expect("restore grant");
    let restored = cassie.execute_sql(&reader, "SELECT 1", Vec::new());

    // Assert
    assert!(matches!(revoked, CassieError::InsufficientPrivilege));
    assert_eq!(hits_after_revoked_query, hits_after_warm);
    assert!(restored.is_ok());
}

#[test]
fn should_validate_database_connect_grant_authority_targets() {
    // Arrange
    let cassie = cassie("errors");
    let admin = setup_reader(&cassie);
    let reader = cassie
        .authenticate_role("reader", Some("reader-secret"), None)
        .expect("reader");

    // Act
    let denied = cassie
        .execute_sql(
            &reader,
            "GRANT CONNECT ON DATABASE analytics TO reader",
            Vec::new(),
        )
        .expect_err("non-admin grant");
    let missing_database = cassie
        .execute_sql(
            &admin,
            "GRANT CONNECT ON DATABASE missing TO reader",
            Vec::new(),
        )
        .expect_err("missing database");
    let missing_role = cassie
        .execute_sql(
            &admin,
            "GRANT CONNECT ON DATABASE analytics TO missing",
            Vec::new(),
        )
        .expect_err("missing role");
    let implicit_admin = cassie
        .execute_sql(
            &admin,
            "REVOKE CONNECT ON DATABASE analytics FROM root",
            Vec::new(),
        )
        .expect_err("admin access is implicit");

    // Assert
    assert!(matches!(denied, CassieError::InsufficientPrivilege));
    assert!(matches!(
        missing_database,
        CassieError::CatalogObjectNotFound {
            kind: CatalogObjectKind::Database,
            ..
        }
    ));
    assert!(matches!(
        missing_role,
        CassieError::CatalogObjectNotFound {
            kind: CatalogObjectKind::Role,
            ..
        }
    ));
    assert!(matches!(implicit_admin, CassieError::Unsupported(_)));
}
