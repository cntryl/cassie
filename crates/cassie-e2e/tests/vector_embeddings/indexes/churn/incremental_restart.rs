//! An incrementally maintained HNSW graph survives a reopen on local disk.

use cassie::app::Cassie;
use cassie::config::{CassieRuntimeConfig, EmbeddingsRuntimeConfig, LocalRuntimeConfig};
use cassie::types::Value;
use cassie::vector::hnsw::graph_validity_violation;

use super::restart_lifecycle::{assert_indexed_zero_fallback, open};
use super::support::{
    counters, create_index, create_table, seed_population, seed_rows, upsert, vector_metric, Rng,
    ALGORITHMS,
};
use crate::support_sql::{data_dir, use_local_storage};

#[test]
fn should_reopen_an_incrementally_maintained_graph_valid_with_zero_fallback() {
    // Arrange
    use_local_storage();
    let path = data_dir("vector_churn_incremental_reopen");
    let table = "reopen_incremental";
    let mut rng = Rng::new(0x1364_0009);
    let (population, generation) = {
        let (cassie, session) = open(&path);
        let mut population = seed_population(&mut rng);
        create_table(&cassie, &session, table);
        seed_rows(&cassie, table, &population);
        create_index(&cassie, &session, table, ALGORITHMS[0].1);

        // Act
        for step in 0..14 {
            let id = format!("churn-{step}");
            let vector = rng.vector(2_000 + step);
            upsert(&cassie, table, &id, vector);
            population.insert(id, vector);
            if step % 3 == 0 {
                let victim = population.keys().nth(step).expect("victim").clone();
                assert!(cassie
                    .midge
                    .delete_document(table, &victim)
                    .expect("delete"));
                population.remove(&victim);
            }
        }

        // Assert
        assert_eq!(
            vector_metric(&cassie, "index_maintenance_full_rebuilds_total"),
            0,
            "every write was maintained in place"
        );
        let generation = cassie
            .midge
            .collection_generation(table)
            .expect("generation");
        (population, generation)
    };

    // Reopen
    let (reopened, session) = open(&path);

    // Assert
    assert_eq!(
        reopened
            .midge
            .collection_generation(table)
            .expect("generation"),
        generation
    );
    let graph = reopened
        .midge
        .get_vector_index_state(table, "embedding")
        .expect("read state")
        .expect("published state")
        .hnsw_graph
        .expect("hnsw graph");
    let violation = cassie::vector::hnsw::graph_validity_violation(&graph, 16);
    assert!(violation.is_none(), "reopened graph: {violation:?}");
    assert_eq!(graph.row_count, population.len());
    assert_indexed_zero_fallback(&reopened, &session, table, "hnsw", &population, "reopened");
    drop(session);
    drop(reopened);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_reload_a_promoted_hnsw_entry_point_after_a_lower_layer_insert() {
    // Arrange
    let path = crate::support_temp_dirs::IsolatedDataDir::new("hnsw_entry_promotion");
    let open_local = || {
        let config = CassieRuntimeConfig {
            embeddings: EmbeddingsRuntimeConfig::Local(LocalRuntimeConfig {
                model: "deterministic-test".into(),
                dimensions: 3,
            }),
            ..CassieRuntimeConfig::default()
        };
        let cassie = Cassie::new_local_with_data_dir_and_config(path.path(), config)
            .expect("open local Cassie");
        crate::support_sql::install_vector_extension(&cassie);
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        (cassie, session)
    };
    let table = "reopen_entry_promotion";
    let read_graph = |cassie: &Cassie| {
        cassie
            .midge
            .get_vector_index_state(table, "embedding")
            .expect("read HNSW state")
            .expect("published HNSW state")
            .hnsw_graph
            .expect("HNSW graph")
    };

    // Act
    let expected_graph = {
        let (cassie, session) = open_local();
        create_table(&cassie, &session, table);
        upsert(&cassie, table, "row-4-0", [1.0, 0.0, 0.0]);
        create_index(&cassie, &session, table, ALGORITHMS[0].1);
        let original = read_graph(&cassie);
        assert_eq!(original.max_layer, 0);
        assert_eq!(original.entry_point.as_deref(), Some("row-4-0"));

        // The deterministic hash gives row-4-1 level one and row-4-2 level zero.
        upsert(&cassie, table, "row-4-1", [0.0, 1.0, 0.0]);
        let promoted = read_graph(&cassie);
        assert_eq!(promoted.entry_point.as_deref(), Some("row-4-1"));
        assert_eq!(promoted.max_layer, 1);
        assert_eq!(graph_validity_violation(&promoted, 16), None);
        upsert(&cassie, table, "row-4-2", [0.25, 0.75, 0.0]);
        assert_eq!(
            vector_metric(&cassie, "index_maintenance_full_rebuilds_total"),
            0,
            "both documents were inserted incrementally"
        );
        read_graph(&cassie)
    };
    let (reopened, session) = open_local();
    let graph = read_graph(&reopened);
    let before = counters(&reopened, "hnsw");
    let result = reopened
        .execute_sql(
            &session,
            "SELECT label, embedding <-> '[0,1,0]' AS distance FROM reopen_entry_promotion \
             ORDER BY embedding <-> '[0,1,0]' LIMIT 1",
            vec![],
        )
        .expect("search reopened promoted graph");
    let after = counters(&reopened, "hnsw");

    // Assert
    assert_eq!(graph, expected_graph);
    assert_eq!(graph.entry_point.as_deref(), Some("row-4-1"));
    assert_eq!(graph.max_layer, 1);
    assert_eq!(graph.row_count, 3);
    assert_eq!(graph_validity_violation(&graph, 16), None);
    assert_eq!(
        result.rows(),
        &[vec![Value::String("row-4-1".into()), Value::Float64(0.0)]]
    );
    assert_eq!(after.executions, before.executions + 1);
    assert_eq!(after.fallbacks, before.fallbacks);
}
