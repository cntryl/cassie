//! Localized HNSW maintenance (#1364): a document write touches only the graph
//! nodes it must, keeps the graph valid and keeps recall above the declared
//! floor, and a damaged graph is repaired without failing the write.

use std::collections::BTreeSet;

use cassie::app::{Cassie, CassieSession};
use cassie::embeddings::{HnswGraphState, HnswIndexOptions};
use cassie::midge::adapter::StorageFamily;
use cassie::types::Value;
use cassie::vector::hnsw::{graph_validity_violation, search_graph};

use super::support::{
    assert_matches_population, counters, create_index, create_table, last_fallback_reason, payload,
    run_query, seed_rows, upsert, vector_metric, with_memory_fixture, Population, Rng,
};

const M: usize = 8;
const HNSW_OPTIONS: &str = "index_type = hnsw, m = 8, ef_construction = 32, ef_search = 64";
const PRNG_START: u64 = 0x1364_0007;
/// One insertion rewrites the new node, its linked neighbors and the neighbors
/// they drop, per layer. It never depends on how many other rows exist.
const WRITE_NODE_BOUND: u64 = 64;
const RECALL_FLOOR: f64 = 0.90;
const RECALL_K: usize = 10;

fn rows(count: usize, rng: &mut Rng) -> Population {
    (0..count)
        .map(|index| (format!("row-{index:05}"), rng.vector(index)))
        .collect()
}

fn indexed_table(cassie: &Cassie, session: &CassieSession, table: &str, population: &Population) {
    create_table(cassie, session, table);
    seed_rows(cassie, table, population);
    create_index(cassie, session, table, HNSW_OPTIONS);
}

fn persisted_graph(cassie: &Cassie, table: &str) -> HnswGraphState {
    cassie
        .midge
        .get_vector_index_state(table, "embedding")
        .expect("read vector state")
        .expect("published vector state")
        .hnsw_graph
        .expect("hnsw graph")
}

fn assert_valid_graph(cassie: &Cassie, table: &str, population: &Population, context: &str) {
    let graph = persisted_graph(cassie, table);
    let violation = graph_validity_violation(&graph, M);
    assert!(violation.is_none(), "{context}: {violation:?}");
    let nodes = graph
        .nodes
        .iter()
        .map(|node| node.id.as_str())
        .collect::<BTreeSet<_>>();
    let rows = population
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    assert_eq!(nodes, rows, "{context}: graph nodes mirror the population");
}

fn l2(left: [f32; 3], right: [f32; 3]) -> f64 {
    left.iter()
        .zip(right)
        .map(|(left, right)| (f64::from(*left) - f64::from(right)).powi(2))
        .sum::<f64>()
        .sqrt()
}

/// Mean recall@k of graph search against the exact top-k of the population.
fn recall(graph: &HnswGraphState, population: &Population, rng: &mut Rng) -> f64 {
    let options = HnswIndexOptions {
        version: 1,
        m: M,
        ef_construction: 32,
        ef_search: 64,
    };
    let queries = 40;
    let mut hits = 0usize;
    for ordinal in 0..queries {
        let query = rng.vector(50_000 + ordinal);
        let mut exact = population
            .iter()
            .map(|(id, vector)| (l2(*vector, query), id.as_str()))
            .collect::<Vec<_>>();
        exact.sort_by(|left, right| left.0.total_cmp(&right.0).then_with(|| left.1.cmp(right.1)));
        let expected = exact
            .iter()
            .take(RECALL_K)
            .map(|(_, id)| *id)
            .collect::<BTreeSet<_>>();
        let found = search_graph(graph, &query, &options, RECALL_K).expect("graph search");
        hits += found
            .candidates
            .iter()
            .filter(|candidate| expected.contains(candidate.id.as_str()))
            .count();
    }
    let total = u32::try_from(queries * RECALL_K).expect("query count");
    f64::from(u32::try_from(hits).expect("hits")) / f64::from(total)
}

fn last_nodes_written(cassie: &Cassie) -> u64 {
    vector_metric(cassie, "index_maintenance_last_nodes_written")
}

fn single_insert_cost(row_count: usize) -> (u64, u64, u64) {
    let mut cost = None;
    with_memory_fixture(|cassie, session| {
        let mut rng = Rng::new(PRNG_START);
        let population = rows(row_count, &mut rng);
        indexed_table(cassie, session, "write_cost", &population);
        let rebuilds = vector_metric(cassie, "index_maintenance_full_rebuilds_total");
        let commits = vector_metric(cassie, "index_maintenance_commits_total");

        // Act
        upsert(cassie, "write_cost", "added-row", rng.vector(row_count + 1));

        // Assert
        assert_eq!(
            vector_metric(cassie, "index_maintenance_commits_total"),
            commits + 1
        );
        assert_eq!(
            vector_metric(cassie, "index_maintenance_full_rebuilds_total"),
            rebuilds,
            "the write is maintained in place"
        );
        assert_eq!(
            vector_metric(cassie, "index_maintenance_deferred_rebuilds_total"),
            0
        );
        let graph = persisted_graph(cassie, "write_cost");
        assert_eq!(graph.row_count, row_count + 1);
        cost = Some((
            last_nodes_written(cassie),
            vector_metric(cassie, "index_maintenance_last_nodes_read"),
            vector_metric(cassie, "index_maintenance_last_nodes_deleted"),
        ));
    });
    cost.expect("fixture ran")
}

#[test]
fn should_write_a_bounded_number_of_nodes_regardless_of_unrelated_rows() {
    // Arrange
    let small_rows = 60;
    let large_rows = small_rows * 10;

    // Act
    let (small_written, _, small_deleted) = single_insert_cost(small_rows);
    let (large_written, _, large_deleted) = single_insert_cost(large_rows);

    // Assert
    assert!((1..=WRITE_NODE_BOUND).contains(&small_written));
    assert!(
        (1..=WRITE_NODE_BOUND).contains(&large_written),
        "a {large_rows} row collection wrote {large_written} nodes for one document"
    );
    assert_eq!((small_deleted, large_deleted), (0, 0));
}

#[test]
fn should_write_a_bounded_number_of_nodes_to_a_ten_thousand_row_collection() {
    // Arrange
    let row_count = 10_000;

    // Act
    let (written, read, deleted) = single_insert_cost(row_count);

    // Assert
    assert!(
        (1..=WRITE_NODE_BOUND).contains(&written),
        "one document wrote {written} nodes to a {row_count} row graph"
    );
    assert_eq!(deleted, 0);
    assert!(
        read < u64::try_from(row_count).expect("rows") / 4,
        "one write read {read} of {row_count} nodes"
    );
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    Insert,
    Replace,
    Delete,
    DeleteEntryPoint,
}

fn plan(rng: &mut Rng, steps: usize) -> Vec<Op> {
    (0..steps)
        .map(|step| {
            if step == 8 || step == steps / 2 {
                return Op::DeleteEntryPoint;
            }
            match rng.below(10) {
                0..=3 => Op::Insert,
                4..=6 => Op::Replace,
                _ => Op::Delete,
            }
        })
        .collect()
}

fn nth_id(population: &Population, rng: &mut Rng) -> String {
    population
        .keys()
        .nth(rng.below(population.len()))
        .expect("population is never empty")
        .clone()
}

fn apply(cassie: &Cassie, table: &str, op: Op, population: &mut Population, rng: &mut Rng) {
    match op {
        Op::Insert => {
            let id = format!("ins-{}", population.len() + rng.below(1_000_000));
            let vector = rng.vector(population.len());
            upsert(cassie, table, &id, vector);
            population.insert(id, vector);
        }
        Op::Replace => {
            let id = nth_id(population, rng);
            let vector = rng.vector(population.len() + 7);
            upsert(cassie, table, &id, vector);
            population.insert(id, vector);
        }
        Op::Delete | Op::DeleteEntryPoint => {
            let id = if op == Op::DeleteEntryPoint {
                persisted_graph(cassie, table)
                    .entry_point
                    .expect("populated graph has an entry point")
            } else {
                nth_id(population, rng)
            };
            assert!(cassie.midge.delete_document(table, &id).expect("delete"));
            population.remove(&id);
        }
    }
}

#[test]
fn should_keep_the_graph_valid_through_seeded_churn_with_entry_point_deletes() {
    // Arrange
    with_memory_fixture(|cassie, session| {
        let table = "incremental_churn";
        let mut rng = Rng::new(PRNG_START);
        let mut population = rows(120, &mut rng);
        indexed_table(cassie, session, table, &population);
        let plan = plan(&mut rng, 150);

        for (step, op) in plan.into_iter().enumerate() {
            let context = format!("step {step} {op:?}");
            let entry_before = persisted_graph(cassie, table).entry_point;

            // Act
            apply(cassie, table, op, &mut population, &mut rng);

            // Assert
            assert_valid_graph(cassie, table, &population, &context);
            assert!(last_nodes_written(cassie) <= WRITE_NODE_BOUND, "{context}");
            if op == Op::DeleteEntryPoint {
                let entry_after = persisted_graph(cassie, table).entry_point;
                assert_ne!(entry_after, entry_before, "{context}: entry re-elected");
            }
        }

        // Assert
        assert_eq!(
            vector_metric(cassie, "index_maintenance_full_rebuilds_total"),
            0,
            "no write needed a rebuild"
        );
        let graph = persisted_graph(cassie, table);
        let score = recall(&graph, &population, &mut rng);
        assert!(
            score >= RECALL_FLOOR,
            "recall@{RECALL_K} {score} fell below {RECALL_FLOOR} after churn"
        );
    });
}

#[test]
fn should_publish_the_highest_surviving_layer_after_hnsw_entry_deletions() {
    // Arrange
    with_memory_fixture(|cassie, session| {
        let table = "incremental_entry_deletions";
        // The seed-four witness uses two dimensions. A trailing zero retains the
        // same L2 geometry while using the shared three-dimensional fixture.
        let coordinates = [
            [-12.0, 0.0, 0.0],
            [-14.0, 10.0, 0.0],
            [-1.0, -1.0, 0.0],
            [0.0, 0.0, 0.0],
            [4.0, 6.0, 0.0],
            [10.0, -13.0, 0.0],
            [8.0, 6.0, 0.0],
            [0.0, -10.0, 0.0],
            [6.0, 15.0, 0.0],
            [3.0, 11.0, 0.0],
            [-6.0, 5.0, 0.0],
            [-1.0, -13.0, 0.0],
        ];
        let mut population = coordinates
            .into_iter()
            .enumerate()
            .map(|(ordinal, vector)| (format!("row-4-{ordinal}"), vector))
            .collect::<Population>();
        create_table(cassie, session, table);
        seed_rows(cassie, table, &population);
        create_index(
            cassie,
            session,
            table,
            "index_type = hnsw, m = 2, ef_construction = 64, ef_search = 40",
        );
        let original = persisted_graph(cassie, table);
        assert_eq!(original.entry_point.as_deref(), Some("row-4-1"));
        assert_eq!(original.max_layer, 1);
        assert_eq!(graph_validity_violation(&original, 2), None);

        // Act
        for id in ["row-4-1", "row-4-10", "row-4-9"] {
            assert!(cassie.midge.delete_document(table, id).expect("delete row"));
            population.remove(id);
            let graph = persisted_graph(cassie, table);
            assert_eq!(
                graph_validity_violation(&graph, 2),
                None,
                "after deleting {id}"
            );
        }
        let before_query = counters(cassie, "hnsw");
        let result = cassie
            .execute_sql(
                session,
                "SELECT label, embedding <-> '[10,-13,0]' AS distance FROM incremental_entry_deletions \
                 ORDER BY embedding <-> '[10,-13,0]' LIMIT 1",
                vec![],
            )
            .expect("search graph after repeated entry deletions");
        let after_query = counters(cassie, "hnsw");

        // Assert
        let graph = persisted_graph(cassie, table);
        assert_eq!(graph.row_count, 9);
        assert_eq!(graph.max_layer, 1);
        assert_eq!(graph.entry_point.as_deref(), Some("row-4-5"));
        assert_valid_graph(cassie, table, &population, "after entry deletions");
        assert_eq!(
            result.rows(),
            &[vec![Value::String("row-4-5".into()), Value::Float64(0.0)]]
        );
        assert_eq!(after_query.executions, before_query.executions + 1);
        assert_eq!(after_query.fallbacks, before_query.fallbacks);

        // Move the surviving entry to the deleted original entry's coordinates.
        let replacement = [-14.0, 10.0, 0.0];
        upsert(cassie, table, "row-4-5", replacement);
        population.insert("row-4-5".into(), replacement);
        let updated = persisted_graph(cassie, table);
        assert_eq!(updated.row_count, 9);
        assert_eq!(graph_validity_violation(&updated, 2), None);
        assert_valid_graph(
            cassie,
            table,
            &population,
            "after replacing the surviving entry",
        );
        let before_update_query = counters(cassie, "hnsw");
        let updated_result = cassie
            .execute_sql(
                session,
                "SELECT label, embedding <-> '[-14,10,0]' AS distance FROM incremental_entry_deletions \
                 ORDER BY embedding <-> '[-14,10,0]' LIMIT 1",
                vec![],
            )
            .expect("indexed search at the updated coordinates");
        let after_update_query = counters(cassie, "hnsw");
        assert_eq!(
            updated_result.rows(),
            &[vec![Value::String("row-4-5".into()), Value::Float64(0.0)]]
        );
        assert_eq!(
            after_update_query.executions,
            before_update_query.executions + 1
        );
        assert_eq!(after_update_query.fallbacks, before_update_query.fallbacks);
    });
}

#[test]
fn should_serve_exactly_reranked_indexed_results_after_churn() {
    // Arrange
    with_memory_fixture(|cassie, session| {
        let table = "incremental_results";
        let mut rng = Rng::new(PRNG_START + 1);
        let mut population = rows(90, &mut rng);
        indexed_table(cassie, session, table, &population);
        for (step, op) in plan(&mut rng, 40).into_iter().enumerate() {
            apply(cassie, table, op, &mut population, &mut rng);

            // Act
            let before = counters(cassie, "hnsw");
            let ranked = run_query(cassie, session, table);
            let after = counters(cassie, "hnsw");

            // Assert
            assert_matches_population(
                cassie,
                table,
                &ranked,
                &population,
                &format!("step {step} {op:?}"),
            );
            assert_eq!(after.executions, before.executions + 1, "step {step}");
            assert_eq!(
                after.fallbacks,
                before.fallbacks,
                "step {step}: zero fallback ({})",
                last_fallback_reason(cassie)
            );
        }
    });
}

#[test]
fn should_repopulate_an_indexed_collection_after_emptying_it() {
    // Arrange
    with_memory_fixture(|cassie, session| {
        let table = "incremental_empty";
        let mut rng = Rng::new(PRNG_START + 2);
        let mut population = rows(5, &mut rng);
        indexed_table(cassie, session, table, &population);

        // Act
        for id in population.keys().cloned().collect::<Vec<_>>() {
            assert!(cassie.midge.delete_document(table, &id).expect("delete"));
            population.remove(&id);
            assert_valid_graph(cassie, table, &population, &format!("deleted {id}"));
        }
        for ordinal in 0..4 {
            let id = format!("again-{ordinal}");
            let vector = rng.vector(300 + ordinal);
            upsert(cassie, table, &id, vector);
            population.insert(id, vector);
        }

        // Assert
        assert_valid_graph(cassie, table, &population, "repopulated");
        let ranked = run_query(cassie, session, table);
        assert_matches_population(cassie, table, &ranked, &population, "repopulated");
        assert_eq!(
            vector_metric(cassie, "index_maintenance_full_rebuilds_total"),
            0
        );
    });
}

/// Damages the entry point's node, which every insertion search visits.
fn damage_one_node(cassie: &Cassie, table: &str, garbage: Option<&[u8]>) {
    let entry = persisted_graph(cassie, table)
        .entry_point
        .expect("populated graph has an entry point");
    let prefix = cassie
        .midge
        .hnsw_node_prefix_for_diagnostics(table, "embedding")
        .expect("node prefix");
    let (key, _) = cassie
        .midge
        .raw_scan_prefix(StorageFamily::Data, &prefix)
        .expect("scan nodes")
        .into_iter()
        .find(|(key, _)| key.ends_with(entry.as_bytes()))
        .expect("entry point node");
    match garbage {
        Some(bytes) => cassie
            .midge
            .raw_put(StorageFamily::Data, &key, bytes)
            .expect("corrupt node"),
        None => cassie
            .midge
            .raw_delete(StorageFamily::Data, &key)
            .expect("remove node"),
    }
}

#[test]
fn should_repair_a_damaged_graph_without_failing_the_write() {
    for garbage in [None, Some(&b"not a node"[..])] {
        // Arrange
        with_memory_fixture(|cassie, session| {
            let table = "incremental_damaged";
            let mut rng = Rng::new(PRNG_START + 3);
            let mut population = rows(40, &mut rng);
            indexed_table(cassie, session, table, &population);
            damage_one_node(cassie, table, garbage);
            let rebuilds = vector_metric(cassie, "index_maintenance_full_rebuilds_total");

            // Act
            let written = cassie.midge.put_document(
                table,
                Some("after-damage".to_string()),
                payload("after-damage", rng.vector(900)),
            );

            // Assert
            assert!(written.is_ok(), "damage must not fail the user's write");
            population.insert("after-damage".to_string(), rng.vector(900));
            assert_eq!(
                vector_metric(cassie, "index_maintenance_full_rebuilds_total"),
                rebuilds + 1,
                "the damaged graph is rebuilt in the commit"
            );
            assert_valid_graph(cassie, table, &population, "after repair");
            let ranked = run_query(cassie, session, table);
            assert_matches_population(cassie, table, &ranked, &population, "after repair");
        });
    }
}

#[test]
fn should_rebuild_a_large_damaged_graph_after_the_commit() {
    // Arrange
    with_memory_fixture(|cassie, session| {
        let table = "incremental_deferred";
        let mut rng = Rng::new(PRNG_START + 4);
        let mut population = rows(700, &mut rng);
        indexed_table(cassie, session, table, &population);
        damage_one_node(cassie, table, None);
        let vector = rng.vector(901);

        // Act
        let written = cassie.midge.put_document(
            table,
            Some("after-damage".to_string()),
            payload("after-damage", vector),
        );

        // Assert
        assert!(written.is_ok(), "damage must not fail the user's write");
        population.insert("after-damage".to_string(), vector);
        assert_eq!(
            vector_metric(cassie, "index_maintenance_deferred_rebuilds_total"),
            1
        );
        assert!(
            !cassie
                .midge
                .has_vector_maintenance_debt(table, "embedding")
                .expect("debt lookup"),
            "the post-commit rebuild repaid the debt"
        );
        assert_valid_graph(cassie, table, &population, "after deferred rebuild");
        let ranked = run_query(cassie, session, table);
        assert_matches_population(
            cassie,
            table,
            &ranked,
            &population,
            "after deferred rebuild",
        );
    });
}

#[test]
fn should_keep_recall_after_deleting_two_thirds_of_the_rows() {
    // Arrange
    with_memory_fixture(|cassie, session| {
        let table = "incremental_mass_delete";
        let mut rng = Rng::new(PRNG_START ^ 0x00de);
        let mut population = rows(300, &mut rng);
        indexed_table(cassie, session, table, &population);
        let doomed = population
            .keys()
            .enumerate()
            .filter(|(ordinal, _)| ordinal % 3 != 0)
            .map(|(_, id)| id.clone())
            .collect::<Vec<_>>();

        // Act
        for id in &doomed {
            assert!(cassie.midge.delete_document(table, id).expect("delete"));
            population.remove(id);
        }
        let graph = persisted_graph(cassie, table);
        let score = recall(&graph, &population, &mut rng);

        // Assert
        assert_valid_graph(cassie, table, &population, "after mass delete");
        assert_eq!(
            vector_metric(cassie, "index_maintenance_full_rebuilds_total"),
            0,
            "deletes are maintained locally"
        );
        assert!(
            score >= RECALL_FLOOR,
            "recall@{RECALL_K} {score} fell below {RECALL_FLOOR} after deleting {} of 300 rows",
            doomed.len()
        );
    });
}
