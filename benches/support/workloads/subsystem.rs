#![allow(dead_code)]

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use cassie::app::{ProjectionReplayBatch, ProjectionReplayEvent};
pub use cassie::benchmark::SubsystemExecutorKernel;
use cassie::catalog::Catalog;
use cassie::config::{CassieRuntimeLimits, ExecutionResultCacheEnabled};
use cassie::embeddings::{
    DistanceMetric, HnswIndexOptions, IvfFlatTrainingState, NormalizedVectorRecord,
};
use cassie::executor::QueryResult;
use cassie::planner::logical::{self, LogicalPlan};
use cassie::planner::physical::{self, PhysicalPlan};
use cassie::runtime::{ExecutionMode, ExecutionResultCacheKey, PlanCacheKey, RuntimeState};
use cassie::search::inverted_index::InvertedIndex;
use cassie::sql::ast::ParsedStatement;
use cassie::sql::binder::{self, BoundStatement};
use cassie::types::{DataType, Value};
use serde_json::json;

use super::context::{replay_context, BenchContext};

const PLANNING_SQL: &str =
    "SELECT id, title FROM bench_documents WHERE score >= $1 ORDER BY id LIMIT 20";
const VECTOR_DIMENSIONS: usize = 3;
const IVFFLAT_LISTS: usize = 16;

/// Fixed SQL inputs for the parser-only owner.
pub struct ParserFixture {
    statements: Vec<&'static str>,
}

impl ParserFixture {
    #[must_use]
    pub fn new(statements: usize) -> Self {
        assert_tier2_fixture(statements);
        Self {
            statements: vec![PLANNING_SQL; statements],
        }
    }

    #[must_use]
    pub fn parse(&self) -> u64 {
        let parsed = self
            .statements
            .iter()
            .map(|sql| cassie::sql::parse_statement(sql).expect("benchmark SQL should parse"))
            .collect::<Vec<_>>();
        std::hint::black_box(parsed);
        fixture_count(self.statements.len())
    }
}

/// Parsed statements prepared for binder-only measurements.
pub struct BindingFixture {
    catalog: Catalog,
    parsed: Vec<ParsedStatement>,
}

impl BindingFixture {
    #[must_use]
    pub fn new(statements: usize) -> Self {
        assert_tier2_fixture(statements);
        Self {
            catalog: planning_catalog(),
            parsed: parse_planning_statements(statements),
        }
    }

    #[must_use]
    pub fn bind(&self) -> u64 {
        let bound = self
            .parsed
            .iter()
            .cloned()
            .map(|statement| {
                binder::bind(statement, &self.catalog).expect("benchmark SQL should bind")
            })
            .collect::<Vec<_>>();
        std::hint::black_box(bound);
        fixture_count(self.parsed.len())
    }
}

/// Bound statements prepared for logical-planner-only measurements.
pub struct LogicalPlanningFixture {
    bound: Vec<BoundStatement>,
}

impl LogicalPlanningFixture {
    #[must_use]
    pub fn new(statements: usize) -> Self {
        assert_tier2_fixture(statements);
        Self {
            bound: bind_planning_statements(statements),
        }
    }

    #[must_use]
    pub fn logical_plan(&self) -> u64 {
        let plans = self
            .bound
            .iter()
            .map(|statement| logical::plan(statement).expect("benchmark SQL should plan"))
            .collect::<Vec<_>>();
        std::hint::black_box(plans);
        fixture_count(self.bound.len())
    }
}

/// Logical plans prepared for physical-planner-only measurements.
pub struct PhysicalPlanningFixture {
    logical: Vec<LogicalPlan>,
}

impl PhysicalPlanningFixture {
    #[must_use]
    pub fn new(statements: usize) -> Self {
        assert_tier2_fixture(statements);
        let logical = bind_planning_statements(statements)
            .iter()
            .map(|statement| logical::plan(statement).expect("benchmark SQL should plan"))
            .collect::<Vec<_>>();
        Self { logical }
    }

    #[must_use]
    pub fn physical_plan(&self) -> u64 {
        let plans = self
            .logical
            .iter()
            .cloned()
            .map(physical::build)
            .collect::<Vec<_>>();
        std::hint::black_box(plans);
        fixture_count(self.logical.len())
    }

    fn sample_physical_plan(&self) -> PhysicalPlan {
        physical::build(
            self.logical
                .first()
                .expect("planning fixture should contain a plan")
                .clone(),
        )
    }
}

/// Raw bind values prepared outside the production parameter-decoding measurement.
pub struct ParameterBindingFixture {
    kernel: cassie::benchmark::PgwireParameterBindingKernel,
}

impl ParameterBindingFixture {
    #[must_use]
    pub fn new(parameters: usize) -> Self {
        assert_tier2_fixture(parameters);
        Self {
            kernel: cassie::benchmark::PgwireParameterBindingKernel::with_parameters(parameters),
        }
    }

    #[must_use]
    pub fn bind_parameters(&self) -> u64 {
        let decoded = self.kernel.decode();
        assert_eq!(decoded.len(), self.kernel.parameter_count());
        std::hint::black_box(decoded);
        fixture_count(self.kernel.parameter_count())
    }
}

fn planning_catalog() -> Catalog {
    let catalog = Catalog::new();
    catalog.register_collection(
        "bench_documents",
        vec![
            ("id".to_string(), DataType::Text),
            ("title".to_string(), DataType::Text),
            ("score".to_string(), DataType::Int),
        ],
    );
    catalog
}

fn parse_planning_statements(statements: usize) -> Vec<ParsedStatement> {
    (0..statements)
        .map(|_| cassie::sql::parse_statement(PLANNING_SQL).expect("benchmark SQL should parse"))
        .collect()
}

fn bind_planning_statements(statements: usize) -> Vec<BoundStatement> {
    let catalog = planning_catalog();
    parse_planning_statements(statements)
        .into_iter()
        .map(|statement| binder::bind(statement, &catalog).expect("benchmark SQL should bind"))
        .collect()
}

/// Direct production plan-cache and execution-result-cache operations.
pub struct CacheFixture {
    plan_runtime: Arc<RuntimeState>,
    result_runtime: Option<Arc<RuntimeState>>,
    plan_hit_key: PlanCacheKey,
    plan_miss_key: PlanCacheKey,
    result_hit_key: ExecutionResultCacheKey,
}

impl CacheFixture {
    #[must_use]
    pub fn new(entries: usize, include_result_cache: bool) -> Self {
        assert_tier2_fixture(entries);
        let planning = PhysicalPlanningFixture::new(1);
        let plan = Arc::new(planning.sample_physical_plan());
        let plan_limits = CassieRuntimeLimits {
            plan_cache_entries: entries,
            execution_result_cache_enabled: ExecutionResultCacheEnabled::disabled(),
            ..CassieRuntimeLimits::default()
        };
        let plan_runtime = Arc::new(RuntimeState::new(plan_limits));
        for fingerprint in 0..entries {
            plan_runtime.plan_cache_store(plan_key(usize_to_u64(fingerprint)), plan.clone(), false);
        }
        let plan_hit_key = plan_key(usize_to_u64(entries.saturating_sub(1)));
        let plan_miss_key = plan_key(usize_to_u64(entries));

        let result_hit_key = result_key(usize_to_u64(entries.saturating_sub(1)));
        let result_runtime = include_result_cache.then(|| {
            let result_limits = CassieRuntimeLimits {
                execution_result_cache_enabled: ExecutionResultCacheEnabled::enabled(),
                execution_result_cache_max_entries: entries,
                execution_result_cache_max_bytes: 64 * 1024 * 1024,
                ..CassieRuntimeLimits::default()
            };
            let runtime = Arc::new(RuntimeState::new(result_limits));
            for fingerprint in 0..entries {
                let key = result_key(usize_to_u64(fingerprint));
                runtime.execution_result_cache_store(&key, sample_query_result(fingerprint));
            }
            runtime
        });

        Self {
            plan_runtime,
            result_runtime,
            plan_hit_key,
            plan_miss_key,
            result_hit_key,
        }
    }

    #[must_use]
    pub fn plan_hit(&self) -> usize {
        let plan = self
            .plan_runtime
            .plan_cache_lookup(&self.plan_hit_key)
            .expect("benchmark plan cache should hit");
        std::hint::black_box(plan);
        1
    }

    #[must_use]
    pub fn plan_miss(&self) -> usize {
        assert!(
            self.plan_runtime
                .plan_cache_lookup(&self.plan_miss_key)
                .is_none(),
            "benchmark plan cache should miss"
        );
        1
    }

    #[must_use]
    pub fn result_hit(&self) -> usize {
        self.result_runtime
            .as_ref()
            .expect("result cache fixture should be enabled")
            .execution_result_cache_lookup(&self.result_hit_key)
            .expect("benchmark result cache should hit")
            .rows
            .len()
    }

    #[must_use]
    pub fn plan_runtime(&self) -> Arc<RuntimeState> {
        self.plan_runtime.clone()
    }

    #[must_use]
    pub fn result_runtime(&self) -> Arc<RuntimeState> {
        self.result_runtime
            .as_ref()
            .expect("result cache fixture should be enabled")
            .clone()
    }
}

/// Real in-memory inverted-index posting union over 2,048 documents.
pub struct PostingMergeFixture {
    index: InvertedIndex,
    query_tokens: Vec<String>,
    rows: usize,
}

impl PostingMergeFixture {
    #[must_use]
    pub fn new(rows: usize) -> Self {
        assert_tier2_fixture(rows);
        let mut index = InvertedIndex::default();
        for row in 0..rows {
            let tokens = if row % 2 == 0 {
                vec!["alpha".to_string(), "common".to_string()]
            } else {
                vec!["beta".to_string(), "common".to_string()]
            };
            index.index_document(&format!("doc-{row}"), &tokens);
        }
        let query_tokens = vec!["alpha".to_string(), "beta".to_string()];
        Self {
            index,
            query_tokens,
            rows,
        }
    }

    #[must_use]
    pub fn merge(&self) -> cassie::benchmark::KernelObservation {
        let candidates = self.index.candidate_documents(&self.query_tokens);
        let candidate_count = candidates.len();
        assert_eq!(candidate_count, self.rows);
        std::hint::black_box(candidates);
        cassie::benchmark::KernelObservation::new(
            fixture_count(self.rows),
            fixture_count(candidate_count),
        )
        .with_candidate_count(fixture_count(candidate_count))
    }
}

/// Exact, HNSW, and `IVFFlat` candidate-selection fixtures.
pub struct VectorCandidateFixture {
    query: Vec<f32>,
    records: Vec<NormalizedVectorRecord>,
    brute_force_candidates: Vec<(String, Vec<f32>)>,
    hnsw_options: HnswIndexOptions,
    hnsw_graph: cassie::embeddings::HnswGraphState,
    ivfflat_training: IvfFlatTrainingState,
}

impl VectorCandidateFixture {
    #[must_use]
    pub fn new(rows: usize) -> Self {
        assert_tier2_fixture(rows);
        let records = vector_records(rows);
        let brute_force_candidates = records
            .iter()
            .map(|record| (record.id.clone(), record.values.clone()))
            .collect::<Vec<_>>();
        let hnsw_options = HnswIndexOptions {
            version: 1,
            m: 16,
            ef_construction: 64,
            ef_search: 40,
        };
        let hnsw_graph = cassie::vector::hnsw::build_graph(
            records.clone(),
            &hnsw_options,
            VECTOR_DIMENSIONS,
            DistanceMetric::L2,
        );
        let ivfflat_training = ivfflat_training(&records);
        assert!(cassie::vector::hnsw::graph_fallback_reason(
            Some(&hnsw_graph),
            DistanceMetric::L2,
            VECTOR_DIMENSIONS,
            &records,
        )
        .is_none());
        assert!(cassie::vector::ivfflat::training_compatible(
            &ivfflat_training,
            VECTOR_DIMENSIONS,
            &records,
        ));

        Self {
            query: vec![0.25, 0.75, 0.5],
            records,
            brute_force_candidates,
            hnsw_options,
            hnsw_graph,
            ivfflat_training,
        }
    }

    #[must_use]
    pub fn brute_force(&self) -> cassie::benchmark::KernelObservation {
        let selected = cassie::vector::brute_force::top_k(
            &self.query,
            self.brute_force_candidates.clone(),
            20,
            cassie::vector::l2_distance,
        );
        let result_cardinality = selected.len();
        assert_eq!(result_cardinality, 20);
        std::hint::black_box(selected);
        cassie::benchmark::KernelObservation::new(
            fixture_count(self.records.len()),
            fixture_count(result_cardinality),
        )
        .with_candidate_count(fixture_count(self.records.len()))
    }

    #[must_use]
    pub fn hnsw(&self) -> cassie::benchmark::KernelObservation {
        let selected = cassie::vector::hnsw::search_graph(
            &self.hnsw_graph,
            &self.query,
            &self.hnsw_options,
            20,
        )
        .expect("benchmark HNSW search should succeed");
        let result_cardinality = selected.candidates.len();
        let candidate_count = selected.candidate_count;
        assert!(result_cardinality > 0);
        assert!(candidate_count >= result_cardinality);
        std::hint::black_box(selected);
        cassie::benchmark::KernelObservation::new(
            fixture_count(candidate_count),
            fixture_count(result_cardinality),
        )
        .with_candidate_count(fixture_count(candidate_count))
    }

    #[must_use]
    pub fn ivfflat(&self) -> cassie::benchmark::KernelObservation {
        let probes = cassie::vector::ivfflat::probe_lists(&self.query, &self.ivfflat_training);
        let count = probes.len();
        assert!(count > 0);
        std::hint::black_box(probes);
        cassie::benchmark::KernelObservation::new(fixture_count(count), fixture_count(count))
            .with_candidate_count(fixture_count(count))
    }
}

/// Real hybrid scoring fusion over a bounded candidate set.
pub struct HybridFusionFixture {
    scores: Vec<(f64, f64)>,
}

impl HybridFusionFixture {
    #[must_use]
    pub fn new(rows: usize) -> Self {
        assert_tier2_fixture(rows);
        let scores = (0..rows)
            .map(|index| {
                let search = usize_to_f64(index % 100) / 100.0;
                let vector = usize_to_f64((rows - index) % 100) / 100.0;
                (search, vector)
            })
            .collect();
        Self { scores }
    }

    #[must_use]
    pub fn fuse(&self) -> cassie::benchmark::KernelObservation {
        let total = self
            .scores
            .iter()
            .map(|(search, vector)| cassie::hybrid::hybrid_score(*search, *vector, None))
            .sum::<f64>();
        std::hint::black_box(total);
        let rows = fixture_count(self.scores.len());
        cassie::benchmark::KernelObservation::new(rows, rows).with_candidate_count(rows)
    }
}

/// Pure pgwire and JSON codec inputs with no listener or query execution.
pub struct ProtocolCodecFixture {
    pgwire_rows: Vec<cassie::benchmark::PgwireRowCodecKernel>,
    prepared_messages: cassie::benchmark::PgwireFrontendCodecKernel,
    json_rows: Vec<serde_json::Value>,
}

impl ProtocolCodecFixture {
    #[must_use]
    pub fn new(rows: usize) -> Self {
        assert_tier2_fixture(rows);
        let pgwire_rows = (0..rows)
            .map(cassie::benchmark::PgwireRowCodecKernel::sample)
            .collect();
        let prepared_messages = cassie::benchmark::PgwireFrontendCodecKernel::with_frames(rows);
        let json_rows = (0..rows)
            .map(|index| {
                json!({
                    "id": format!("doc-{index}"),
                    "title": format!("title-{}", index % 16),
                    "score": index % 100,
                })
            })
            .collect();
        Self {
            pgwire_rows,
            prepared_messages,
            json_rows,
        }
    }

    #[must_use]
    pub fn pgwire_codec(&self) -> u64 {
        let encoded = self
            .pgwire_rows
            .iter()
            .map(cassie::benchmark::PgwireRowCodecKernel::encode)
            .collect::<Vec<_>>();
        std::hint::black_box(encoded);
        fixture_count(self.pgwire_rows.len())
    }

    #[must_use]
    pub fn prepared_loop(&self) -> u64 {
        let decoded_bytes = self.prepared_messages.decode();
        std::hint::black_box(decoded_bytes);
        fixture_count(self.prepared_messages.frame_count())
    }

    #[must_use]
    pub fn json_serialization(&self) -> u64 {
        let encoded = serde_json::to_vec(&self.json_rows).expect("benchmark rows should serialize");
        std::hint::black_box(encoded);
        fixture_count(self.json_rows.len())
    }
}

/// One logical projection fixture shared by write and replay batch measurements.
pub struct ProjectionBatchFixture {
    context: BenchContext,
    fixture_identity: String,
    write_documents: Arc<[(Option<String>, serde_json::Value)]>,
    replay_state: Arc<Mutex<ReplayFixtureState>>,
}

struct ReplayFixtureState {
    batch: ProjectionReplayBatch,
    sample: usize,
}

impl ProjectionBatchFixture {
    #[must_use]
    pub fn new(runtime: &tokio::runtime::Runtime, rows: usize) -> Self {
        assert_tier2_fixture(rows);
        let context = runtime
            .block_on(replay_context("tier2-subsystem-projection", 0))
            .expect("projection benchmark context should initialize");
        let write_documents = (0..rows)
            .map(|index| {
                let document_id = format!("tier2-projection-{index}");
                (
                    Some(document_id.clone()),
                    json!({
                        "id": document_id,
                        "title": format!("title-{}", index % 16),
                        "body": "alpha beta",
                        "score": index % 100,
                        "status": "approved",
                    }),
                )
            })
            .collect::<Arc<[_]>>();
        let batch = replay_batch(&context.collection, 0, &write_documents);
        assert_eq!(batch.events.len(), rows);
        let fixture_identity = context.data_dir.display().to_string();
        Self {
            context,
            fixture_identity,
            write_documents,
            replay_state: Arc::new(Mutex::new(ReplayFixtureState { batch, sample: 0 })),
        }
    }

    #[must_use]
    pub fn write_batch(&self) -> cassie::benchmark::KernelObservation {
        let written = self
            .context
            .cassie
            .midge
            .put_documents(&self.context.collection, self.write_documents.to_vec())
            .expect("projection benchmark batch should write");
        assert_eq!(written.len(), self.write_documents.len());
        let result_cardinality = fixture_count(written.len());
        std::hint::black_box(written);
        cassie::benchmark::KernelObservation::new(
            fixture_count(self.write_documents.len()),
            result_cardinality,
        )
    }

    #[must_use]
    pub fn replay_batch(&self) -> cassie::benchmark::KernelObservation {
        let replay_state = self.replay_state.clone();
        let batch = replay_state
            .lock()
            .expect("projection replay fixture")
            .batch
            .clone();
        let report = self
            .context
            .cassie
            .replay_projection_batch(batch)
            .expect("projection benchmark batch should replay");
        assert_eq!(
            report.applied_event_count,
            fixture_count(self.write_documents.len()),
            "projection replay must apply every fixture event",
        );
        assert_eq!(
            report.skipped_duplicate_count, 0,
            "projection replay measurement must not exercise duplicate skipping"
        );
        let result_cardinality = report.applied_event_count;
        std::hint::black_box(report);
        let collection = self.context.collection.clone();
        let write_documents = self.write_documents.clone();
        let rows = write_documents.len();
        cassie::benchmark::KernelObservation::new(fixture_count(rows), result_cardinality)
            .with_after_sample(move || {
                let mut state = replay_state.lock().expect("projection replay fixture");
                state.sample = state.sample.wrapping_add(1);
                state.batch = replay_batch(&collection, state.sample, &write_documents);
            })
    }

    #[must_use]
    pub fn cassie(&self) -> Arc<cassie::app::Cassie> {
        self.context.cassie.clone()
    }

    #[must_use]
    pub fn fixture_identity(&self) -> &str {
        &self.fixture_identity
    }

    #[must_use]
    pub fn retained_fixture_rows(&self) -> usize {
        let replay_rows = self
            .replay_state
            .lock()
            .expect("replay fixture")
            .batch
            .events
            .len();
        assert_eq!(replay_rows, self.write_documents.len());
        self.write_documents.len()
    }
}

fn plan_key(sql_fingerprint: u64) -> PlanCacheKey {
    PlanCacheKey {
        sql_fingerprint,
        schema_epoch: 1,
        data_epoch: 1,
        index_feedback_epoch: 0,
        cost_model_version: 1,
        adaptive_config_hash: 0,
        parameter_shape: Vec::new(),
        mode: ExecutionMode::SimpleQuery,
        database: None,
        search_path: Vec::new(),
    }
}

fn result_key(sql_fingerprint: u64) -> ExecutionResultCacheKey {
    ExecutionResultCacheKey {
        sql_fingerprint,
        params_hash: 0,
        schema_epoch: 1,
        data_epoch: 1,
        user: "benchmark".to_string(),
        database: None,
        search_path: Vec::new(),
        mode: ExecutionMode::SimpleQuery,
    }
}

fn sample_query_result(index: usize) -> QueryResult {
    QueryResult {
        columns: Vec::new(),
        rows: vec![vec![Value::Int64(
            i64::try_from(index).expect("benchmark cache index should fit i64"),
        )]],
        command: "SELECT 1".to_string(),
    }
}

fn vector_records(rows: usize) -> Vec<NormalizedVectorRecord> {
    (0..rows)
        .map(|index| {
            let component = usize_to_f32(index) / usize_to_f32(rows);
            NormalizedVectorRecord {
                collection: "bench_documents".to_string(),
                field: "embedding".to_string(),
                id: format!("doc-{index}"),
                built_generation: 0,
                dimensions: VECTOR_DIMENSIONS,
                metric: DistanceMetric::L2,
                normalization_version: NormalizedVectorRecord::CURRENT_NORMALIZATION_VERSION,
                payload_available: true,
                magnitude: 1.0,
                values: vec![component, 1.0 - component, 0.5],
            }
        })
        .collect()
}

fn ivfflat_training(records: &[NormalizedVectorRecord]) -> IvfFlatTrainingState {
    let mut assignments = BTreeMap::new();
    let mut list_sizes = vec![0usize; IVFFLAT_LISTS];
    for (index, record) in records.iter().enumerate() {
        let list = index % IVFFLAT_LISTS;
        assignments.insert(record.id.clone(), list);
        list_sizes[list] = list_sizes[list].saturating_add(1);
    }
    let centroids = (0..IVFFLAT_LISTS)
        .map(|index| {
            let component = usize_to_f32(index) / usize_to_f32(IVFFLAT_LISTS);
            vec![component, 1.0 - component, 0.5]
        })
        .collect::<Vec<_>>();

    IvfFlatTrainingState {
        version: 1,
        source_fingerprint: cassie::vector::normalized_vector_source_fingerprint(records),
        trained: true,
        row_count: records.len(),
        lists: IVFFLAT_LISTS,
        probes: 4,
        training_seed: 1,
        centroid_ids: (0..IVFFLAT_LISTS)
            .map(|index| format!("centroid-{index}"))
            .collect(),
        centroids,
        assignments,
        list_sizes,
    }
}

fn replay_batch(
    collection: &str,
    sample: usize,
    documents: &[(Option<String>, serde_json::Value)],
) -> ProjectionReplayBatch {
    let start = sample.saturating_mul(documents.len());
    let events = documents
        .iter()
        .enumerate()
        .map(|(index, (document_id, payload))| {
            let position = start.saturating_add(index).saturating_add(1);
            let document_id = document_id
                .as_ref()
                .expect("projection fixture document should have an identity")
                .clone();
            ProjectionReplayEvent {
                event_id: format!("tier2-replay-event-{sample}-{index}"),
                checkpoint: format!("tier2-replay-checkpoint-{sample}-{index}"),
                position: Some(usize_to_u64(position)),
                document_id: document_id.clone(),
                payload: Some(payload.clone()),
            }
        })
        .collect();
    ProjectionReplayBatch {
        projection: collection.to_string(),
        source_identity: "tier2-replay-source".to_string(),
        batch_id: format!("tier2-replay-batch-{sample}"),
        lag: 0,
        events,
    }
}

fn assert_tier2_fixture(rows: usize) {
    assert!(rows > 0, "Tier 2 fixture must not be empty");
    assert!(rows <= 2_048, "Tier 2 fixture exceeds 2,048 rows");
}

fn fixture_count(rows: usize) -> u64 {
    u64::try_from(rows).expect("benchmark fixture count should fit u64")
}

fn usize_to_u64(value: usize) -> u64 {
    u64::try_from(value).expect("benchmark value should fit u64")
}

fn usize_to_f32(value: usize) -> f32 {
    f32::from(u16::try_from(value).expect("benchmark value should fit u16"))
}

fn usize_to_f64(value: usize) -> f64 {
    f64::from(u32::try_from(value).expect("benchmark value should fit u32"))
}
