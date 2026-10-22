use super::support_sql as support;
use cassie::app::{Cassie, CassieError, CatalogObjectKind};
use cassie::types::Value;
use support::*;

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
    cassie
        .execute_sql(session, sql, vec![])
        .unwrap()
        .rows()
        .to_vec()
}

fn seed_projection_catalog_filtering_fixtures(
    cassie: &Cassie,
    postgres: &cassie::app::CassieSession,
    tenant: &cassie::app::CassieSession,
) {
    for sql in [
            "CREATE TABLE public.payroll_events (event_at TIMESTAMP)",
            "CREATE RETENTION POLICY payroll_keep ON payroll_events USING event_at RETAIN FOR '7 days'",
            "CREATE TABLE public.rollup_events (event_at TIMESTAMP, tenant TEXT)",
            "CREATE ROLLUP payroll_hourly ON rollup_events USING time_bucket('1 hour', event_at) GROUP BY tenant AGGREGATES COUNT(*) AS total",
            "CREATE TABLE public.projection_events (title TEXT)",
            "CREATE MATERIALIZED PROJECTION payroll_projection AS SELECT title FROM projection_events",
        ] {
            cassie.execute_sql(postgres, sql, vec![]).unwrap();
        }
    cassie
        .execute_sql(
            tenant,
            "CREATE TABLE public.tenant_events (title TEXT)",
            vec![],
        )
        .unwrap();
    cassie
        .execute_sql(
            tenant,
            "CREATE TABLE public.payroll_events (event_at TIMESTAMP)",
            vec![],
        )
        .unwrap();
    for (assignment_id, projection_id) in [
        ("postgres-assignment", "postgres.public.payroll_events"),
        ("ambiguous-assignment", "payroll_events"),
        ("tenant-assignment", "tenant_events"),
    ] {
        cassie
            .put_operational_assignment(cassie::catalog::OperationalAssignmentMeta {
                assignment_id: assignment_id.to_string(),
                node_id: "node-a".to_string(),
                projection_id: projection_id.to_string(),
                tenant: Some("tenant-a".to_string()),
                partition_key: Some("tenant-a:0".to_string()),
                generation: 1,
                state: cassie::catalog::OperationalAssignmentState::Claimed,
                routing_hint: None,
                updated_ms: 1,
            })
            .unwrap();
    }
}

#[test]
fn should_bootstrap_default_database_with_public_schema_on_fresh_startup() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = new_in_memory_cassie().unwrap();

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
            databases.rows(),
            vec![vec![Value::String("postgres".to_string())]]
        );
        assert_eq!(
            schemata.rows(),
            vec![
                vec![Value::String("information_schema".to_string())],
                vec![Value::String("pg_catalog".to_string())],
                vec![Value::String("public".to_string())],
            ]
        );
    });
}

#[test]
fn should_reject_queries_for_missing_session_database() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = new_in_memory_cassie().unwrap();
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
    });
}

#[test]
fn should_filter_catalog_views_plus_constraints_to_current_database() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = new_in_memory_cassie().unwrap();
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
    });
}

#[test]
fn should_filter_projection_catalog_views_to_current_database() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = new_in_memory_cassie().unwrap();
        cassie.startup().unwrap();
        let postgres = cassie.create_session("tester", Some("postgres".to_string()));
        cassie
            .execute_sql(&postgres, "CREATE DATABASE tenant_b", vec![])
            .unwrap();
        let tenant = cassie.create_session("tester", Some("tenant_b".to_string()));
        seed_projection_catalog_filtering_fixtures(&cassie, &postgres, &tenant);

        // Act
        let policies = query_rows(
            &cassie,
            &tenant,
            "SELECT collection FROM pg_catalog.pg_retention_policies",
        );
        let rollups = query_rows(
            &cassie,
            &tenant,
            "SELECT source_collection FROM pg_catalog.pg_rollups",
        );
        let projections = query_rows(
            &cassie,
            &tenant,
            "SELECT projection_name FROM pg_catalog.pg_materialized_projections",
        );
        let checkpoints = query_rows(
            &cassie,
            &tenant,
            "SELECT collection FROM pg_catalog.pg_projection_checkpoints",
        );
        let tenant_assignments = query_rows(
            &cassie,
            &tenant,
            "SELECT assignment_id FROM pg_catalog.pg_operational_assignments",
        );
        let owner_policies = query_rows(
            &cassie,
            &postgres,
            "SELECT collection FROM pg_catalog.pg_retention_policies",
        );
        let owner_rollups = query_rows(
            &cassie,
            &postgres,
            "SELECT source_collection FROM pg_catalog.pg_rollups",
        );
        let owner_projections = query_rows(
            &cassie,
            &postgres,
            "SELECT projection_name FROM pg_catalog.pg_materialized_projections",
        );
        let owner_assignments = query_rows(
            &cassie,
            &postgres,
            "SELECT assignment_id FROM pg_catalog.pg_operational_assignments",
        );

        // Assert
        assert_eq!(policies, [] as [std::vec::Vec<cassie::types::Value>; 0]);
        assert_eq!(rollups, [] as [std::vec::Vec<cassie::types::Value>; 0]);
        assert_eq!(projections, [] as [std::vec::Vec<cassie::types::Value>; 0]);
        assert_ne!(checkpoints, [] as [std::vec::Vec<cassie::types::Value>; 0]);
        assert!(checkpoints.iter().all(|row| {
            matches!(&row[0], Value::String(collection) if collection.starts_with("tenant_b."))
        }));
        assert_eq!(
            tenant_assignments,
            vec![vec![Value::String("tenant-assignment".to_string())]]
        );
        assert_eq!(owner_policies.len(), 1);
        assert_eq!(owner_rollups.len(), 1);
        assert_eq!(owner_projections.len(), 1);
        assert_eq!(
            owner_assignments,
            vec![vec![Value::String("postgres-assignment".to_string())]]
        );
    });
}

#[test]
fn should_restrict_pg_table_visibility_to_search_path() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = new_in_memory_cassie().unwrap();
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
            result.rows(),
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
    });
}

#[test]
fn should_reject_relation_creation_in_system_schemas() {
    // Arrange
    let cassie = new_in_memory_cassie().unwrap();
    cassie.startup().unwrap();
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(&session, "CREATE TABLE owned (id INT)", vec![])
        .unwrap();
    let statements = [
        "CREATE TABLE pg_catalog.leak (id BIGINT, secret TEXT)",
        "CREATE TABLE information_schema.leak (id BIGINT, secret TEXT)",
        "CREATE VIEW pg_catalog.leak_view AS SELECT id FROM owned",
        "CREATE SEQUENCE pg_catalog.leak_seq",
        "CREATE MATERIALIZED PROJECTION pg_catalog.leak_projection AS SELECT id FROM owned",
    ];

    for sql in statements {
        // Act
        let result = cassie.execute_sql(&session, sql, vec![]);

        // Assert
        assert!(
            matches!(result, Err(CassieError::InsufficientPrivilege)),
            "{sql} must be rejected"
        );
    }
}

#[test]
fn should_reject_relation_creation_given_system_schema_first_in_search_path() {
    // Arrange
    let cassie = new_in_memory_cassie().unwrap();
    cassie.startup().unwrap();
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(&session, "SET search_path TO pg_catalog", vec![])
        .unwrap();

    // Act
    let result = cassie.execute_sql(
        &session,
        "CREATE TABLE hidden (id BIGINT, secret TEXT)",
        vec![],
    );

    // Assert
    assert!(matches!(result, Err(CassieError::InsufficientPrivilege)));
}

#[test]
fn should_keep_system_schema_qualified_names_out_of_other_databases() {
    // Arrange
    let cassie = new_in_memory_cassie().unwrap();
    cassie.startup().unwrap();
    let postgres = cassie.create_session("tester", None);
    cassie
        .execute_sql(&postgres, "CREATE DATABASE tenant_b", vec![])
        .unwrap();
    let _ = cassie.execute_sql(
        &postgres,
        "CREATE TABLE pg_catalog.leak (id BIGINT, secret TEXT)",
        vec![],
    );
    let _ = cassie.execute_sql(
        &postgres,
        "INSERT INTO pg_catalog.leak (id, secret) VALUES (1, 'TOPSECRET')",
        vec![],
    );
    let tenant = cassie.create_session("tester", Some("tenant_b".to_string()));

    // Act
    let read = cassie.execute_sql(&tenant, "SELECT id, secret FROM pg_catalog.leak", vec![]);

    // Assert
    assert!(read.is_err(), "tenant_b must not reach pg_catalog.leak");
}

#[test]
fn should_not_read_graph_edges_from_another_database() {
    // Arrange
    let cassie = new_in_memory_cassie().unwrap();
    cassie.startup().unwrap();
    let postgres = cassie.create_session("tester", None);
    for sql in [
            "CREATE DATABASE analytics",
            "CREATE GRAPH social",
            "INSERT INTO social_edges (edge_id, source_type, source_id, target_type, target_id, edge_type, weight) VALUES ('e1', 'person', 'alice', 'person', 'bob', 'knows', 2)",
        ] {
            cassie.execute_sql(&postgres, sql, vec![]).unwrap();
        }
    let analytics = cassie.create_session("tester", Some("analytics".to_string()));
    for sql in [
            "CREATE GRAPH social",
            "INSERT INTO social_edges (edge_id, source_type, source_id, target_type, target_id, edge_type, weight) VALUES ('e2', 'person', 'alice', 'person', 'carol', 'knows', 1)",
        ] {
            cassie.execute_sql(&analytics, sql, vec![]).unwrap();
        }

    // Act
    let default_neighbors = query_rows(
        &cassie,
        &postgres,
        "SELECT node_id FROM graph_neighbors('social', 'person', 'alice', 'out', 'knows', 10)",
    );
    let tenant_neighbors = query_rows(
        &cassie,
        &analytics,
        "SELECT node_id FROM graph_neighbors('social', 'person', 'alice', 'out', 'knows', 10)",
    );
    let tenant_expansion = query_rows(
            &cassie,
            &analytics,
            "SELECT node_id, depth FROM graph_expand('social', 'person', 'alice', 2, 'out', 'knows', 10)",
        );
    let cross_database_reads = [
            "SELECT node_id FROM graph_shortest_path('social', 'person', 'alice', 'person', 'bob', 3, 'out', 'knows', 1)",
            "SELECT node_id FROM graph_neighbors('postgres.public.social', 'person', 'alice', 'out', 'knows', 10)",
        ]
        .map(|sql| (sql, cassie.execute_sql(&analytics, sql, vec![])));

    // Assert
    assert_eq!(
        default_neighbors,
        vec![vec![Value::String("bob".to_string())]]
    );
    assert_eq!(
        tenant_neighbors,
        vec![vec![Value::String("carol".to_string())]]
    );
    assert_eq!(
        tenant_expansion,
        vec![vec![Value::String("carol".to_string()), Value::Int64(1)]]
    );
    for (sql, read) in cross_database_reads {
        let leaked_rows = read.map_or(0, |result| result.rows().len());
        assert_eq!(
            leaked_rows, 0,
            "analytics session must not traverse the postgres graph: {sql}"
        );
    }
}
