//! Public-plan fixture for option-table ownership and execution controls.
use cassie::app::Cassie;
use cassie::planner::physical::PhysicalPlan;
use std::sync::Arc;

pub fn physical(cassie: &Cassie, sql: &str) -> Arc<PhysicalPlan> {
    let parsed = cassie::sql::parse_statement(sql).expect("parse option query");
    let bound = cassie::sql::binder::bind(parsed, &cassie.catalog).expect("bind option query");
    let logical = cassie::planner::logical::plan(&bound).expect("logical plan");
    Arc::new(cassie::planner::physical::build(logical))
}
