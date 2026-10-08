//! Joined qualifier/name cross-product admission and denial cleanup.
use super::*;
use crate::executor::execution::{Cassie, CteContext};
use crate::runtime::QueryExecutionControls;
use crate::types::DataType;
use std::time::Instant;

fn controls(budget: usize) -> QueryExecutionControls {
    QueryExecutionControls::from_limits(
        &crate::config::CassieRuntimeLimits {
            query_memory_budget_bytes: budget,
            ..crate::config::CassieRuntimeLimits::default()
        },
        Instant::now(),
    )
}

#[test]
fn should_release_denied_joined_qualifier_name_cross_product_scope() {
    // Arrange
    let path = std::env::temp_dir().join(format!("cassie-joined-scope-{}", uuid::Uuid::new_v4()));
    let cassie = Cassie::new_with_data_dir(&path).expect("fixture");
    cassie.catalog.register_collection(
        "wide",
        (0..24)
            .map(|index| (format!("field_{index}"), DataType::Int))
            .collect(),
    );
    let leaf = QuerySource::Aliased {
        source: Box::new(QuerySource::Collection(
            crate::sql::ast::IdentifierPath::parse("wide").expect("source identity"),
        )),
        alias: "a".repeat(256),
        column_aliases: Vec::new(),
    };
    let mut right = leaf.clone();
    if let QuerySource::Aliased { alias, .. } = &mut right {
        *alias = "b".repeat(256);
    }
    let source = QuerySource::Join {
        left: Box::new(leaf.clone()),
        right: Box::new(right),
        kind: crate::sql::ast::JoinKind::Cross,
        on: crate::sql::ast::Expr::BoolLiteral(true),
    };
    let ctes = CteContext::new();
    let functions = std::collections::HashMap::new();
    let broad = controls(16 * 1024 * 1024);
    let context = ExistsResolutionContext {
        cassie: &cassie,
        session: None,
        cte_context: &ctes,
        user_functions: &functions,
        params: &[],
        controls: &broad,
        outer_row: None,
    };
    let admitted = Scope::new(&context, &source).expect("long joined qualifier scope");
    let peak = broad.peak_query_memory_bytes();
    let retained = broad.current_query_memory_bytes();
    assert!(
        retained > 24 * 512,
        "qualifier/name cross-product retains charge"
    );
    assert!(!admitted.fields.is_empty());
    drop(admitted);
    assert_eq!(broad.current_query_memory_bytes(), 0);
    let tight = controls(peak - 1);
    let context = ExistsResolutionContext {
        controls: &tight,
        ..context
    };

    // Act
    let rejected = Scope::new(&context, &source);

    // Assert
    assert!(matches!(
        rejected,
        Err(QueryError::Cassie(crate::app::CassieError::ResourceLimit(
            _
        )))
    ));
    assert_eq!(
        tight.current_query_memory_bytes(),
        0,
        "denied scope releases all private names/schema/table leases"
    );
    drop(cassie);
    std::fs::remove_dir_all(path).expect("strict cleanup");
}
