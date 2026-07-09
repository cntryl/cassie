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
fn should_fallback_to_conservative_cardinality_estimates_when_stats_missing() {
    // Arrange
    let catalog = Catalog::new();
    register_test_collection(&catalog, "planner_cardinality_fallback");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let parsed = parser::parse_statement(
            "SELECT title FROM planner_cardinality_fallback WHERE title = 'alpha'",
        )
        .unwrap();
        let bound = binder::bind(parsed, &catalog).unwrap();
        let logical = logical::plan(&bound).unwrap();
        let logical = optimizer::optimize(logical);
        let cardinality_stats = std::collections::HashMap::new();

        // Act
        let physical_plan =
            physical::build_with_indexes(logical, bound.indexes.as_slice(), &cardinality_stats);

        // Assert
        assert_eq!(physical_plan.estimates.scan_rows, 1_000);
        assert_eq!(physical_plan.estimates.index_rows, 1_000);
    });
}

#[test]
fn should_use_hydrated_cardinality_estimates_for_index_plans() {
    // Arrange
    let catalog = Catalog::new();
    register_test_collection(&catalog, "planner_cardinality_hydrated");
    catalog.hydrate_cardinality_stats(
        "planner_cardinality_hydrated",
        cassie::catalog::CollectionCardinalityStats {
            row_count: 42,
            hydrated: true,
            indexes: std::collections::BTreeMap::from([(
                "scalar:planner_cardinality_hydrated_title_idx".to_string(),
                cassie::catalog::IndexCardinalityStats { cardinality: 7 },
            )]),
            fields: BTreeMap::default(),
        },
    );
    catalog.register_index(cassie::catalog::IndexMeta {
        collection: "planner_cardinality_hydrated".to_string(),
        name: "planner_cardinality_hydrated_title_idx".to_string(),
        field: "title".to_string(),
        fields: vec!["title".to_string()],
        expressions: Vec::new(),
        include_fields: Vec::new(),
        predicate: None,
        kind: cassie::catalog::IndexKind::Scalar,
        unique: false,
        options: BTreeMap::default(),
    });
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let parsed = parser::parse_statement(
            "SELECT body FROM planner_cardinality_hydrated WHERE title = 'alpha'",
        )
        .unwrap();
        let bound = binder::bind(parsed, &catalog).unwrap();
        let logical = logical::plan(&bound).unwrap();
        let logical = optimizer::optimize(logical);
        let cardinality_stats = catalog.cardinality_snapshot();

        // Act
        let physical_plan =
            physical::build_with_indexes(logical, bound.indexes.as_slice(), &cardinality_stats);

        // Assert
        assert_eq!(physical_plan.estimates.scan_rows, 42);
        assert_eq!(physical_plan.estimates.index_rows, 7);
    });
}

#[test]
fn should_use_advanced_field_stats_for_selective_filter_estimates() {
    // Arrange
    let catalog = Catalog::new();
    register_test_collection(&catalog, "planner_cardinality_advanced");
    register_scalar_index(
        &catalog,
        "planner_cardinality_advanced",
        "planner_cardinality_advanced_title_idx",
        vec!["title"],
    );
    catalog.hydrate_cardinality_stats(
        "planner_cardinality_advanced",
        cassie::catalog::CollectionCardinalityStats {
            row_count: 100,
            hydrated: true,
            indexes: std::collections::BTreeMap::from([(
                "scalar:planner_cardinality_advanced_title_idx".to_string(),
                cassie::catalog::IndexCardinalityStats { cardinality: 80 },
            )]),
            fields: std::collections::BTreeMap::from([(
                "title".to_string(),
                cassie::catalog::FieldCardinalityStats {
                    non_null_count: 100,
                    distinct_count: 3,
                    sample_count: 100,
                    confidence: 100,
                    histogram_buckets: vec![cassie::catalog::FieldHistogramBucket {
                        lower: "\"alpha\"".to_string(),
                        upper: "\"gamma\"".to_string(),
                        count: 100,
                    }],
                    heavy_hitters: vec![cassie::catalog::FieldHeavyHitter {
                        value: "\"alpha\"".to_string(),
                        count: 5,
                    }],
                    ..Default::default()
                },
            )]),
        },
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let parsed = parser::parse_statement(
            "SELECT body FROM planner_cardinality_advanced WHERE title = 'alpha'",
        )
        .unwrap();
        let bound = binder::bind(parsed, &catalog).unwrap();
        let logical = logical::plan(&bound).unwrap();
        let logical = optimizer::optimize(logical);
        let cardinality_stats = catalog.cardinality_snapshot();

        // Act
        let physical_plan =
            physical::build_with_indexes(logical, bound.indexes.as_slice(), &cardinality_stats);

        // Assert
        assert_eq!(physical_plan.estimates.scan_rows, 100);
        assert_eq!(physical_plan.estimates.index_rows, 5);
        assert_eq!(physical_plan.estimates.cost_source, "advanced_stats");
    });
}

#[test]
fn should_choose_lower_cost_competing_scalar_index_from_stats() {
    // Arrange
    let catalog = Catalog::new();
    register_test_collection(&catalog, "planner_competing_indexes");
    register_scalar_index(
        &catalog,
        "planner_competing_indexes",
        "planner_competing_title_idx",
        vec!["title"],
    );
    register_scalar_index(
        &catalog,
        "planner_competing_indexes",
        "planner_competing_body_idx",
        vec!["body"],
    );
    catalog.hydrate_cardinality_stats(
        "planner_competing_indexes",
        cassie::catalog::CollectionCardinalityStats {
            row_count: 100,
            hydrated: true,
            indexes: std::collections::BTreeMap::from([
                (
                    "scalar:planner_competing_title_idx".to_string(),
                    cassie::catalog::IndexCardinalityStats { cardinality: 80 },
                ),
                (
                    "scalar:planner_competing_body_idx".to_string(),
                    cassie::catalog::IndexCardinalityStats { cardinality: 5 },
                ),
            ]),
            fields: BTreeMap::default(),
        },
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let parsed = parser::parse_statement(
            "SELECT title FROM planner_competing_indexes WHERE title = 'alpha' AND body = 'one'",
        )
        .unwrap();
        let bound = binder::bind(parsed, &catalog).unwrap();
        let logical = logical::plan(&bound).unwrap();
        let logical = optimizer::optimize(logical);
        let cardinality_stats = catalog.cardinality_snapshot();

        // Act
        let physical_plan =
            physical::build_with_indexes(logical, bound.indexes.as_slice(), &cardinality_stats);

        // Assert
        assert_eq!(
            physical_plan.read.selected_index.as_deref(),
            Some("planner_competing_body_idx")
        );
        assert_eq!(physical_plan.estimates.index_rows, 5);
    });
}
