//! An incrementally maintained HNSW graph survives a reopen on local disk.

use super::restart_lifecycle::{assert_indexed_zero_fallback, open};
use super::support::{
    create_index, create_table, seed_population, seed_rows, upsert, vector_metric, Rng, ALGORITHMS,
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
