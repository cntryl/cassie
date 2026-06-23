use cassie::app::Cassie;
use cassie::catalog::{OperationalAssignmentMeta, OperationalAssignmentState};
use cassie::types::Value;
use std::path::PathBuf;
use uuid::Uuid;

fn with_fallback() {
    if std::env::var("CASSIE_EMBEDDINGS_PROVIDER").is_err() {
        std::env::set_var("CASSIE_EMBEDDINGS_PROVIDER", "fallback");
    }
}

fn data_dir(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("cassie-operational-{name}-{}", Uuid::new_v4()))
}

fn assignment(projection_id: &str, tenant: &str) -> OperationalAssignmentMeta {
    OperationalAssignmentMeta {
        assignment_id: format!("{projection_id}-{tenant}"),
        node_id: "node-a".to_string(),
        projection_id: projection_id.to_string(),
        tenant: Some(tenant.to_string()),
        partition_key: Some(format!("{tenant}:0")),
        generation: 7,
        state: OperationalAssignmentState::Claimed,
        routing_hint: Some(format!("local://node-a/{projection_id}/{tenant}")),
        updated_ms: 1_234,
    }
}

#[test]
fn should_persist_operational_assignment_metadata_after_restart() {
    // Arrange
    with_fallback();
    let path = data_dir("restart");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE operational_restart_docs (tenant_id TEXT, title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .put_operational_assignment(assignment("operational_restart_docs", "tenant-a"))
            .unwrap();
        drop(cassie);

        // Act
        let restarted = Cassie::new_with_data_dir(&path).unwrap();
        restarted.startup().unwrap();
        let session = restarted.create_session("tester", None);
        let selected = restarted
            .execute_sql(
                &session,
                "SELECT node_id, projection_id, tenant, partition_key, generation, state, routing_hint, updated_ms FROM pg_catalog.pg_operational_assignments WHERE assignment_id = 'operational_restart_docs-tenant-a'",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![vec![
                Value::String("node-a".to_string()),
                Value::String("operational_restart_docs".to_string()),
                Value::String("tenant-a".to_string()),
                Value::String("tenant-a:0".to_string()),
                Value::Int64(7),
                Value::String("claimed".to_string()),
                Value::String("local://node-a/operational_restart_docs/tenant-a".to_string()),
                Value::Int64(1_234),
            ]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_not_route_or_filter_queries_from_operational_assignment_metadata() {
    // Arrange
    with_fallback();
    let path = data_dir("query_semantics");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE operational_query_docs (tenant_id TEXT, title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO operational_query_docs (tenant_id, title) VALUES ('tenant-a', 'alpha'), ('tenant-b', 'bravo')",
                vec![],
            )
            .unwrap();
        cassie
            .put_operational_assignment(assignment("operational_query_docs", "tenant-a"))
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT tenant_id, title FROM operational_query_docs ORDER BY tenant_id",
                vec![],
            )
            .unwrap();
        let routed_tenant = cassie
            .execute_sql(
                &session,
                "SELECT title FROM operational_query_docs WHERE tenant_id = 'tenant-b'",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![
                vec![
                    Value::String("tenant-a".to_string()),
                    Value::String("alpha".to_string())
                ],
                vec![
                    Value::String("tenant-b".to_string()),
                    Value::String("bravo".to_string())
                ],
            ]
        );
        assert_eq!(
            routed_tenant.rows,
            vec![vec![Value::String("bravo".to_string())]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}
