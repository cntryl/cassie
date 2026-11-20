//! Deterministic read/write barriers at vector index publication and at rerank.

use std::sync::{Arc, Barrier};
use std::time::Duration;

use cassie::app::{Cassie, CassieSession};
use cassie::midge::adapter::{
    document_write_failure_point_test_guard, schema_write_conflict_test_guard,
    schema_write_conflict_worker_guard, set_document_write_failure_point,
    set_schema_write_commit_barriers, DocumentWriteFailurePoint, SchemaWritePausePoint,
    StorageFamily,
};
use cntryl_midge::{TransactionMode, WriteOptions};

use super::support::{
    assert_matches_population, assert_matches_snapshot, counters, create_index, create_table,
    last_fallback_reason, oracle, payload, rebuild, run_function_query, run_query, seed_population,
    seed_rows, upsert, vector_metric, with_memory_fixture, Population, Rng, ALGORITHMS, QUERY,
};

#[derive(Debug, Clone, Copy)]
enum Write {
    InsertCloser,
    ReplaceTopFar,
    DeleteTop,
}

fn apply(cassie: &Cassie, table: &str, population: &mut Population, write: Write) {
    let top = oracle(population)[0].0.clone();
    match write {
        Write::InsertCloser => {
            upsert(cassie, table, "closest", QUERY);
            population.insert("closest".to_string(), QUERY);
        }
        Write::ReplaceTopFar => {
            upsert(cassie, table, &top, [-9.0, -9.0, -9.0]);
            population.insert(top, [-9.0, -9.0, -9.0]);
        }
        Write::DeleteTop => {
            assert!(cassie.midge.delete_document(table, &top).expect("delete"));
            population.remove(&top);
        }
    }
}

fn fixture(cassie: &Cassie, session: &CassieSession, table: &str, options: &str) -> Population {
    let mut rng = Rng::new(0x1364_0002);
    let population = seed_population(&mut rng);
    create_table(cassie, session, table);
    seed_rows(cassie, table, &population);
    create_index(cassie, session, table, options);
    population
}

fn read_on_another_thread<T: Send>(read: impl FnOnce() -> T + Send) -> T {
    std::thread::scope(|scope| {
        let (sender, receiver) = std::sync::mpsc::channel();
        scope.spawn(move || sender.send(read()).expect("send observation"));
        receiver
            .recv_timeout(Duration::from_secs(60))
            .expect("reader must not wait on the paused publication")
    })
}

#[test]
fn should_label_a_paused_reader_with_the_generation_it_read_after_each_committed_write() {
    // Arrange
    for (algorithm, options) in ALGORITHMS {
        for write in [Write::InsertCloser, Write::ReplaceTopFar, Write::DeleteTop] {
            with_memory_fixture(|cassie, session| {
                let table = "paused_reader";
                let mut population = fixture(cassie, session, table, options);
                let captured = population.clone();
                let context = format!("{algorithm} {write:?}");
                let generation = cassie
                    .midge
                    .collection_generation(table)
                    .expect("generation");
                let before = counters(cassie, algorithm);
                let selected = Arc::new(Barrier::new(2));
                let resume = Arc::new(Barrier::new(2));
                cassie::executor::set_vector_ann_rerank_barriers(
                    cassie,
                    Some(Arc::clone(&selected)),
                    Some(Arc::clone(&resume)),
                );

                // Act
                let paused = std::thread::scope(|scope| {
                    let reader = scope
                        .spawn(|| run_query(cassie, &cassie.create_session("reader", None), table));
                    selected.wait();
                    apply(cassie, table, &mut population, write);
                    resume.wait();
                    reader.join().expect("reader thread")
                });
                let during = counters(cassie, algorithm);
                let committed = cassie
                    .midge
                    .collection_generation(table)
                    .expect("generation");
                let fresh = run_query(cassie, session, table);
                let after = counters(cassie, algorithm);

                // Assert
                assert!(committed > generation, "{context}");
                assert_matches_snapshot(&paused, &captured, &context);
                assert_eq!(during.executions, before.executions + 1, "{context}");
                assert_eq!(during.fallbacks, before.fallbacks, "{context}");
                assert_eq!(
                    during.observed_generation, generation,
                    "{context}: the candidate read never receives the post-write label"
                );
                assert_matches_population(cassie, table, &fresh, &population, &context);
                assert_eq!(after.executions, during.executions + 1, "{context}");
                assert_eq!(after.fallbacks, during.fallbacks, "{context}");
                assert_eq!(after.observed_generation, committed, "{context}");
            });
        }
    }
}

fn delete_row_storage_key(cassie: &Cassie, table: &str, id: &str) {
    let prefix = cassie
        .midge
        .row_store_prefixes_for_diagnostics(table)
        .expect("row prefixes")
        .remove(0);
    let key = cassie
        .midge
        .raw_scan_prefix(StorageFamily::Data, &prefix)
        .expect("scan row storage")
        .into_iter()
        .map(|(key, _)| key)
        .find(|key| key.windows(id.len()).any(|window| window == id.as_bytes()))
        .expect("stored row key");
    let mut tx = cassie
        .midge
        .data_tx(TransactionMode::ReadWrite)
        .expect("data transaction");
    tx.delete(key).expect("delete row");
    tx.commit(WriteOptions::sync())
        .expect("commit row deletion");
}

#[test]
fn should_discard_the_attempt_and_its_counters_when_a_candidate_row_is_missing() {
    // Arrange
    for (algorithm, options) in ALGORITHMS {
        with_memory_fixture(|cassie, session| {
            let table = "missing_candidate";
            let mut population = fixture(cassie, session, table, options);
            let healthy = run_query(cassie, session, table);
            assert_matches_population(cassie, table, &healthy, &population, algorithm);
            let top = oracle(&population)[0].0.clone();
            delete_row_storage_key(cassie, table, &top);
            population.remove(&top);
            let before = counters(cassie, algorithm);

            // Act
            let rows = run_query(cassie, session, table);
            let after = counters(cassie, algorithm);

            // Assert
            assert_eq!(last_fallback_reason(cassie), "concurrent-source-change");
            assert_eq!(after.fallbacks, before.fallbacks + 1, "{algorithm}");
            assert_eq!(after.discards, before.discards + 1, "{algorithm}");
            assert_eq!(
                after.executions, before.executions,
                "{algorithm}: successful-path counters are discarded"
            );
            assert_eq!(
                after.observed_generation, before.observed_generation,
                "{algorithm}: the discarded attempt receives no generation label"
            );
            assert_matches_population(cassie, table, &rows, &population, algorithm);
            let function_before = counters(cassie, algorithm);
            let function_rows = run_function_query(cassie, session, table);
            assert_matches_population(cassie, table, &function_rows, &population, algorithm);
            assert_eq!(
                counters(cassie, algorithm).fallbacks,
                function_before.fallbacks + 1,
                "{algorithm}"
            );
            assert!(
                vector_metric(cassie, "ann_fallback_micros_total") > 0,
                "{algorithm}: fallback duration is measured"
            );
        });
    }
}

#[test]
fn should_serve_the_attempt_when_an_aborted_write_leaves_the_generation_unchanged() {
    // Arrange
    let _failpoint_guard = document_write_failure_point_test_guard();
    for (algorithm, options) in ALGORITHMS {
        with_memory_fixture(|cassie, session| {
            let table = "aborted_attempt";
            let population = fixture(cassie, session, table, options);
            let generation = cassie
                .midge
                .collection_generation(table)
                .expect("generation");
            let before = counters(cassie, algorithm);
            let selected = Arc::new(Barrier::new(2));
            let resume = Arc::new(Barrier::new(2));
            cassie::executor::set_vector_ann_rerank_barriers(
                cassie,
                Some(Arc::clone(&selected)),
                Some(Arc::clone(&resume)),
            );

            // Act
            let (rows, aborted) = std::thread::scope(|scope| {
                let reader = scope
                    .spawn(|| run_query(cassie, &cassie.create_session("reader", None), table));
                selected.wait();
                set_document_write_failure_point(Some(DocumentWriteFailurePoint::VectorState));
                let aborted = cassie.midge.put_document(
                    table,
                    Some("never".to_string()),
                    payload("never", QUERY),
                );
                set_document_write_failure_point(None);
                resume.wait();
                (reader.join().expect("reader thread"), aborted)
            });
            let after = counters(cassie, algorithm);

            // Assert
            assert!(aborted.is_err(), "{algorithm}: injected failure aborts");
            assert_eq!(
                cassie
                    .midge
                    .collection_generation(table)
                    .expect("generation"),
                generation
            );
            assert_matches_population(cassie, table, &rows, &population, algorithm);
            assert_eq!(after.executions, before.executions + 1, "{algorithm}");
            assert_eq!(after.fallbacks, before.fallbacks, "{algorithm}");
            assert_eq!(after.discards, before.discards, "{algorithm}");
            assert_eq!(after.observed_generation, generation, "{algorithm}");
        });
    }
}

#[test]
fn should_serve_the_previous_generation_while_a_write_waits_at_index_publication() {
    // Arrange
    let _test_guard = schema_write_conflict_test_guard();
    for (algorithm, options) in ALGORITHMS {
        with_memory_fixture(|cassie, session| {
            let table = "publication_pause";
            let population = fixture(cassie, session, table, options);
            let generation = cassie
                .midge
                .collection_generation(table)
                .expect("generation");
            let before = counters(cassie, algorithm);
            let mut expected_after = population.clone();
            expected_after.insert("closest".to_string(), QUERY);

            let ready = Arc::new(Barrier::new(2));
            let resume = Arc::new(Barrier::new(2));
            set_schema_write_commit_barriers(
                Some(SchemaWritePausePoint::VectorIndexPublication),
                Some(Arc::clone(&ready)),
                Some(Arc::clone(&resume)),
            );

            // Act
            let (during, finished) = std::thread::scope(|scope| {
                let write = scope.spawn(|| {
                    let _worker = schema_write_conflict_worker_guard();
                    upsert(cassie, table, "closest", QUERY);
                });
                ready.wait();
                set_schema_write_commit_barriers(None, None, None);
                let during = read_on_another_thread(|| {
                    (
                        run_query(cassie, &cassie.create_session("reader", None), table),
                        counters(cassie, algorithm),
                        cassie
                            .midge
                            .collection_generation(table)
                            .expect("generation"),
                    )
                });
                resume.wait();
                write.join().expect("writer thread");
                (during, counters(cassie, algorithm))
            });
            let committed = cassie
                .midge
                .collection_generation(table)
                .expect("generation");
            let fresh = run_query(cassie, session, table);
            let after = counters(cassie, algorithm);

            // Assert
            assert_matches_population(cassie, table, &during.0, &population, algorithm);
            assert_eq!(during.2, generation, "{algorithm}: write not yet published");
            assert_eq!(during.1.fallbacks, before.fallbacks, "{algorithm}");
            assert_eq!(during.1.executions, before.executions + 1, "{algorithm}");
            assert_eq!(during.1.observed_generation, generation, "{algorithm}");
            assert!(committed > generation, "{algorithm}");
            assert_eq!(finished.fallbacks, before.fallbacks, "{algorithm}");
            assert_matches_population(cassie, table, &fresh, &expected_after, algorithm);
            assert_eq!(after.observed_generation, committed, "{algorithm}");
            assert_eq!(after.fallbacks, before.fallbacks, "{algorithm}");
        });
    }
}

#[test]
fn should_keep_serving_the_published_index_while_a_rebuild_waits_to_publish() {
    // Arrange
    let _test_guard = schema_write_conflict_test_guard();
    for (algorithm, options) in ALGORITHMS {
        with_memory_fixture(|cassie, session| {
            let table = "rebuild_pause";
            let population = fixture(cassie, session, table, options);
            let generation = cassie
                .midge
                .collection_generation(table)
                .expect("generation");
            let before = counters(cassie, algorithm);
            let rebuilds = vector_metric(cassie, "index_rebuilds_total");
            let ready = Arc::new(Barrier::new(2));
            let resume = Arc::new(Barrier::new(2));
            set_schema_write_commit_barriers(
                Some(SchemaWritePausePoint::VectorIndexRebuildCommit),
                Some(Arc::clone(&ready)),
                Some(Arc::clone(&resume)),
            );

            // Act
            let (during, mid_rebuilds) = std::thread::scope(|scope| {
                let rebuild_worker = scope.spawn(|| {
                    let _worker = schema_write_conflict_worker_guard();
                    rebuild(cassie, table, algorithm);
                });
                ready.wait();
                set_schema_write_commit_barriers(None, None, None);
                let during = read_on_another_thread(|| {
                    (
                        run_query(cassie, &cassie.create_session("reader", None), table),
                        counters(cassie, algorithm),
                    )
                });
                let mid_rebuilds = vector_metric(cassie, "index_rebuilds_total");
                resume.wait();
                rebuild_worker.join().expect("rebuild thread");
                (during, mid_rebuilds)
            });
            let published = cassie
                .midge
                .collection_generation(table)
                .expect("generation");
            let fresh = run_query(cassie, session, table);
            let after = counters(cassie, algorithm);

            // Assert
            assert_matches_population(cassie, table, &during.0, &population, algorithm);
            assert_eq!(during.1.fallbacks, before.fallbacks, "{algorithm}");
            assert_eq!(
                mid_rebuilds, rebuilds,
                "{algorithm}: rebuild not yet counted"
            );
            assert_eq!(
                published, generation,
                "{algorithm}: rebuild keeps the generation"
            );
            assert_eq!(
                vector_metric(cassie, "index_rebuilds_total"),
                rebuilds + 1,
                "{algorithm}"
            );
            assert!(
                vector_metric(cassie, "index_last_rebuild_bytes_written") > 0,
                "{algorithm}: manifest bytes are counted"
            );
            assert_matches_population(cassie, table, &fresh, &population, algorithm);
            assert_eq!(after.fallbacks, before.fallbacks, "{algorithm}");
            assert_eq!(after.observed_generation, generation, "{algorithm}");
        });
    }
}
