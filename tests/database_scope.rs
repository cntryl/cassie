use cassie::app::{Cassie, CassieError, CatalogObjectKind};
use cassie::types::Value;
use uuid::Uuid;

fn with_fallback() {
    std::env::set_var("CASSIE_MIDGE_ALLOW_FALLBACK", "1");
}

fn data_dir(label: &str) -> String {
    let mut path = std::env::temp_dir();
    path.push(format!(
        "cassie-database-scope-{}-{}",
        label,
        Uuid::new_v4()
    ));
    path.to_string_lossy().to_string()
}

fn seed_catalog_filtering_fixtures(
    cassie: &Cassie,
    postgres: &cassie::app::CassieSession,
    tenant: &cassie::app::CassieSession,
) {
    cassie
        .execute_sql(postgres, "CREATE DATABASE tenant_b", vec![])
        .unwrap();
    cassie
        .execute_sql(postgres, "CREATE SCHEMA reporting", vec![])
        .unwrap();
    for sql in [
        "CREATE TABLE public.shared_docs (id INT PRIMARY KEY, title TEXT)",
        "CREATE TABLE reporting.shared_docs (id INT PRIMARY KEY, title TEXT)",
        "CREATE TABLE reporting.orders (id INT PRIMARY KEY, doc_id INT REFERENCES reporting.shared_docs(id))",
        "CREATE VIEW reporting.shared_docs_view AS SELECT title FROM reporting.shared_docs",
    ] {
        cassie.execute_sql(postgres, sql, vec![]).unwrap();
    }

    cassie
        .execute_sql(tenant, "CREATE SCHEMA reporting", vec![])
        .unwrap();
    for sql in [
        "CREATE TABLE reporting.shared_docs (id INT PRIMARY KEY, title TEXT)",
        "CREATE VIEW reporting.shared_docs_view AS SELECT title FROM reporting.shared_docs",
    ] {
        cassie.execute_sql(tenant, sql, vec![]).unwrap();
    }
}

fn query_rows(cassie: &Cassie, session: &cassie::app::CassieSession, sql: &str) -> Vec<Vec<Value>> {
    cassie.execute_sql(session, sql, vec![]).unwrap().rows
}

#[test]
fn should_bootstrap_default_database_with_public_schema_on_fresh_startup() {
    // Arrange
    with_fallback();
    let path = data_dir("bootstrap_default_database");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();

        // Act
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        let databases = cassie
            .execute_sql(
                &session,
                "SELECT datname FROM pg_catalog.pg_database ORDER BY datname",
                vec![],
            )
            .unwrap();
        let schemata = cassie
            .execute_sql(
                &session,
                "SELECT schema_name FROM information_schema.schemata ORDER BY schema_name",
                vec![],
            )
            .unwrap();

        // Assert
        assert!(cassie.catalog.database_exists("postgres"));
        assert!(cassie.catalog.namespace_exists("postgres.public"));
        assert_eq!(
            databases.rows,
            vec![vec![Value::String("postgres".to_string())]]
        );
        assert_eq!(
            schemata.rows,
            vec![
                vec![Value::String("information_schema".to_string())],
                vec![Value::String("pg_catalog".to_string())],
                vec![Value::String("public".to_string())],
            ]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reject_queries_for_missing_session_database() {
    // Arrange
    with_fallback();
    let path = data_dir("missing_session_database");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", Some("missing_db".to_string()));

        // Act
        let error = cassie
            .execute_sql(&session, "SELECT 1", vec![])
            .expect_err("missing database should be rejected");

        // Assert
        let CassieError::CatalogObjectNotFound { kind, name } = error else {
            panic!("expected missing database error");
        };
        assert_eq!(kind, CatalogObjectKind::Database);
        assert_eq!(name, "missing_db");

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_filter_catalog_views_plus_constraints_to_current_database() {
    // Arrange
    with_fallback();
    let path = data_dir("catalog_filtering");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let postgres = cassie.create_session("tester", Some("postgres".to_string()));
        let tenant = cassie.create_session("tester", Some("tenant_b".to_string()));
        seed_catalog_filtering_fixtures(&cassie, &postgres, &tenant);

        // Act
        let tables = query_rows(
            &cassie,
            &postgres,
            "SELECT table_schema, table_name FROM information_schema.tables WHERE table_name IN ('shared_docs', 'shared_docs_view') ORDER BY table_schema, table_name",
        );
        let constraints = query_rows(
            &cassie,
            &postgres,
            "SELECT table_schema, table_name FROM information_schema.table_constraints WHERE table_name IN ('shared_docs', 'orders') ORDER BY table_schema, table_name",
        );
        let references = query_rows(
            &cassie,
            &postgres,
            "SELECT constraint_schema, unique_constraint_schema FROM information_schema.referential_constraints ORDER BY constraint_schema, unique_constraint_schema",
        );
        let tenant_tables = query_rows(
            &cassie,
            &tenant,
            "SELECT table_schema, table_name FROM information_schema.tables WHERE table_name = 'shared_docs' ORDER BY table_schema, table_name",
        );

        // Assert
        assert_eq!(
            tables,
            vec![
                vec![
                    Value::String("public".to_string()),
                    Value::String("shared_docs".to_string()),
                ],
                vec![
                    Value::String("reporting".to_string()),
                    Value::String("shared_docs".to_string()),
                ],
                vec![
                    Value::String("reporting".to_string()),
                    Value::String("shared_docs_view".to_string()),
                ],
            ]
        );
        assert_eq!(
            constraints,
            vec![
                vec![
                    Value::String("public".to_string()),
                    Value::String("shared_docs".to_string()),
                ],
                vec![
                    Value::String("reporting".to_string()),
                    Value::String("orders".to_string()),
                ],
                vec![
                    Value::String("reporting".to_string()),
                    Value::String("orders".to_string()),
                ],
                vec![
                    Value::String("reporting".to_string()),
                    Value::String("shared_docs".to_string()),
                ],
            ]
        );
        assert_eq!(
            references,
            vec![vec![
                Value::String("reporting".to_string()),
                Value::String("reporting".to_string()),
            ]]
        );
        assert_eq!(
            tenant_tables,
            vec![vec![
                Value::String("reporting".to_string()),
                Value::String("shared_docs".to_string()),
            ]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_restrict_pg_table_visibility_to_search_path() {
    // Arrange
    with_fallback();
    let path = data_dir("pg_table_visibility");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", Some("postgres".to_string()));
        cassie
            .execute_sql(&session, "CREATE SCHEMA reporting", vec![])
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE public.visible_docs (title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE reporting.visible_docs (title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(&session, "SET search_path = reporting", vec![])
            .unwrap();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT relnamespace, pg_catalog.pg_table_is_visible(oid) FROM pg_catalog.pg_class WHERE relname = 'visible_docs' ORDER BY relnamespace",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            result.rows,
            vec![
                vec![
                    Value::String("public".to_string()),
                    Value::Bool(false),
                ],
                vec![
                    Value::String("reporting".to_string()),
                    Value::Bool(true),
                ],
            ]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}
