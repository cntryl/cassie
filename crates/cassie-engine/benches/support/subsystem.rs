use cassie_engine::benchmark::{KernelObservation, SubsystemExecutorKernel};
use cassie_engine::catalog::Catalog;
use cassie_engine::planner::logical::{self, LogicalPlan};
use cassie_engine::planner::physical::{self, PhysicalPlan};
use cassie_engine::sql::ast::ParsedStatement;
use cassie_engine::sql::binder::{self, BoundStatement};
use cassie_engine::types::DataType;

#[path = "subsystem_cache.rs"]
mod cache;
pub use cache::CacheFixture;

const PLANNING_SQL: &str =
    "SELECT id, title FROM bench_documents WHERE score >= $1 ORDER BY id LIMIT 20";

pub const PLANNING_FIXTURE_INVOCATIONS_PER_SAMPLE: usize = 256;
pub const PARAMETER_BINDING_INVOCATIONS_PER_SAMPLE: usize = 8_192;
pub const EXECUTOR_FIXTURE_INVOCATIONS_PER_SAMPLE: usize = 8;
pub const CACHE_HIT_LOOKUPS_PER_SAMPLE: usize = 8_192;
pub const HYBRID_FUSION_INVOCATIONS_PER_SAMPLE: usize = 256;
pub const PLAN_CACHE_MISS_LOOKUPS_PER_SAMPLE: usize = 8_192;

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
            .map(|statement| binder::bind(statement, &self.catalog).expect("benchmark SQL binds"))
            .collect::<Vec<_>>();
        std::hint::black_box(bound);
        fixture_count(self.parsed.len())
    }
}

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
            .map(|statement| logical::plan(statement).expect("benchmark SQL plans"))
            .collect::<Vec<_>>();
        std::hint::black_box(plans);
        fixture_count(self.bound.len())
    }
}

pub struct PhysicalPlanningFixture {
    logical: Vec<LogicalPlan>,
}

impl PhysicalPlanningFixture {
    #[must_use]
    pub fn new(statements: usize) -> Self {
        assert_tier2_fixture(statements);
        let logical = bind_planning_statements(statements)
            .iter()
            .map(|statement| logical::plan(statement).expect("benchmark SQL plans"))
            .collect();
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

pub struct ParameterBindingFixture {
    kernel: cassie_engine::benchmark::PgwireParameterBindingKernel,
}

impl ParameterBindingFixture {
    #[must_use]
    pub fn new(parameters: usize) -> Self {
        assert_tier2_fixture(parameters);
        Self {
            kernel: cassie_engine::benchmark::PgwireParameterBindingKernel::with_parameters(
                parameters,
            ),
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
    pub fn fuse_batch(&self, fixture_invocations: usize) -> KernelObservation {
        assert!(
            fixture_invocations > 0,
            "hybrid-fusion batch must not be empty"
        );
        let completed = (0..fixture_invocations).fold(0_u64, |total, _| {
            total
                .checked_add(self.fuse_candidate_count())
                .expect("hybrid-fusion candidate count fits u64")
        });
        KernelObservation::new(completed, completed).with_candidate_count(completed)
    }

    fn fuse_candidate_count(&self) -> u64 {
        let total = self
            .scores
            .iter()
            .map(|(search, vector)| cassie_engine::hybrid::hybrid_score(*search, *vector, None))
            .sum::<f64>();
        std::hint::black_box(total);
        fixture_count(self.scores.len())
    }
}

pub fn executor_batch(
    fixture: &SubsystemExecutorKernel,
    invocations: usize,
    mut operation: impl FnMut(&SubsystemExecutorKernel) -> KernelObservation,
) -> KernelObservation {
    assert!(invocations > 0, "executor batch must not be empty");
    let mut completed = 0_u64;
    let mut cardinality = 0_u64;
    let mut candidates = 0_u64;
    let mut saw_candidates = false;
    for _ in 0..invocations {
        let observation = operation(fixture);
        completed = completed
            .checked_add(observation.completed_operations())
            .expect("executor batch operation count fits u64");
        cardinality = cardinality
            .checked_add(observation.result_cardinality())
            .expect("executor batch result count fits u64");
        if let Some(count) = observation.candidate_count() {
            candidates = candidates
                .checked_add(count)
                .expect("executor batch candidate count fits u64");
            saw_candidates = true;
        }
        observation.finish_sample();
    }
    let batch = KernelObservation::new(completed, cardinality);
    if saw_candidates {
        batch.with_candidate_count(candidates)
    } else {
        batch
    }
}

fn planning_catalog() -> Catalog {
    let catalog = Catalog::new();
    catalog.register_collection(
        "bench_documents",
        vec![
            ("id".to_owned(), DataType::Text),
            ("title".to_owned(), DataType::Text),
            ("score".to_owned(), DataType::Int),
        ],
    );
    catalog
}

fn parse_planning_statements(statements: usize) -> Vec<ParsedStatement> {
    (0..statements)
        .map(|_| cassie_engine::sql::parse_statement(PLANNING_SQL).expect("benchmark SQL parses"))
        .collect()
}

fn bind_planning_statements(statements: usize) -> Vec<BoundStatement> {
    let catalog = planning_catalog();
    parse_planning_statements(statements)
        .into_iter()
        .map(|statement| binder::bind(statement, &catalog).expect("benchmark SQL binds"))
        .collect()
}

fn assert_tier2_fixture(rows: usize) {
    assert!(rows > 0, "Tier 2 fixture must not be empty");
    assert!(rows <= 2_048, "Tier 2 fixture exceeds 2,048 rows");
}

fn fixture_count(rows: usize) -> u64 {
    u64::try_from(rows).expect("fixture count should fit u64")
}

fn usize_to_f64(value: usize) -> f64 {
    f64::from(u32::try_from(value).expect("benchmark value should fit u32"))
}
