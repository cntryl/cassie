use cassie_engine::catalog::Catalog;
use cassie_engine::planner::logical::{self, LogicalPlan};
use cassie_engine::planner::physical;
use cassie_engine::sql::ast::ParsedStatement;
use cassie_engine::sql::binder::{self, BoundStatement};

#[path = "subsystem_fixture.rs"]
mod fixture;
use fixture::{
    assert_tier2_fixture, bind_planning_statements, fixture_count, parse_planning_statements,
    planning_catalog,
};

pub const PLANNING_FIXTURE_INVOCATIONS_PER_SAMPLE: usize = 256;

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
}
