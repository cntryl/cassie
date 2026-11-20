//! Seeded insert, replace, delete, aborted-write and rebuild sequence over both ANN indexes.

use super::support::{
    assert_matches_population, counters, create_index, create_table, footprint,
    index_state_generation, last_fallback_reason, rebuild, run_query, seed_population, seed_rows,
    upsert, vector_metric, with_memory_fixture, Population, Rng, ALGORITHMS,
};
use cassie::midge::adapter::{
    document_write_failure_point_test_guard, set_document_write_failure_point,
    DocumentWriteFailurePoint,
};

const SEED: u64 = 0x1364_0001;
const STEPS: usize = 24;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    Insert,
    Replace,
    Delete,
    Aborted,
    Rebuild,
}

/// The first five steps cover every kind once; the rest are drawn from the seed.
fn plan(rng: &mut Rng) -> Vec<Step> {
    let mut steps = vec![
        Step::Insert,
        Step::Replace,
        Step::Delete,
        Step::Aborted,
        Step::Rebuild,
    ];
    while steps.len() < STEPS {
        steps.push(match rng.below(5) {
            0 => Step::Insert,
            1 => Step::Replace,
            2 => Step::Delete,
            3 => Step::Aborted,
            _ => Step::Rebuild,
        });
    }
    steps
}

fn nth_id(population: &Population, rng: &mut Rng) -> String {
    population
        .keys()
        .nth(rng.below(population.len()))
        .expect("population is never empty")
        .clone()
}

#[test]
fn should_match_the_eligible_population_through_a_seeded_churn_sequence() {
    // Arrange
    let _failpoint_guard = document_write_failure_point_test_guard();
    for (algorithm, options) in ALGORITHMS {
        with_memory_fixture(|cassie, session| {
            let table = "churn_sequence";
            let mut rng = Rng::new(SEED);
            let mut population = seed_population(&mut rng);
            create_table(cassie, session, table);
            seed_rows(cassie, table, &population);
            create_index(cassie, session, table, options);
            let steps = plan(&mut rng);
            let mut next_insert = 1000;

            for (index, step) in steps.into_iter().enumerate() {
                let context = format!("{algorithm} step {index} {step:?}");
                let generation_before = cassie
                    .midge
                    .collection_generation(table)
                    .expect("collection generation");
                let rebuilds_before = vector_metric(cassie, "index_rebuilds_total");

                // Act
                match step {
                    Step::Insert => {
                        let id = format!("ins-{next_insert}");
                        let vector = rng.vector(next_insert);
                        next_insert += 1;
                        upsert(cassie, table, &id, vector);
                        population.insert(id, vector);
                    }
                    Step::Replace => {
                        let id = nth_id(&population, &mut rng);
                        let vector = rng.vector(next_insert);
                        next_insert += 1;
                        let count = population.len();
                        upsert(cassie, table, &id, vector);
                        population.insert(id, vector);
                        assert_eq!(population.len(), count, "{context}: same count");
                    }
                    Step::Delete => {
                        let id = nth_id(&population, &mut rng);
                        assert!(cassie
                            .midge
                            .delete_document(table, &id)
                            .expect("delete row"));
                        population.remove(&id);
                    }
                    Step::Aborted => {
                        let id = nth_id(&population, &mut rng);
                        set_document_write_failure_point(Some(
                            DocumentWriteFailurePoint::VectorState,
                        ));
                        let aborted = cassie.midge.put_document(
                            table,
                            Some(id),
                            super::support::payload("aborted", [9.0; 3]),
                        );
                        set_document_write_failure_point(None);
                        assert!(aborted.is_err(), "{context}: injected failure aborts");
                    }
                    Step::Rebuild => rebuild(cassie, table, algorithm),
                }

                // Assert
                let generation_after = cassie
                    .midge
                    .collection_generation(table)
                    .expect("collection generation");
                match step {
                    Step::Insert | Step::Replace | Step::Delete => {
                        assert!(generation_after > generation_before, "{context}");
                    }
                    Step::Aborted | Step::Rebuild => {
                        assert_eq!(generation_after, generation_before, "{context}");
                    }
                }
                assert_eq!(
                    index_state_generation(cassie, table),
                    generation_after,
                    "{context}: manifest generation follows the collection"
                );
                let before = counters(cassie, algorithm);
                let rows = run_query(cassie, session, table);
                let after = counters(cassie, algorithm);
                assert_matches_population(cassie, table, &rows, &population, &context);
                assert_eq!(after.executions, before.executions + 1, "{context}");
                assert_eq!(
                    after.fallbacks, before.fallbacks,
                    "{context}: zero fallback"
                );
                assert_eq!(after.discards, before.discards, "{context}");
                assert_eq!(
                    after.observed_generation, generation_after,
                    "{context}: observed generation label"
                );
                let artifacts = footprint(cassie, table);
                assert_eq!(
                    artifacts["normalized_keys"].as_u64(),
                    Some(population.len() as u64),
                    "{context}: obsolete normalized vectors are reclaimed"
                );
                let graph_or_lists = if algorithm == "hnsw" {
                    &artifacts["graph_node_keys"]
                } else {
                    &artifacts["membership_keys"]
                };
                assert_eq!(
                    graph_or_lists.as_u64(),
                    Some(population.len() as u64),
                    "{context}: obsolete index artifacts are reclaimed"
                );
                if step == Step::Rebuild {
                    assert_eq!(
                        vector_metric(cassie, "index_rebuilds_total"),
                        rebuilds_before + 1,
                        "{context}: rebuild counted"
                    );
                    assert_eq!(last_fallback_reason(cassie), "", "{context}");
                }
            }
        });
    }
}
