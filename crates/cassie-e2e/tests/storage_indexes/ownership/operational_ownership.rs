use cassie::app::Cassie;
use cassie::catalog::{OperationalAssignmentMeta, OperationalAssignmentState};
use cassie::types::Value;

use super::support_data_dir as data_dir;
use super::support_local_storage as local_storage;
use data_dir::data_dir;
use local_storage::use_local_storage;

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
    use_local_storage();
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
            selected.rows(),
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
    let path = data_dir("query_semantics");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
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
            selected.rows(),
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
            routed_tenant.rows(),
            vec![vec![Value::String("bravo".to_string())]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_apply_last_write_wins_on_concurrent_assignment_updates() {
    // Arrange
    let path = data_dir("concurrent_update");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE operational_concurrent_docs (tenant_id TEXT, title TEXT)",
                vec![],
            )
            .unwrap();

        let mut first = assignment("operational_concurrent_docs", "tenant-a");
        first.generation = 1;
        first.state = OperationalAssignmentState::Claimed;
        first.updated_ms = 1_000;
        let mut second = first.clone();
        second.generation = 2;
        second.state = OperationalAssignmentState::Draining;
        second.updated_ms = 2_000;
        let mut third = first.clone();
        third.generation = 3;
        third.state = OperationalAssignmentState::Released;
        third.updated_ms = 3_000;

        // Act: simulate racing writers landing out of submission order but in
        // increasing generation/updated_ms order, matching the assignment
        // lifecycle contract in docs/operational-scale.md.
        cassie.put_operational_assignment(first).unwrap();
        cassie.put_operational_assignment(second).unwrap();
        cassie.put_operational_assignment(third).unwrap();

        let selected = cassie
            .execute_sql(
                &session,
                "SELECT generation, state, updated_ms FROM pg_catalog.pg_operational_assignments WHERE assignment_id = 'operational_concurrent_docs-tenant-a'",
                vec![],
            )
            .unwrap();
        let all_for_assignment = cassie
            .execute_sql(
                &session,
                "SELECT assignment_id FROM pg_catalog.pg_operational_assignments WHERE assignment_id = 'operational_concurrent_docs-tenant-a'",
                vec![],
            )
            .unwrap();

        // Assert: exactly one row survives, holding the last write.
        assert_eq!(
            selected.rows(),
            vec![vec![
                Value::Int64(3),
                Value::String("released".to_string()),
                Value::Int64(3_000),
            ]]
        );
        assert_eq!(all_for_assignment.rows().len(), 1);

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_bound_assignment_cardinality_to_distinct_assignment_ids() {
    // Arrange
    use_local_storage();
    let path = data_dir("bounded_cardinality");
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
                "CREATE TABLE operational_bounded_docs (tenant_id TEXT, title TEXT)",
                vec![],
            )
            .unwrap();

        // Act: write 5 distinct assignments, each updated 4 times, so 20
        // total put_operational_assignment calls land on 5 distinct keys.
        let tenants = ["tenant-a", "tenant-b", "tenant-c", "tenant-d", "tenant-e"];
        for tenant in tenants {
            for generation in 1..=4 {
                let mut meta = assignment("operational_bounded_docs", tenant);
                meta.generation = generation;
                meta.updated_ms = 1_000 * generation;
                cassie.put_operational_assignment(meta).unwrap();
            }
        }

        let listed = cassie.list_operational_assignments();
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT assignment_id FROM pg_catalog.pg_operational_assignments WHERE projection_id = 'operational_bounded_docs'",
                vec![],
            )
            .unwrap();

        // Assert: cardinality tracks distinct assignment_id count, not the
        // number of writes, for both the in-memory catalog and the
        // persisted store.
        assert_eq!(listed.len(), tenants.len());
        assert_eq!(selected.rows().len(), tenants.len());
        assert!(listed.iter().all(|meta| meta.generation == 4));

        drop(cassie);
        let restarted = Cassie::new_with_data_dir(&path).unwrap();
        restarted.startup().unwrap();
        assert_eq!(restarted.list_operational_assignments().len(), tenants.len());

        let _ = std::fs::remove_dir_all(path);
    });
}
