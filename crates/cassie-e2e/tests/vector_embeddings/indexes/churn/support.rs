//! Shared fixture for seeded vector-index churn qualification (#1364).
//!
//! The population model mirrors the committed rows, so every observed query is
//! compared with the exact eligible population for the generation it observed.

use std::collections::BTreeMap;

use cassie::app::{Cassie, CassieSession};
use cassie::config::{EmbeddingsRuntimeConfig, LocalRuntimeConfig};
use cassie::types::Value;

pub(super) const ALGORITHMS: [(&str, &str); 2] = [
    (
        "hnsw",
        "index_type = hnsw, m = 16, ef_construction = 64, ef_search = 64",
    ),
    (
        "ivfflat",
        "index_type = ivfflat, lists = 4, probes = 4, training_sample_size = 30, training_seed = 7",
    ),
];

pub(super) const QUERY: [f32; 3] = [1.0, 0.2, 0.0];
pub(super) const TOP: usize = 5;
pub(super) const SEED_ROWS: usize = 36;

pub(super) type Population = BTreeMap<String, [f32; 3]>;

/// Deterministic generator; the same seed always yields the same churn.
pub(super) struct Rng(u64);

impl Rng {
    pub(super) fn new(state: u64) -> Self {
        Self(state)
    }

    pub(super) fn next_u32(&mut self) -> u32 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        u32::try_from(self.0 >> 33).expect("31 bit lane")
    }

    pub(super) fn below(&mut self, bound: usize) -> usize {
        usize::try_from(self.next_u32()).expect("lane fits usize") % bound
    }

    /// Distinct, exactly representable coordinates avoid distance ties.
    pub(super) fn vector(&mut self, ordinal: usize) -> [f32; 3] {
        let mut lane = || {
            let steps = i16::try_from(self.next_u32() % 129).expect("step") - 64;
            f32::from(steps) / 32.0
        };
        let jitter = f32::from(u16::try_from(ordinal % 1000).expect("ordinal")) / 65_536.0;
        [lane() + jitter, lane(), lane()]
    }
}

pub(super) fn seed_population(rng: &mut Rng) -> Population {
    (0..SEED_ROWS)
        .map(|index| (format!("row-{index:03}"), rng.vector(index)))
        .collect()
}

pub(super) fn collection(cassie: &Cassie, table: &str) -> String {
    crate::support_sql::canonical_test_collection(cassie, table)
}

pub(super) fn payload(id: &str, vector: [f32; 3]) -> serde_json::Value {
    serde_json::json!({"body": id, "label": id, "embedding": vector})
}

pub(super) fn create_table(cassie: &Cassie, session: &CassieSession, table: &str) {
    create_table_with(cassie, session, table, 3);
}

pub(super) fn create_table_with(
    cassie: &Cassie,
    session: &CassieSession,
    table: &str,
    dimensions: usize,
) {
    cassie
        .execute_sql(
            session,
            &format!(
                "CREATE TABLE {table} (body TEXT, label TEXT, embedding VECTOR({dimensions}))"
            ),
            vec![],
        )
        .expect("create vector table");
}

pub(super) fn seed_rows(cassie: &Cassie, table: &str, population: &Population) {
    let documents = population
        .iter()
        .map(|(id, vector)| (Some(id.clone()), payload(id, *vector)))
        .collect();
    cassie
        .midge
        .put_fresh_documents(table, documents)
        .expect("seed churn rows");
}

pub(super) fn create_index(cassie: &Cassie, session: &CassieSession, table: &str, options: &str) {
    cassie
        .execute_sql(
            session,
            &format!(
                "CREATE INDEX {table}_idx ON {table} USING vector (embedding) \
                 WITH (source_field = body, metric = l2, {options})"
            ),
            vec![],
        )
        .expect("create vector index");
}

pub(super) fn upsert(cassie: &Cassie, table: &str, id: &str, vector: [f32; 3]) {
    cassie
        .midge
        .put_document(table, Some(id.to_string()), payload(id, vector))
        .expect("upsert churn row");
}

pub(super) fn function_ranking_sql(table: &str) -> String {
    let literal = format!("[{},{},{}]", QUERY[0], QUERY[1], QUERY[2]);
    format!(
        "SELECT id, vector_distance(embedding, '{literal}') AS distance FROM {table} \
         ORDER BY distance ASC LIMIT {TOP}"
    )
}

pub(super) fn ranking_sql(table: &str) -> String {
    let literal = format!("[{},{},{}]", QUERY[0], QUERY[1], QUERY[2]);
    format!(
        "SELECT label, embedding <-> '{literal}' AS distance FROM {table} \
         ORDER BY embedding <-> '{literal}' LIMIT {TOP}"
    )
}

pub(super) fn l2(vector: [f32; 3]) -> f64 {
    vector
        .iter()
        .zip(QUERY)
        .map(|(left, right)| (f64::from(*left) - f64::from(right)).powi(2))
        .sum::<f64>()
        .sqrt()
}

/// Exact eligible top-k of a population: distance, then identity.
pub(super) fn oracle(population: &Population) -> Vec<(String, f64)> {
    let mut ranked = population
        .iter()
        .map(|(id, vector)| (id.clone(), l2(*vector)))
        .collect::<Vec<_>>();
    ranked.sort_by(|left, right| {
        left.1
            .total_cmp(&right.1)
            .then_with(|| left.0.cmp(&right.0))
    });
    ranked.truncate(TOP);
    ranked
}

/// Vector-section counters a query can move.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Counters {
    pub(super) executions: u64,
    pub(super) fallbacks: u64,
    pub(super) discards: u64,
    pub(super) observed_generation: u64,
}

pub(super) fn counters(cassie: &Cassie, algorithm: &str) -> Counters {
    let metrics = cassie.metrics();
    let vector = &metrics["vector"];
    let read = |name: &str| vector[name].as_u64().expect("vector counter");
    Counters {
        executions: read(&format!("{algorithm}_executions")),
        fallbacks: read(&format!("{algorithm}_fallbacks")),
        discards: read("ann_stale_attempt_discards_total"),
        observed_generation: read("last_ann_observed_generation"),
    }
}

pub(super) fn last_fallback_reason(cassie: &Cassie) -> String {
    cassie.metrics()["vector"]["last_fallback_reason"]
        .as_str()
        .unwrap_or_default()
        .to_string()
}

pub(super) fn run_query(
    cassie: &Cassie,
    session: &CassieSession,
    table: &str,
) -> Vec<(String, f64)> {
    run_ranking(cassie, session, &ranking_sql(table))
}

pub(super) fn run_function_query(
    cassie: &Cassie,
    session: &CassieSession,
    table: &str,
) -> Vec<(String, f64)> {
    run_ranking(cassie, session, &function_ranking_sql(table))
}

fn run_ranking(cassie: &Cassie, session: &CassieSession, statement: &str) -> Vec<(String, f64)> {
    let result = cassie
        .execute_sql(session, statement, vec![])
        .expect("vector top-k");
    result
        .rows()
        .iter()
        .map(|row| {
            let Value::String(label) = &row[0] else {
                panic!("label must be text");
            };
            let Value::Float64(distance) = row[1] else {
                panic!("distance must be Float64");
            };
            (label.clone(), distance)
        })
        .collect()
}

/// Every candidate's identity, stored vector and exact rerank score must equal
/// the exact eligible population.
pub(super) fn assert_matches_population(
    cassie: &Cassie,
    table: &str,
    rows: &[(String, f64)],
    population: &Population,
    context: &str,
) {
    assert_matches_snapshot(rows, population, context);
    for (label, _) in rows {
        let stored = cassie
            .midge
            .get_document(table, label)
            .expect("read candidate row")
            .unwrap_or_else(|| panic!("{context}: candidate {label} is not committed"));
        let vector = stored.payload["embedding"]
            .as_array()
            .expect("stored vector")
            .iter()
            .map(|component| component.as_f64().expect("component"))
            .collect::<Vec<_>>();
        for (stored, model) in vector.iter().zip(population[label]) {
            assert!(
                (stored - f64::from(model)).abs() < 1e-6,
                "{context}: stored vector of {label} differs from the eligible population"
            );
        }
    }
}

/// Identity and exact rerank score of every candidate against the population a
/// reader observed. Scores are recomputed from the population's vectors.
pub(super) fn assert_matches_snapshot(
    rows: &[(String, f64)],
    population: &Population,
    context: &str,
) {
    let expected = oracle(population);
    assert_eq!(
        rows.iter().map(|row| row.0.as_str()).collect::<Vec<_>>(),
        expected
            .iter()
            .map(|row| row.0.as_str())
            .collect::<Vec<_>>(),
        "{context}: candidate identities"
    );
    for ((label, distance), (_, expected_distance)) in rows.iter().zip(&expected) {
        assert!(
            (distance - expected_distance).abs() < 1e-5,
            "{context}: exact rerank score of {label}: {distance} != {expected_distance}"
        );
    }
}

pub(super) fn index_state_generation(cassie: &Cassie, table: &str) -> u64 {
    cassie
        .midge
        .get_vector_index_state(table, "embedding")
        .expect("read vector index state")
        .expect("persisted vector index state")
        .built_generation
}

pub(super) fn footprint(cassie: &Cassie, table: &str) -> serde_json::Value {
    cassie
        .midge
        .vector_index_artifact_footprint_for_diagnostics(table, "embedding")
        .expect("vector index footprint")
}

pub(super) fn rebuild(cassie: &Cassie, table: &str, algorithm: &str) {
    let collection = collection(cassie, table);
    let refreshed = if algorithm == "hnsw" {
        cassie
            .midge
            .refresh_hnsw_indexes_for_collection(&collection)
    } else {
        cassie
            .midge
            .refresh_ivfflat_indexes_for_collection(&collection)
    }
    .expect("rebuild vector index");
    assert_eq!(refreshed, 1, "one index rebuilt");
}

pub(super) fn vector_metric(cassie: &Cassie, name: &str) -> u64 {
    cassie.metrics()["vector"][name]
        .as_u64()
        .unwrap_or_else(|| panic!("vector metric {name}"))
}

/// Runs `test` on a memory-mode fixture with the vector extension installed.
pub(super) fn with_memory_fixture(test: impl FnOnce(&Cassie, &CassieSession)) {
    crate::support_read_equivalence::with_fixture("vector_index_churn", test);
}

/// Memory-mode fixture whose local embedding provider matches `dimensions`
/// and whose per-query memory budget is `query_memory_budget_bytes`.
pub(super) fn with_memory_fixture_dimensions(
    dimensions: usize,
    query_memory_budget_bytes: usize,
    test: impl FnOnce(&Cassie, &CassieSession),
) {
    crate::support_read_equivalence::with_fixture_config(
        "vector_index_churn",
        |config| {
            config.limits.query_memory_budget_bytes = query_memory_budget_bytes;
            config.embeddings = EmbeddingsRuntimeConfig::Local(LocalRuntimeConfig {
                model: "deterministic-test".into(),
                dimensions,
            });
        },
        test,
    );
}
