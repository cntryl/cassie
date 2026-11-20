//! Interrupted publication, reopen on local disk and obsolete-artifact reclamation.

use cassie::app::{Cassie, CassieSession};
use cassie::config::{
    CassieRuntimeConfig, EmbeddingsRuntimeConfig, ExecutionResultCacheEnabled, LocalRuntimeConfig,
};
use cassie::midge::adapter::{
    document_write_failure_point_test_guard, set_document_write_failure_point,
    DocumentWriteFailurePoint, StorageFamily,
};
use cntryl_midge::{TransactionMode, WriteOptions};

use super::support::{
    assert_matches_population, collection, counters, create_index, create_table, footprint,
    index_state_generation, last_fallback_reason, rebuild, run_function_query, run_query,
    seed_population, seed_rows, upsert, vector_metric, Population, Rng, ALGORITHMS, QUERY,
};
use crate::support_sql::{data_dir, use_local_storage};

fn local_config() -> CassieRuntimeConfig {
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
    config.embeddings = EmbeddingsRuntimeConfig::Local(LocalRuntimeConfig {
        model: "deterministic-test".to_string(),
        dimensions: 3,
    });
    config
}

fn open(path: &str) -> (Cassie, CassieSession) {
    let cassie = Cassie::new_with_data_dir_and_config(path, local_config()).expect("open Cassie");
    crate::support_sql::install_vector_extension(&cassie);
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    (cassie, session)
}

fn populated(cassie: &Cassie, session: &CassieSession, table: &str, options: &str) -> Population {
    let mut rng = Rng::new(0x1364_0003);
    let mut population = seed_population(&mut rng);
    create_table(cassie, session, table);
    seed_rows(cassie, table, &population);
    create_index(cassie, session, table, options);
    upsert(cassie, table, "closest", QUERY);
    population.insert("closest".to_string(), QUERY);
    let victim = population.keys().nth(3).expect("victim").clone();
    assert!(cassie
        .midge
        .delete_document(table, &victim)
        .expect("delete"));
    population.remove(&victim);
    population
}

/// Copies one real node and one real list membership under an identity no row
/// owns. The copies decode, so startup still hydrates, but only a rebuild may
/// reclaim them. Returns how many obsolete keys were planted.
fn plant_obsolete_artifacts(cassie: &Cassie, population: &Population, table: &str) -> usize {
    let prefixes = [
        cassie
            .midge
            .hnsw_node_prefix_for_diagnostics(table, "embedding")
            .expect("node prefix"),
        cassie
            .midge
            .ivfflat_membership_prefix_for_diagnostics(table, "embedding")
            .expect("membership prefix"),
    ];
    let mut planted = Vec::new();
    for prefix in &prefixes {
        let Some((key, value)) = cassie
            .midge
            .raw_scan_prefix(StorageFamily::Data, prefix)
            .expect("scan artifacts")
            .into_iter()
            .next()
        else {
            continue;
        };
        let owner = population
            .keys()
            .find(|id| key.ends_with(id.as_bytes()))
            .expect("artifact key ends with its row identity");
        let mut obsolete = key[..key.len() - owner.len()].to_vec();
        obsolete.extend_from_slice(b"obsolete-artifact");
        planted.push((obsolete, value));
    }
    let mut tx = cassie
        .midge
        .data_tx(TransactionMode::ReadWrite)
        .expect("data transaction");
    for (key, value) in &planted {
        tx.put(key.clone(), value.clone(), None)
            .expect("plant obsolete key");
    }
    tx.commit(WriteOptions::sync())
        .expect("commit obsolete keys");
    planted.len()
}

fn assert_serves_population(
    cassie: &Cassie,
    session: &CassieSession,
    table: &str,
    population: &Population,
    context: &str,
) {
    let rows = run_query(cassie, session, table);
    assert_matches_population(cassie, table, &rows, population, context);
}

fn assert_indexed_zero_fallback(
    cassie: &Cassie,
    session: &CassieSession,
    table: &str,
    algorithm: &str,
    population: &Population,
    context: &str,
) {
    let generation = cassie
        .midge
        .collection_generation(table)
        .expect("generation");
    let before = counters(cassie, algorithm);
    let rows = run_query(cassie, session, table);
    let after = counters(cassie, algorithm);
    assert_matches_population(cassie, table, &rows, population, context);
    assert_eq!(
        after.executions,
        before.executions + 1,
        "{context}: indexed (last fallback {}; before {before:?} after {after:?})",
        last_fallback_reason(cassie)
    );
    assert_eq!(
        after.fallbacks, before.fallbacks,
        "{context}: zero fallback"
    );
    assert_eq!(
        after.observed_generation, generation,
        "{context}: generation label"
    );
    assert_eq!(
        index_state_generation(cassie, table),
        generation,
        "{context}"
    );
}

fn live_artifacts(artifacts: &serde_json::Value, algorithm: &str) -> u64 {
    let key = if algorithm == "hnsw" {
        "graph_node_keys"
    } else {
        "membership_keys"
    };
    artifacts[key].as_u64().expect("artifact count")
}

#[test]
fn should_reclaim_obsolete_artifacts_and_reopen_the_rebuilt_generation_with_zero_fallback() {
    // Arrange
    use_local_storage();
    for (algorithm, options) in ALGORITHMS {
        let path = data_dir("vector_churn_reopen");
        let table = "reopen_rebuilt";
        let (generation, rebuilt_footprint, population) = {
            let (cassie, session) = open(&path);
            let population = populated(&cassie, &session, table, options);
            let planted = plant_obsolete_artifacts(&cassie, &population, table);
            let polluted = footprint(&cassie, table);
            let rebuilds = vector_metric(&cassie, "index_rebuilds_total");

            // Act
            rebuild(&cassie, table, algorithm);

            // Assert
            let rebuilt = footprint(&cassie, table);
            assert_eq!(
                polluted["total_keys"].as_u64().unwrap() - rebuilt["total_keys"].as_u64().unwrap(),
                planted as u64,
                "{algorithm}: planted obsolete artifacts are reclaimed"
            );
            assert_eq!(
                rebuilt["normalized_keys"].as_u64(),
                Some(population.len() as u64)
            );
            assert_eq!(live_artifacts(&rebuilt, algorithm), population.len() as u64);
            assert_eq!(vector_metric(&cassie, "index_rebuilds_total"), rebuilds + 1);
            assert_indexed_zero_fallback(
                &cassie,
                &session,
                table,
                algorithm,
                &population,
                algorithm,
            );
            let generation = cassie
                .midge
                .collection_generation(table)
                .expect("generation");
            (generation, rebuilt, population)
        };

        // Act
        let (reopened, session) = open(&path);

        // Assert
        assert_eq!(
            reopened
                .midge
                .collection_generation(table)
                .expect("generation"),
            generation,
            "{algorithm}: reopen keeps the published generation"
        );
        assert_indexed_zero_fallback(
            &reopened,
            &session,
            table,
            algorithm,
            &population,
            algorithm,
        );
        assert_eq!(
            footprint(&reopened, table),
            rebuilt_footprint,
            "{algorithm}"
        );
        drop(session);
        drop(reopened);
        let _ = std::fs::remove_dir_all(path);
    }
}

#[test]
fn should_keep_the_prior_index_when_a_rebuild_is_interrupted_before_manifest_publication() {
    // Arrange
    use_local_storage();
    let _failpoint_guard = document_write_failure_point_test_guard();
    for (algorithm, options) in ALGORITHMS {
        let path = data_dir("vector_churn_before_manifest");
        let table = "before_manifest";
        let (cassie, session) = open(&path);
        let population = populated(&cassie, &session, table, options);
        let planted = plant_obsolete_artifacts(&cassie, &population, table);
        let polluted = footprint(&cassie, table);
        let generation = cassie
            .midge
            .collection_generation(table)
            .expect("generation");
        let canonical = collection(&cassie, table);

        // Act
        set_document_write_failure_point(Some(DocumentWriteFailurePoint::VectorState));
        let interrupted = if algorithm == "hnsw" {
            cassie.midge.refresh_hnsw_indexes_for_collection(&canonical)
        } else {
            cassie
                .midge
                .refresh_ivfflat_indexes_for_collection(&canonical)
        };
        set_document_write_failure_point(None);

        // Assert
        assert!(interrupted.is_err(), "{algorithm}: interruption surfaces");
        assert_eq!(
            footprint(&cassie, table),
            polluted,
            "{algorithm}: nothing was published, so nothing was reclaimed"
        );
        assert_serves_population(&cassie, &session, table, &population, algorithm);
        drop(session);
        drop(cassie);
        let (reopened, session) = open(&path);
        assert_serves_population(&reopened, &session, table, &population, algorithm);

        // Act
        rebuild(&reopened, table, algorithm);

        // Assert
        let retried = footprint(&reopened, table);
        assert_eq!(live_artifacts(&retried, algorithm), population.len() as u64);
        assert_eq!(
            retried["total_keys"].as_u64().unwrap(),
            polluted["total_keys"].as_u64().unwrap() - planted as u64,
            "{algorithm}: the retry reclaims obsolete artifacts"
        );
        assert_indexed_zero_fallback(
            &reopened,
            &session,
            table,
            algorithm,
            &population,
            algorithm,
        );
        assert_eq!(
            reopened
                .midge
                .collection_generation(table)
                .expect("generation"),
            generation
        );
        drop(session);
        drop(reopened);
        let _ = std::fs::remove_dir_all(path);
    }
}

#[test]
fn should_reopen_a_usable_current_index_when_a_rebuild_is_interrupted_after_manifest_publication() {
    // Arrange
    use_local_storage();
    let _failpoint_guard = document_write_failure_point_test_guard();
    for (algorithm, options) in ALGORITHMS {
        let path = data_dir("vector_churn_after_manifest");
        let table = "after_manifest";
        let (cassie, session) = open(&path);
        let population = populated(&cassie, &session, table, options);
        plant_obsolete_artifacts(&cassie, &population, table);
        let generation = cassie
            .midge
            .collection_generation(table)
            .expect("generation");
        let canonical = collection(&cassie, table);

        // Act
        set_document_write_failure_point(Some(DocumentWriteFailurePoint::VectorStatePublished));
        let interrupted = if algorithm == "hnsw" {
            cassie.midge.refresh_hnsw_indexes_for_collection(&canonical)
        } else {
            cassie
                .midge
                .refresh_ivfflat_indexes_for_collection(&canonical)
        };
        set_document_write_failure_point(None);

        // Assert
        assert!(interrupted.is_err(), "{algorithm}: interruption surfaces");
        let published = footprint(&cassie, table);
        assert_eq!(
            live_artifacts(&published, algorithm),
            population.len() as u64
        );
        drop(session);
        drop(cassie);
        let (reopened, session) = open(&path);
        assert_indexed_zero_fallback(
            &reopened,
            &session,
            table,
            algorithm,
            &population,
            algorithm,
        );
        assert_eq!(footprint(&reopened, table), published, "{algorithm}");
        assert_eq!(
            reopened
                .midge
                .collection_generation(table)
                .expect("generation"),
            generation
        );
        drop(session);
        drop(reopened);
        let _ = std::fs::remove_dir_all(path);
    }
}

#[test]
fn should_take_the_diagnosed_fallback_for_stale_state_then_return_to_indexed_service() {
    // Arrange
    use_local_storage();
    for (algorithm, options) in ALGORITHMS {
        let path = data_dir("vector_churn_stale_state");
        let table = "stale_state";
        let (cassie, session) = open(&path);
        let mut population = populated(&cassie, &session, table, options);
        let state_key = cassie
            .midge
            .vector_state_key_for_diagnostics(table, "embedding")
            .expect("state key");
        let stale_state = cassie
            .midge
            .raw_scan_prefix(StorageFamily::Data, &state_key)
            .expect("read state")
            .into_iter()
            .next()
            .expect("persisted state")
            .1;
        upsert(&cassie, table, "late", [0.5, 0.1, 0.0]);
        population.insert("late".to_string(), [0.5, 0.1, 0.0]);
        let mut tx = cassie
            .midge
            .data_tx(TransactionMode::ReadWrite)
            .expect("data transaction");
        tx.put(state_key, stale_state, None)
            .expect("restore stale state");
        tx.commit(WriteOptions::sync()).expect("commit stale state");
        let before = counters(&cassie, algorithm);

        // Act
        let fallback_rows = run_query(&cassie, &session, table);
        let fell_back = counters(&cassie, algorithm);
        let reason = last_fallback_reason(&cassie);
        let function_rows = run_function_query(&cassie, &session, table);
        rebuild(&cassie, table, algorithm);

        // Assert
        assert_matches_population(&cassie, table, &fallback_rows, &population, algorithm);
        assert_matches_population(&cassie, table, &function_rows, &population, algorithm);
        assert_eq!(fell_back.fallbacks, before.fallbacks + 1, "{algorithm}");
        assert_eq!(fell_back.executions, before.executions, "{algorithm}");
        assert_eq!(reason, "concurrent-source-change", "{algorithm}");
        assert!(
            vector_metric(&cassie, "ann_fallback_micros_total") > 0,
            "{algorithm}: fallback duration is measured"
        );
        assert_indexed_zero_fallback(&cassie, &session, table, algorithm, &population, algorithm);
        let generation = cassie
            .midge
            .collection_generation(table)
            .expect("generation");
        drop(session);
        drop(cassie);
        let (reopened, session) = open(&path);
        assert_indexed_zero_fallback(
            &reopened,
            &session,
            table,
            algorithm,
            &population,
            algorithm,
        );
        assert_eq!(
            reopened
                .midge
                .collection_generation(table)
                .expect("generation"),
            generation
        );
        drop(session);
        drop(reopened);
        let _ = std::fs::remove_dir_all(path);
    }
}
