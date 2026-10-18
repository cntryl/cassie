//! Seeded stored-row and direct-plan fixtures for FETCH.
use crate::support_sql_fixture::{sql_fixture, SqlFixture};

pub fn fixture(label: &str) -> SqlFixture {
    sql_fixture(
        label,
        &[
            "CREATE TABLE fetch_rows (v INT)",
            "INSERT INTO fetch_rows VALUES (7),(8),(9)",
        ],
    )
}

pub fn physical(fixture: &SqlFixture) -> std::sync::Arc<cassie::planner::physical::PhysicalPlan> {
    let parsed =
        cassie::sql::parse_statement("SELECT 7 FETCH NEXT $1 ROWS ONLY").expect("parse FETCH");
    let bound = cassie::sql::binder::bind(parsed, &fixture.cassie.catalog).expect("bind");
    let logical = cassie::planner::logical::plan(&bound).expect("plan");
    std::sync::Arc::new(cassie::planner::physical::build(logical))
}
