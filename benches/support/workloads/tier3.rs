use cassie::types::Value;
use serde_json::json;

use super::context::{BenchContext, ANALYTICAL_BENCHMARK_QUERY_MEMORY_BYTES};

const MIXED_INGEST_COLLECTION: &str = "bench_mixed_ingest";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryPreflightEvidence {
    pub selected_access_path: String,
    pub fallback_reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VectorAccessPath {
    Exact,
    Hnsw,
    IvfFlat,
}

impl VectorAccessPath {
    fn evidence_label(self) -> &'static str {
        match self {
            Self::Exact => "vector_exact",
            Self::Hnsw => "hnsw",
            Self::IvfFlat => "ivfflat",
        }
    }
}

pub fn configure_tier3_environment() {
    std::env::set_var("CASSIE_MIDGE_ALLOW_FALLBACK", "1");
    std::env::set_var("CASSIE_EXECUTION_RESULT_CACHE_ENABLED", "false");
    std::env::set_var(
        "CASSIE_QUERY_MEMORY_BUDGET_BYTES",
        ANALYTICAL_BENCHMARK_QUERY_MEMORY_BYTES.to_string(),
    );
    std::env::set_var("CASSIE_EMBEDDINGS_PROVIDER", "local");
    std::env::set_var("CASSIE_LOCAL_MODEL", "cassie-tier3-local");
    std::env::set_var("CASSIE_LOCAL_DIMENSIONS", "3");
}

pub fn execute_expected_query(
    context: &BenchContext,
    sql: &str,
    params: Vec<Value>,
    expected_rows: usize,
) -> usize {
    let result = context
        .cassie
        .execute_sql(&context.session, sql, params)
        .expect("Tier 3 representative query");
    assert_eq!(
        result.rows.len(),
        expected_rows,
        "Tier 3 representative query returned unexpected cardinality"
    );
    std::hint::black_box(result.rows.len())
}

pub fn assert_explain_contains(
    context: &BenchContext,
    sql: &str,
    params: Vec<Value>,
    expected_fragment: &str,
) -> QueryPreflightEvidence {
    let explain_sql = format!("EXPLAIN {sql}");
    let explain = context
        .cassie
        .execute_sql(&context.session, &explain_sql, params)
        .expect("Tier 3 representative EXPLAIN");
    let Some(Value::String(plan)) = explain.rows.first().and_then(|row| row.first()) else {
        panic!("Tier 3 representative EXPLAIN must return a textual plan");
    };
    assert!(
        plan.contains(expected_fragment),
        "Tier 3 plan did not contain '{expected_fragment}': {plan}"
    );
    query_preflight_evidence(plan)
}

pub fn assert_vector_preflight(
    context: &BenchContext,
    sql: &str,
    params: Vec<Value>,
    expected_fragment: &str,
    expected_fixture_rows: usize,
    expected_access_path: VectorAccessPath,
) -> QueryPreflightEvidence {
    let preflight = assert_explain_contains(context, sql, params, expected_fragment);
    assert_vector_index_ready(context, expected_fixture_rows, expected_access_path);
    QueryPreflightEvidence {
        selected_access_path: expected_access_path.evidence_label().to_string(),
        fallback_reason: preflight.fallback_reason,
    }
}

fn assert_vector_index_ready(
    context: &BenchContext,
    expected_fixture_rows: usize,
    expected: VectorAccessPath,
) {
    let definition = context
        .cassie
        .midge
        .get_vector_index_definition(&context.collection, "embedding")
        .expect("read benchmark vector index definition");
    if expected == VectorAccessPath::Exact {
        assert!(
            definition.as_ref().is_none_or(|record| {
                record.metadata.index_type == cassie::embeddings::VectorIndexType::BruteForce
            }),
            "exact vector benchmark must not retain an ANN index"
        );
        return;
    }

    let definition = definition.expect("ANN benchmark vector index definition");
    let expected_type = match expected {
        VectorAccessPath::Exact => unreachable!("exact vector path returned above"),
        VectorAccessPath::Hnsw => cassie::embeddings::VectorIndexType::Hnsw,
        VectorAccessPath::IvfFlat => cassie::embeddings::VectorIndexType::IvfFlat,
    };
    assert_eq!(
        definition.metadata.index_type, expected_type,
        "ANN benchmark vector index type"
    );
    let state = context
        .cassie
        .midge
        .get_vector_index_state(&context.collection, "embedding")
        .expect("read benchmark vector index state")
        .expect("generation-current benchmark vector index state");
    match expected {
        VectorAccessPath::Hnsw => {
            assert!(
                state.ivfflat_training.is_none(),
                "benchmark HNSW state must not retain IVFFlat training"
            );
            let graph = state.hnsw_graph.expect("ready benchmark HNSW graph");
            assert_eq!(
                graph.row_count, expected_fixture_rows,
                "benchmark HNSW graph row coverage"
            );
            assert_eq!(
                graph.nodes.len(),
                expected_fixture_rows,
                "benchmark HNSW node coverage"
            );
            assert_eq!(
                graph.dimensions, definition.metadata.dimensions,
                "benchmark HNSW dimensions"
            );
            assert_eq!(
                graph.metric, definition.metadata.metric,
                "benchmark HNSW metric"
            );
            assert!(
                graph.entry_point.is_some(),
                "benchmark HNSW graph must have an entry point"
            );
        }
        VectorAccessPath::IvfFlat => {
            assert!(
                state.hnsw_graph.is_none(),
                "benchmark IVFFlat state must not retain an HNSW graph"
            );
            let training = state
                .ivfflat_training
                .expect("ready benchmark IVFFlat training");
            assert_eq!(
                training.row_count, expected_fixture_rows,
                "benchmark IVFFlat training row coverage"
            );
            assert_eq!(
                cassie::vector::ivfflat::training_manifest_fallback_reason(
                    &training,
                    definition.metadata.dimensions,
                ),
                None,
                "benchmark IVFFlat training readiness"
            );
        }
        VectorAccessPath::Exact => unreachable!("exact vector path returned above"),
    }
}

fn query_preflight_evidence(plan: &str) -> QueryPreflightEvidence {
    let selected_access_path = plan_field(plan, "access_path")
        .unwrap_or_else(|| panic!("Tier 3 EXPLAIN did not report access_path: {plan}"));
    let fallback_reasons = plan
        .split_ascii_whitespace()
        .filter_map(|field| {
            let (key, value) = field.split_once('=')?;
            key.ends_with("fallback_reason")
                .then(|| clean_plan_value(value))
        })
        .collect::<Vec<_>>();
    let fallback_reason = fallback_reasons
        .iter()
        .find(|reason| reason.as_str() != "none")
        .cloned()
        .or_else(|| fallback_reasons.first().cloned())
        .unwrap_or_else(|| "none".to_string());
    QueryPreflightEvidence {
        selected_access_path,
        fallback_reason,
    }
}

fn plan_field(plan: &str, expected: &str) -> Option<String> {
    plan.split_ascii_whitespace().find_map(|field| {
        let (key, value) = field.split_once('=')?;
        (key == expected).then(|| clean_plan_value(value))
    })
}

fn clean_plan_value(value: &str) -> String {
    value
        .trim_matches(|character: char| matches!(character, ',' | '|' | '[' | ']'))
        .to_string()
}

pub fn assert_fixture_boundaries(
    context: &BenchContext,
    collection: &str,
    first_id: &str,
    last_id: &str,
) {
    for id in [first_id, last_id] {
        assert!(
            context
                .cassie
                .midge
                .get_document(collection, id)
                .expect("read Tier 3 fixture boundary")
                .is_some(),
            "Tier 3 fixture must contain boundary document '{id}'"
        );
    }
}

pub fn assert_result_cache_disabled(context: &BenchContext) {
    let metrics = context.cassie.metrics();
    assert_eq!(
        metrics["execution_result_cache"]["hits"]
            .as_u64()
            .unwrap_or_default(),
        0,
        "Tier 3 execution-result cache must remain isolated"
    );
    assert_eq!(
        metrics["execution_result_cache"]["entries"]
            .as_u64()
            .unwrap_or_default(),
        0,
        "Tier 3 execution-result cache must remain empty"
    );
}

pub fn prepare_mixed_fixture(context: &BenchContext) {
    context
        .cassie
        .execute_sql(
            &context.session,
            "CREATE TABLE bench_mixed_ingest (title TEXT, status TEXT)",
            vec![],
        )
        .expect("create Tier 3 mixed ingest collection");
}

pub fn mixed_query_ingest_retrieval(context: &BenchContext, nonce: u64) -> usize {
    let query = context
        .cassie
        .execute_sql(
            &context.session,
            "SELECT id FROM bench_documents WHERE status = $1 AND score >= $2 ORDER BY score DESC LIMIT 20",
            vec![Value::String("approved".to_string()), Value::Int64(90)],
        )
        .expect("Tier 3 mixed relational query");
    assert_eq!(query.rows.len(), 20, "Tier 3 mixed query cardinality");

    let marker = format!("tier3-mixed-marker-{nonce}");
    let id = context
        .cassie
        .ingest_document(
            MIXED_INGEST_COLLECTION,
            json!({
                "title": marker,
                "status": "approved",
            }),
        )
        .expect("Tier 3 mixed ingest");
    let retrieval = context
        .cassie
        .execute_sql(
            &context.session,
            "SELECT title FROM bench_mixed_ingest WHERE title = $1",
            vec![Value::String(marker.clone())],
        )
        .expect("Tier 3 mixed point retrieval");
    let search = context
        .cassie
        .execute_sql(
            &context.session,
            "SELECT id, search_score(body, $1) AS score FROM bench_documents WHERE search(body, $1) ORDER BY score DESC LIMIT 5",
            vec![Value::String("alpha".to_string())],
        )
        .expect("Tier 3 mixed full-text retrieval");
    let cleanup = context
        .cassie
        .execute_sql(
            &context.session,
            "DELETE FROM bench_mixed_ingest WHERE title = $1",
            vec![Value::String(marker.clone())],
        )
        .expect("Tier 3 mixed cleanup");

    assert_eq!(
        retrieval.rows,
        vec![vec![Value::String(marker)]],
        "Tier 3 mixed point retrieval must observe the ingested document"
    );
    assert_eq!(search.rows.len(), 5, "Tier 3 mixed retrieval cardinality");
    assert_eq!(cleanup.command, "DELETE 1", "Tier 3 mixed cleanup count");
    assert!(
        context
            .cassie
            .midge
            .get_document(MIXED_INGEST_COLLECTION, &id)
            .expect("verify Tier 3 mixed cleanup")
            .is_none(),
        "Tier 3 mixed cleanup must remove the ingested document"
    );
    std::hint::black_box(query.rows.len() + retrieval.rows.len() + search.rows.len() + 1)
}
