use std::path::PathBuf;
use std::time::Instant;

use crate::app::{Cassie, CassieError};
use crate::config::CassieRuntimeLimits;
use crate::planner::physical::PhysicalPlan;
use crate::runtime::QueryExecutionControls;

use super::QueryExplainPlan;

struct Fixture {
    cassie: Option<Cassie>,
    path: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("cassie-owned-explain-{}", uuid::Uuid::new_v4()));
        let cassie = Cassie::new_with_data_dir(&path).expect("Cassie fixture");
        Self { cassie: Some(cassie), path }
    }

    fn cassie(&self) -> &Cassie {
        self.cassie.as_ref().expect("live Cassie")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        drop(self.cassie.take());
        if self.path.exists() {
            std::fs::remove_dir_all(&self.path).expect("remove retired fixture");
        }
    }
}

fn controls(budget: usize) -> QueryExecutionControls {
    QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: budget,
            query_timeout_ms: 0,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    )
}

fn physical(cassie: &Cassie) -> std::sync::Arc<PhysicalPlan> {
    let parsed = crate::sql::parser::parse_statement("SELECT 1 AS value").expect("parsed SQL");
    let session = cassie.create_session("root", None);
    cassie.compile_physical_plan(parsed, Some(&session), None).expect("physical plan")
}

fn build_plan(
    cassie: &Cassie,
    physical: &PhysicalPlan,
    controls: &QueryExecutionControls,
) -> Result<QueryExplainPlan, CassieError> {
    super::structured_plan(cassie, physical, controls)
}

#[test]
fn should_keep_explain_plan_capacity_reserved_until_the_last_alias_drops() {
    // Arrange
    let fixture = Fixture::new();
    let physical = physical(fixture.cassie());
    let controls = controls(128 * 1024);

    // Act
    let plan = build_plan(fixture.cassie(), &physical, &controls).expect("structured plan");
    let alias = plan.clone();
    let literal = serde_json::to_value(&alias).expect("literal plan");
    drop(plan);
    drop(physical);
    drop(fixture);
    let live = controls.current_query_memory_bytes();
    drop(alias);
    let released = controls.current_query_memory_bytes();

    // Assert
    assert_eq!(literal["format_version"], 1);
    assert!(live > 0, "public EXPLAIN storage must retain its narrow reservation");
    assert_eq!(released, 0);
}

#[test]
fn should_reject_explain_construction_before_allocating_without_budget() {
    // Arrange
    let fixture = Fixture::new();
    let physical = physical(fixture.cassie());
    let controls = controls(0);

    // Act
    let plan = build_plan(fixture.cassie(), &physical, &controls);
    let rejected = matches!(plan, Err(CassieError::ResourceLimit(_)));
    drop(plan);
    drop(physical);
    drop(fixture);
    let released = controls.current_query_memory_bytes();

    // Assert
    assert!(rejected, "the plan owner must be admitted before nested construction");
    assert_eq!(released, 0);
}
