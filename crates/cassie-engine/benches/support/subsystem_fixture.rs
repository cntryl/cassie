use cassie_engine::catalog::Catalog;
use cassie_engine::sql::ast::ParsedStatement;
use cassie_engine::sql::binder::{self, BoundStatement};
use cassie_engine::types::DataType;

const PLANNING_SQL: &str =
    "SELECT id, title FROM bench_documents WHERE score >= $1 ORDER BY id LIMIT 20";

pub(super) fn planning_catalog() -> Catalog {
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

pub(super) fn parse_planning_statements(statements: usize) -> Vec<ParsedStatement> {
    (0..statements)
        .map(|_| cassie_engine::sql::parse_statement(PLANNING_SQL).expect("benchmark SQL parses"))
        .collect()
}

pub(super) fn bind_planning_statements(statements: usize) -> Vec<BoundStatement> {
    let catalog = planning_catalog();
    parse_planning_statements(statements)
        .into_iter()
        .map(|statement| binder::bind(statement, &catalog).expect("benchmark SQL binds"))
        .collect()
}

pub(super) fn assert_tier2_fixture(rows: usize) {
    assert!(rows > 0, "Tier 2 fixture must not be empty");
    assert!(rows <= 2_048, "Tier 2 fixture exceeds 2,048 rows");
}

pub(super) fn fixture_count(rows: usize) -> u64 {
    u64::try_from(rows).expect("fixture count should fit u64")
}
