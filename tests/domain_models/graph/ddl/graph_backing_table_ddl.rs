use cassie::app::{Cassie, CassieSession};
use cassie::types::Value;

use super::support_graph as support;
use support::{create_graph, current_thread_runtime, data_dir, execute, use_local_storage};

const NEIGHBORS: &str =
        "SELECT node_id FROM graph_neighbors('social', 'person', 'alice', 'out', 'knows', 10) ORDER BY node_id";

fn seeded_graph(label: &str) -> (Cassie, CassieSession, String) {
    use_local_storage();
    let path = data_dir(label);
    let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    create_graph(&cassie, &session);
    execute(
            &cassie,
            &session,
            "INSERT INTO social_nodes (node_type, node_id) VALUES ('person', 'alice'), ('person', 'bob'), ('person', 'carol')",
        );
    execute(
            &cassie,
            &session,
            "INSERT INTO social_edges (edge_id, source_type, source_id, target_type, target_id, edge_type, weight) VALUES ('e1', 'person', 'alice', 'person', 'bob', 'knows', 1)",
        );
    (cassie, session, path)
}

fn neighbors(cassie: &Cassie, session: &CassieSession) -> Vec<Vec<Value>> {
    cassie
        .execute_sql(session, NEIGHBORS, vec![])
        .expect("graph traversal")
        .rows()
        .to_vec()
}

fn restart(cassie: Cassie, path: &str) -> (Cassie, CassieSession) {
    drop(cassie);
    let reopened = Cassie::new_with_data_dir(path).expect("reopen cassie");
    reopened.startup().expect("restart after graph table DDL");
    let session = reopened.create_session("tester", None);
    (reopened, session)
}

fn bob_and_carol() -> Vec<Vec<Value>> {
    vec![
        vec![Value::String("bob".to_string())],
        vec![Value::String("carol".to_string())],
    ]
}

#[test]
fn should_keep_graph_traversal_after_renaming_an_edge_key_column() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
            let (cassie, session, path) = seeded_graph("graph_edge_column_rename");

            // Act
            execute(
                &cassie,
                &session,
                "ALTER TABLE social_edges RENAME COLUMN source_id TO src",
            );
            execute(
                &cassie,
                &session,
                "INSERT INTO social_edges (edge_id, source_type, src, target_type, target_id, edge_type, weight) VALUES ('e2', 'person', 'alice', 'person', 'carol', 'knows', 1)",
            );
            let before_restart = neighbors(&cassie, &session);
            let (reopened, reopened_session) = restart(cassie, &path);
            let after_restart = neighbors(&reopened, &reopened_session);

            // Assert
            assert_eq!(before_restart, bob_and_carol());
            assert_eq!(after_restart, bob_and_carol());
            let _ = std::fs::remove_dir_all(path);
        });
}

#[test]
fn should_reject_dropping_a_column_a_graph_reads() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let (cassie, session, path) = seeded_graph("graph_edge_column_drop");

        // Act
        let dropped = cassie.execute_sql(
            &session,
            "ALTER TABLE social_edges DROP COLUMN source_id",
            vec![],
        );
        let (reopened, reopened_session) = restart(cassie, &path);

        // Assert
        let error = dropped.expect_err("graph key column drop must be rejected");
        assert!(
            error.to_string().contains("graph"),
            "expected a graph dependency error, got {error}"
        );
        assert_eq!(
            neighbors(&reopened, &reopened_session),
            vec![vec![Value::String("bob".to_string())]]
        );
        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_keep_graph_traversal_after_renaming_the_edge_table() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
            let (cassie, session, path) = seeded_graph("graph_edge_table_rename");

            // Act
            execute(&cassie, &session, "ALTER TABLE social_edges RENAME TO other_edges");
            execute(
                &cassie,
                &session,
                "INSERT INTO other_edges (edge_id, source_type, source_id, target_type, target_id, edge_type, weight) VALUES ('e2', 'person', 'alice', 'person', 'carol', 'knows', 1)",
            );
            let before_restart = neighbors(&cassie, &session);
            let (reopened, reopened_session) = restart(cassie, &path);
            let after_restart = neighbors(&reopened, &reopened_session);

            // Assert
            assert_eq!(before_restart, bob_and_carol());
            assert_eq!(after_restart, bob_and_carol());
            let _ = std::fs::remove_dir_all(path);
        });
}

#[test]
fn should_start_when_graph_metadata_names_a_missing_edge_column() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let (cassie, _session, path) = seeded_graph("graph_incomplete_edge_restart");
        let mut graph = cassie
            .midge
            .list_graphs()
            .expect("list graphs")
            .into_iter()
            .next()
            .expect("graph metadata");
        graph.source_id_field = "missing_source".to_string();

        // Act
        let persisted = cassie.midge.put_graph(&graph);
        drop(cassie);
        let reopened = Cassie::new_with_data_dir(&path).expect("reopen cassie");
        let started = reopened.startup();

        // Assert
        assert!(
            persisted.is_ok(),
            "reconciling an incomplete edge row must not fail"
        );
        assert!(
            started.is_ok(),
            "an incomplete stored edge row must not block startup"
        );
        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_only_repoint_the_graph_in_the_altering_database() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
            use_local_storage();
            let path = data_dir("graph_backing_ddl_database_scope");
            let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
            cassie.startup().expect("startup");
            let admin = cassie.create_session("tester", Some("postgres".to_string()));
            let mut sessions = Vec::new();
            for database in ["tenant_a", "tenant_b"] {
                execute(&cassie, &admin, &format!("CREATE DATABASE {database}"));
                let session = cassie.create_session("tester", Some(database.to_string()));
                create_graph(&cassie, &session);
                execute(
                    &cassie,
                    &session,
                    "INSERT INTO social_nodes (node_type, node_id) VALUES ('person', 'alice'), ('person', 'bob'), ('person', 'carol')",
                );
                sessions.push(session);
            }
            let (tenant_a, tenant_b) = (&sessions[0], &sessions[1]);

            // Act
            execute(
                &cassie,
                tenant_a,
                "ALTER TABLE social_edges RENAME COLUMN source_id TO src",
            );
            execute(&cassie, tenant_a, "ALTER TABLE social_edges RENAME TO other_edges");
            execute(
                &cassie,
                tenant_a,
                "INSERT INTO other_edges (edge_id, source_type, src, target_type, target_id, edge_type, weight) VALUES ('e1', 'person', 'alice', 'person', 'bob', 'knows', 1)",
            );
            execute(
                &cassie,
                tenant_b,
                "INSERT INTO social_edges (edge_id, source_type, source_id, target_type, target_id, edge_type, weight) VALUES ('e1', 'person', 'alice', 'person', 'carol', 'knows', 1)",
            );
            let tenant_b_drop = cassie.execute_sql(
                tenant_b,
                "ALTER TABLE social_edges DROP COLUMN source_id",
                vec![],
            );
            let altered_neighbors = neighbors(&cassie, tenant_a);
            let untouched_neighbors = neighbors(&cassie, tenant_b);
            drop(sessions);
            drop(cassie);
            let reopened = Cassie::new_with_data_dir(&path).expect("reopen cassie");
            let restarted = reopened.startup();

            // Assert
            assert_eq!(altered_neighbors, vec![vec![Value::String("bob".to_string())]]);
            assert_eq!(untouched_neighbors, vec![vec![Value::String("carol".to_string())]]);
            assert!(
                tenant_b_drop.is_err(),
                "tenant_b graph still reads its own source_id column"
            );
            assert!(restarted.is_ok(), "restart after scoped graph DDL must succeed");
            let _ = std::fs::remove_dir_all(path);
        });
}
