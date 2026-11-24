//! Index builds and rebuilds larger than one Midge transaction validation budget.

use super::support::{
    counters, create_index, create_table_with, footprint, index_state_generation, rebuild,
    vector_metric, with_memory_fixture_dimensions,
};

const ROWS: usize = 1_400;
const DIMENSIONS: usize = 1_536;
const TRANSACTION_BYTE_BUDGET: u64 = 8 * 1024 * 1024;
const QUERY_MEMORY_BUDGET: usize = 64 * 1024 * 1024;

fn component(row: usize, dimension: usize) -> f32 {
    f32::from(u8::try_from((row * 31 + dimension * 17) % 97).expect("lane")) / 97.0 - 0.5
}

fn vector(row: usize) -> Vec<f32> {
    (0..DIMENSIONS)
        .map(|dimension| component(row, dimension))
        .collect()
}

fn literal(vector: &[f32]) -> String {
    let parts = vector
        .iter()
        .map(f32::to_string)
        .collect::<Vec<_>>()
        .join(",");
    format!("[{parts}]")
}

#[test]
fn should_publish_an_index_larger_than_one_transaction_budget_in_bounded_batches() {
    // Arrange
    with_memory_fixture_dimensions(DIMENSIONS, QUERY_MEMORY_BUDGET, |cassie, session| {
        let table = "bounded_rebuild";
        create_table_with(cassie, session, table, DIMENSIONS);
        let documents = (0..ROWS)
            .map(|row| {
                let id = format!("row-{row:05}");
                (
                    Some(id.clone()),
                    serde_json::json!({"body": id, "label": id, "embedding": vector(row)}),
                )
            })
            .collect();
        cassie
            .midge
            .put_fresh_documents(table, documents)
            .expect("seed large vectors");

        // Act
        create_index(
            cassie,
            session,
            table,
            "index_type = ivfflat, lists = 8, probes = 1, training_sample_size = 64, training_seed = 7",
        );

        // Assert
        let created = footprint(cassie, table);
        assert_eq!(created["normalized_keys"].as_u64(), Some(ROWS as u64));
        assert_eq!(created["membership_keys"].as_u64(), Some(ROWS as u64));
        assert!(
            created["normalized_bytes"].as_u64().unwrap() > TRANSACTION_BYTE_BUDGET,
            "the fixture must not fit one validation budget"
        );
        assert!(
            vector_metric(cassie, "index_last_rebuild_transactions") >= 3,
            "the build is split across several transactions"
        );
        assert!(
            vector_metric(cassie, "index_max_sidecar_transaction_bytes")
                <= TRANSACTION_BYTE_BUDGET / 2,
            "every sidecar transaction stays well inside the budget"
        );
        let generation = cassie
            .midge
            .collection_generation(table)
            .expect("generation");
        assert_eq!(index_state_generation(cassie, table), generation);

        // Query the rebuilt index
        let query = vector(7);
        let before = counters(cassie, "ivfflat");
        let literal = literal(&query);
        let rows = cassie
            .execute_sql(
                session,
                &format!(
                    "SELECT label, embedding <-> '{literal}' AS distance FROM {table} \
                     ORDER BY embedding <-> '{literal}' LIMIT 3"
                ),
                vec![],
            )
            .unwrap_or_else(|error| {
                panic!("large vector top-k: {error} {}", cassie.metrics()["vector"])
            });
        let after = counters(cassie, "ivfflat");

        // Assert
        assert_eq!(
            rows.rows()[0][0],
            cassie::types::Value::String("row-00007".to_string())
        );
        assert_eq!(after.executions, before.executions + 1);
        assert_eq!(after.fallbacks, before.fallbacks);
        assert_eq!(after.observed_generation, generation);
    });
}

#[test]
fn should_rewrite_only_changed_artifacts_when_rebuilding_an_unchanged_large_index() {
    // Arrange
    with_memory_fixture_dimensions(DIMENSIONS, QUERY_MEMORY_BUDGET, |cassie, session| {
        let table = "bounded_unchanged";
        create_table_with(cassie, session, table, DIMENSIONS);
        let documents = (0..ROWS)
            .map(|row| {
                let id = format!("row-{row:05}");
                (
                    Some(id.clone()),
                    serde_json::json!({"body": id, "label": id, "embedding": vector(row)}),
                )
            })
            .collect();
        cassie
            .midge
            .put_fresh_documents(table, documents)
            .expect("seed large vectors");
        create_index(
            cassie,
            session,
            table,
            "index_type = ivfflat, lists = 8, probes = 1, training_sample_size = 64, training_seed = 7",
        );
        let initial_bytes = vector_metric(cassie, "index_last_rebuild_bytes_written");

        // Act
        rebuild(cassie, table, "ivfflat");

        // Assert
        let rebuild_bytes = vector_metric(cassie, "index_last_rebuild_bytes_written");
        assert!(
            rebuild_bytes * 20 < initial_bytes,
            "an unchanged rebuild writes {rebuild_bytes} bytes against {initial_bytes} for the build"
        );
        assert_eq!(vector_metric(cassie, "index_last_rebuild_transactions"), 1);
    });
}
