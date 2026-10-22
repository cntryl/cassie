use cassie::types::Value;

use super::support_graph as support;
use support::*;

#[test]
fn should_store_graph_data_in_its_session_database() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
            let cassie = new_in_memory_cassie().expect("cassie");
            cassie.startup().expect("startup");
            let admin = cassie.create_session("tester", Some("postgres".to_string()));
            cassie
                .execute_sql(&admin, "CREATE DATABASE tenant_a", vec![])
                .expect("create tenant_a");
            let tenant_a = cassie.create_session("tester", Some("tenant_a".to_string()));
            let postgres = cassie.create_session("tester", Some("postgres".to_string()));
            cassie
                .execute_sql(&tenant_a, "CREATE GRAPH social", vec![])
                .expect("create tenant_a graph");

            // Act
            let insert = cassie.execute_sql(
                &tenant_a,
                "INSERT INTO social_nodes (node_type, node_id) VALUES ('person', 'alice'), ('person', 'bob')",
                vec![],
            );

            // Assert
            assert!(
                insert.is_ok(),
                "graph backing relations must resolve in the creating database"
            );
            cassie
                .execute_sql(
                    &tenant_a,
                    "INSERT INTO social_edges (edge_id, source_type, source_id, target_type, target_id, edge_type, weight) VALUES ('e1', 'person', 'alice', 'person', 'bob', 'knows', 1)",
                    vec![],
                )
                .expect("insert tenant_a graph edge");
            let neighbors = cassie
                .execute_sql(
                    &tenant_a,
                    "SELECT node_id FROM graph_neighbors('social', 'person', 'alice', 'out', 'knows', 10)",
                    vec![],
                )
                .expect("query tenant_a graph");
            let postgres_read = cassie.execute_sql(
                &postgres,
                "SELECT node_id FROM social_nodes",
                vec![],
            );
            let postgres_graph = cassie.execute_sql(
                &postgres,
                "SELECT node_id FROM graph_neighbors('social', 'person', 'alice', 'out', 'knows', 10)",
                vec![],
            );

            assert_eq!(neighbors.rows(), vec![vec![Value::String("bob".to_string())]]);
            assert!(
                postgres_read.is_err(),
                "another database must not read graph backing relations"
            );
            assert!(
                postgres_graph.is_err(),
                "another database must not resolve the graph metadata"
            );
        });
}

#[test]
fn should_allow_same_graph_name_in_multiple_databases() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
            let cassie = new_in_memory_cassie().expect("cassie");
            cassie.startup().expect("startup");
            let admin = cassie.create_session("tester", Some("postgres".to_string()));
            for database in ["tenant_a", "tenant_b"] {
                cassie
                    .execute_sql(&admin, &format!("CREATE DATABASE {database}"), vec![])
                    .expect("create database");
            }
            let tenant_a = cassie.create_session("tester", Some("tenant_a".to_string()));
            let tenant_b = cassie.create_session("tester", Some("tenant_b".to_string()));

            // Act
            cassie
                .execute_sql(&tenant_a, "CREATE GRAPH social", vec![])
                .expect("create tenant_a graph");
            cassie
                .execute_sql(&tenant_b, "CREATE GRAPH social", vec![])
                .expect("create same-named tenant_b graph");

            for (session, edge_id, target) in [
                (&tenant_a, "edge_a", "tenant_a_node"),
                (&tenant_b, "edge_b", "tenant_b_node"),
            ] {
                cassie
                    .execute_sql(
                        session,
                        &format!(
                            "INSERT INTO social_nodes (node_type, node_id) VALUES ('person', 'alice'), ('person', '{target}')"
                        ),
                        vec![],
                    )
                    .expect("insert graph nodes");
                cassie
                    .execute_sql(
                        session,
                        &format!(
                            "INSERT INTO social_edges (edge_id, source_type, source_id, target_type, target_id, edge_type, weight) VALUES ('{edge_id}', 'person', 'alice', 'person', '{target}', 'knows', 1)"
                        ),
                        vec![],
                    )
                    .expect("insert graph edge");
            }

            let first_database_neighbors = cassie
                .execute_sql(
                    &tenant_a,
                    "SELECT node_id FROM graph_neighbors('social', 'person', 'alice', 'out', 'knows', 10)",
                    vec![],
                )
                .expect("query tenant_a graph");
            let second_database_neighbors = cassie
                .execute_sql(
                    &tenant_b,
                    "SELECT node_id FROM graph_neighbors('social', 'person', 'alice', 'out', 'knows', 10)",
                    vec![],
                )
                .expect("query tenant_b graph");
            let graphs = cassie.catalog.list_graphs();

            // Assert
            assert_eq!(
                graphs
                    .iter()
                    .map(|graph| graph.name.as_str())
                    .collect::<Vec<_>>(),
                vec!["tenant_a.public.social", "tenant_b.public.social"]
            );
            assert_eq!(
                first_database_neighbors.rows(),
                vec![vec![Value::String("tenant_a_node".to_string())]]
            );
            assert_eq!(
                second_database_neighbors.rows(),
                vec![vec![Value::String("tenant_b_node".to_string())]]
            );
        });
}

#[test]
fn should_drop_empty_database_when_graph_exists_in_another_database() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let cassie = new_in_memory_cassie().expect("cassie");
        cassie.startup().expect("startup");
        let admin = cassie.create_session("tester", Some("postgres".to_string()));
        for database in ["tenant_a", "tenant_empty"] {
            cassie
                .execute_sql(&admin, &format!("CREATE DATABASE {database}"), vec![])
                .expect("create database");
        }
        let tenant_a = cassie.create_session("tester", Some("tenant_a".to_string()));

        // Act
        cassie
            .execute_sql(&tenant_a, "CREATE GRAPH social", vec![])
            .expect("create tenant_a graph");
        let empty_database_drop = cassie.execute_sql(&admin, "DROP DATABASE tenant_empty", vec![]);

        // Assert
        assert!(
            empty_database_drop.is_ok(),
            "a graph in another database must not make an empty database non-empty: {empty_database_drop:?}"
        );
    });
}
