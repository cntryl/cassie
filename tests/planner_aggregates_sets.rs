#![allow(unused_imports, dead_code)]
use cassie::app::CassieError;
use cassie::catalog::{Catalog, IndexKind, IndexMeta};
use cassie::planner::{logical, optimizer, physical, physical::Operator};
use cassie::sql::ast::{
    BinaryOp, Expr, InsertSource, JoinKind, ParsedStatement, QuerySource, QueryStatement,
    SelectItem, SelectStatement, SortDirection,
};
use cassie::sql::binder::BoundStatement;
use cassie::sql::{binder, parser};
use cassie::types::{DataType, FieldSchema};
use std::collections::BTreeMap;

fn register_test_collection(catalog: &Catalog, name: &str) {
    let schema = vec![
        FieldSchema {
            name: "title".to_string(),
            data_type: DataType::Text,
            nullable: true,
        },
        FieldSchema {
            name: "body".to_string(),
            data_type: DataType::Text,
            nullable: true,
        },
    ];

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        catalog.register_collection(
            name,
            schema
                .into_iter()
                .map(|field| (field.name, field.data_type))
                .collect(),
        );
    });
}

fn register_scalar_index(catalog: &Catalog, collection: &str, name: &str, fields: Vec<&str>) {
    let fields = fields
        .into_iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    catalog.register_index(IndexMeta {
        collection: collection.to_string(),
        name: name.to_string(),
        field: fields.first().cloned().unwrap_or_default(),
        fields,
        expressions: Vec::new(),
        include_fields: Vec::new(),
        predicate: None,
        kind: IndexKind::Scalar,
        unique: false,
        options: BTreeMap::new(),
    });
}

#[test]
fn should_plan_grouped_distinct_select_controls() {
    // Arrange
    let catalog = Catalog::new();
    register_test_collection(&catalog, "planner_grouped");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let parsed = parser::parse_statement(
            "SELECT DISTINCT title, COUNT(*) AS total FROM planner_grouped GROUP BY title HAVING COUNT(*) > 1",
        )
        .unwrap();
        let bound = binder::bind(parsed, &catalog).unwrap();

        // Act
        let logical = logical::plan(&bound).unwrap();

        // Assert
        assert!(logical.distinct);
        assert_eq!(logical.group_by.len(), 1);
        assert!(logical.having.is_some());
    });
}

#[test]
fn should_keep_set_query_result_controls_in_logical_plan() {
    // Arrange
    let catalog = Catalog::new();
    register_test_collection(&catalog, "planner_set_result_left");
    register_test_collection(&catalog, "planner_set_result_right");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let parsed = parser::parse_statement(
            "SELECT title FROM planner_set_result_left UNION ALL SELECT title FROM planner_set_result_right ORDER BY title DESC LIMIT 2 OFFSET 1",
        )
        .unwrap();
        let bound = binder::bind(parsed, &catalog).unwrap();

        // Act
        let logical = logical::plan(&bound).unwrap();

        // Assert
        assert!(logical.set.is_some());
        assert_eq!(logical.order.len(), 1);
        assert!(matches!(logical.order[0].direction, SortDirection::Desc));
        assert_eq!(logical.limit, Some(2));
        assert_eq!(logical.offset, Some(1));
    });
}

#[test]
fn should_build_physical_operators_for_aggregate_distinct_set() {
    // Arrange
    let catalog = Catalog::new();
    register_test_collection(&catalog, "planner_set_left");
    register_test_collection(&catalog, "planner_set_right");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let parsed = parser::parse_statement(
            "SELECT DISTINCT title, COUNT(*) AS total FROM planner_set_left GROUP BY title UNION SELECT title, COUNT(*) AS total FROM planner_set_right GROUP BY title",
        )
        .unwrap();
        let bound = binder::bind(parsed, &catalog).unwrap();
        let logical = logical::plan(&bound).unwrap();

        // Act
        let physical_plan = physical::build(logical);

        // Assert
        assert!(physical_plan
            .operators
            .iter()
            .any(|operator| matches!(operator, Operator::Aggregate)));
        assert!(physical_plan
            .operators
            .iter()
            .any(|operator| matches!(operator, Operator::Distinct)));
        assert!(physical_plan
            .operators
            .iter()
            .any(|operator| matches!(operator, Operator::SetOperation)));
    });
}

#[test]
fn should_mark_simple_aggregate_as_parallel_candidate() {
    // Arrange
    let catalog = Catalog::new();
    register_test_collection(&catalog, "planner_parallel_aggregate");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let parsed = parser::parse_statement(
            "SELECT title, COUNT(*) AS total FROM planner_parallel_aggregate GROUP BY title",
        )
        .unwrap();
        let bound = binder::bind(parsed, &catalog).unwrap();
        let logical = logical::plan(&bound).unwrap();

        // Act
        let physical_plan = physical::build(logical);

        // Assert
        assert!(physical_plan
            .operators
            .iter()
            .any(|operator| matches!(operator, Operator::Aggregate)));
        assert!(physical_plan.parallel_aggregate_candidate);
    });
}

#[test]
fn should_keep_distinct_aggregate_on_serial_candidate_path() {
    // Arrange
    let catalog = Catalog::new();
    register_test_collection(&catalog, "planner_serial_distinct_aggregate");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let parsed = parser::parse_statement(
            "SELECT DISTINCT title, COUNT(*) AS total FROM planner_serial_distinct_aggregate GROUP BY title",
        )
        .unwrap();
        let bound = binder::bind(parsed, &catalog).unwrap();
        let logical = logical::plan(&bound).unwrap();

        // Act
        let physical_plan = physical::build(logical);

        // Assert
        assert!(physical_plan
            .operators
            .iter()
            .any(|operator| matches!(operator, Operator::Aggregate)));
        assert!(!physical_plan.parallel_aggregate_candidate);
    });
}
