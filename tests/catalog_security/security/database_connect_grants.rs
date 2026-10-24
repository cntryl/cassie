use cassie::app::{Cassie, CassieError, CassieSession, CatalogObjectKind};
use cassie::sql::ast::QueryStatement;
use cassie::types::Value;

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

struct PgShdependGrantFixture {
    cassie: Cassie,
    admin: CassieSession,
    database_oid: i64,
    role_oid: i64,
}

impl PgShdependGrantFixture {
    fn new() -> Self {
        let cassie = cassie("pg-shdepend-connect-grant");
        let admin = cassie
            .authenticate_role("root", Some("postgres"), Some("postgres".to_owned()))
            .expect("admin");
        cassie
            .execute_sql(&admin, "CREATE DATABASE shdepend_analytics", Vec::new())
            .expect("database");
        cassie
            .execute_sql(
                &admin,
                "CREATE ROLE shdepend_reader LOGIN PASSWORD 'reader-secret'",
                Vec::new(),
            )
            .expect("reader");
        let database_oid = read_oid(
            &cassie,
            &admin,
            "SELECT oid FROM pg_catalog.pg_database \
             WHERE datname = 'shdepend_analytics'",
        );
        let role_oid = read_oid(&cassie, &admin, "SELECT 'shdepend_reader'::regrole::oid");

        Self {
            cassie,
            admin,
            database_oid,
            role_oid,
        }
    }

    fn dependency_rows(&self) -> Vec<Vec<Value>> {
        self.dependency_rows_for_database(self.database_oid)
    }

    fn dependency_rows_for_database(&self, database_oid: i64) -> Vec<Vec<Value>> {
        let dependency_sql = format!(
            "SELECT dbid, classid, objid, objsubid, refclassid, refobjid, deptype \
             FROM pg_catalog.pg_shdepend \
             WHERE classid = 1262 AND objid = {database_oid} \
               AND refclassid = 1260 AND refobjid = {} AND deptype = 'a'",
            self.role_oid
        );
        self.cassie
            .execute_sql(&self.admin, &dependency_sql, Vec::new())
            .expect("read shared dependencies")
            .rows()
            .to_vec()
    }

    fn grant_connect(&self) {
        self.grant_connect_to("shdepend_analytics");
    }

    fn grant_connect_to(&self, database: &str) {
        let sql = format!("GRANT CONNECT ON DATABASE {database} TO shdepend_reader");
        self.cassie
            .execute_sql(&self.admin, &sql, Vec::new())
            .expect("grant connect");
    }

    fn revoke_connect(&self) {
        self.cassie
            .execute_sql(
                &self.admin,
                "REVOKE CONNECT ON DATABASE shdepend_analytics FROM shdepend_reader",
                Vec::new(),
            )
            .expect("revoke connect");
    }

    fn expected_dependency_rows(&self, database_oid: i64) -> Vec<Vec<Value>> {
        vec![vec![
            Value::Int64(0),
            Value::Int64(1262),
            Value::Int64(database_oid),
            Value::Int64(0),
            Value::Int64(1260),
            Value::Int64(self.role_oid),
            Value::String("a".to_owned()),
        ]]
    }
}

fn read_oid(cassie: &Cassie, session: &CassieSession, query: &str) -> i64 {
    let result = cassie
        .execute_sql(session, query, Vec::new())
        .expect("read object identity");
    match &result.rows()[0][0] {
        Value::Int64(oid) => *oid,
        value => panic!("object OID is not an integer: {value:?}"),
    }
}

#[test]
fn should_expose_recorded_database_connect_grants_in_pg_shdepend() {
    // Arrange
    let fixture = PgShdependGrantFixture::new();
    let without_grant = fixture.dependency_rows();

    // Act
    fixture.grant_connect();
    let with_grant = fixture.dependency_rows();

    // Assert
    assert_eq!(without_grant, Vec::<Vec<Value>>::new());
    assert_eq!(
        with_grant,
        fixture.expected_dependency_rows(fixture.database_oid)
    );
}

#[test]
fn should_remove_pg_shdepend_row_after_database_connect_revoke() {
    // Arrange
    let fixture = PgShdependGrantFixture::new();
    fixture.grant_connect();
    let before_revoke = fixture.dependency_rows();

    // Act
    fixture.revoke_connect();
    let after_revoke = fixture.dependency_rows();

    // Assert
    assert_eq!(
        before_revoke,
        fixture.expected_dependency_rows(fixture.database_oid)
    );
    assert_eq!(after_revoke, Vec::<Vec<Value>>::new());
}

#[test]
fn should_emit_one_dependency_row_per_live_database_connect_grant() {
    // Arrange
    let fixture = PgShdependGrantFixture::new();
    fixture
        .cassie
        .execute_sql(
            &fixture.admin,
            "CREATE DATABASE shdepend_reporting",
            Vec::new(),
        )
        .expect("reporting database");
    let reporting_database_oid = read_oid(
        &fixture.cassie,
        &fixture.admin,
        "SELECT oid FROM pg_catalog.pg_database \
         WHERE datname = 'shdepend_reporting'",
    );
    fixture.grant_connect();
    fixture.grant_connect_to("shdepend_reporting");

    // Act
    let analytics_rows = fixture.dependency_rows_for_database(fixture.database_oid);
    let reporting_rows = fixture.dependency_rows_for_database(reporting_database_oid);

    // Assert
    assert_eq!(
        analytics_rows,
        fixture.expected_dependency_rows(fixture.database_oid)
    );
    assert_eq!(
        reporting_rows,
        fixture.expected_dependency_rows(reporting_database_oid)
    );
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
