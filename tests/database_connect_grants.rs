use cassie::app::{Cassie, CassieError, CatalogObjectKind};
use cassie::sql::ast::QueryStatement;

fn cassie(label: &str) -> Cassie {
    std::env::set_var("CASSIE_MIDGE_ALLOW_FALLBACK", "1");
    let path = std::env::temp_dir().join(format!(
        "cassie-database-connect-{label}-{}",
        uuid::Uuid::new_v4()
    ));
    let cassie = Cassie::new_with_data_dir(path).expect("cassie");
    cassie.startup().expect("startup");
    cassie
}

fn setup_reader(cassie: &Cassie) -> cassie::app::CassieSession {
    let admin = cassie
        .authenticate_role("postgres", Some("postgres"), None)
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
fn should_revalidate_active_sessions_across_idempotent_database_connect_changes() {
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
    cassie
        .execute_sql(&reader, "SELECT 1", Vec::new())
        .expect("granted query");

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
            "REVOKE CONNECT ON DATABASE analytics FROM postgres",
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
