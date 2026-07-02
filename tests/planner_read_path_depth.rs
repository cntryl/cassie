#![allow(unused_imports, dead_code)]
use cassie::catalog::{Catalog, IndexKind, IndexMeta};
use cassie::planner::{logical, physical};
use cassie::sql::ast::{Expr, QueryStatement};
use cassie::sql::{binder, parser};
use cassie::types::{DataType, FieldSchema};
use std::collections::BTreeMap;

fn register_collection_fields(catalog: &Catalog, name: &str, schema: Vec<FieldSchema>) {
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

fn register_expression_index(catalog: &Catalog, collection: &str, name: &str, expression: &Expr) {
    catalog.register_index(IndexMeta {
        collection: collection.to_string(),
        name: name.to_string(),
        field: String::new(),
        fields: Vec::new(),
        expressions: vec![serde_json::to_string(expression).unwrap()],
        include_fields: Vec::new(),
        predicate: None,
        kind: IndexKind::Scalar,
        unique: false,
        options: BTreeMap::new(),
    });
}

fn expression_from_create_index(sql: &str) -> Expr {
    let parsed = parser::parse_statement(sql).unwrap();
    let QueryStatement::CreateIndex(statement) = parsed.statement else {
        panic!("expected create index statement");
    };
    statement.expressions[0].clone()
}

fn build_plan(catalog: &Catalog, sql: &str, collection: &str) -> physical::PhysicalPlan {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let parsed = parser::parse_statement(sql).unwrap();
        let bound = binder::bind(parsed, catalog).unwrap();
        let logical = logical::plan(&bound).unwrap();
        let indexes = catalog.list_indexes(collection);
        let cardinality_stats =
            std::collections::HashMap::<String, cassie::catalog::CollectionCardinalityStats>::new();
        physical::build_with_indexes(logical, indexes.as_slice(), &cardinality_stats)
    })
}

#[test]
fn should_use_range_scan_when_mixed_order_prefix_is_equality_bound() {
    // Arrange
    let catalog = Catalog::new();
    register_collection_fields(
        &catalog,
        "planner_mixed_order_prefix",
        vec![
            FieldSchema {
                name: "tenant".to_string(),
                data_type: DataType::Text,
                nullable: true,
            },
            FieldSchema {
                name: "status".to_string(),
                data_type: DataType::Text,
                nullable: true,
            },
            FieldSchema {
                name: "created_at".to_string(),
                data_type: DataType::Int,
                nullable: true,
            },
            FieldSchema {
                name: "title".to_string(),
                data_type: DataType::Text,
                nullable: true,
            },
        ],
    );
    register_scalar_index(
        &catalog,
        "planner_mixed_order_prefix",
        "planner_mixed_order_prefix_idx",
        vec!["tenant", "status", "created_at"],
    );

    // Act
    let physical_plan = build_plan(
        &catalog,
        "SELECT title FROM planner_mixed_order_prefix \
         WHERE tenant = 'tenant-a' AND status = 'open' AND created_at >= 10 \
         ORDER BY status DESC, created_at ASC LIMIT 2",
        "planner_mixed_order_prefix",
    );

    // Assert
    assert_eq!(
        physical_plan.read.selected_index.as_deref(),
        Some("planner_mixed_order_prefix_idx")
    );
    assert_eq!(
        physical_plan.read.access_path,
        physical::ReadAccessPath::RangeScan
    );
    assert_eq!(physical_plan.read.access_path_reason, "scalar-index-range");
    assert_eq!(physical_plan.read.fallback_reason, None);
}

#[test]
fn should_use_prefix_scan_when_mixed_order_suffix_needs_final_sort() {
    // Arrange
    let catalog = Catalog::new();
    register_collection_fields(
        &catalog,
        "planner_mixed_order_fallback",
        vec![
            FieldSchema {
                name: "tenant".to_string(),
                data_type: DataType::Text,
                nullable: true,
            },
            FieldSchema {
                name: "created_at".to_string(),
                data_type: DataType::Int,
                nullable: true,
            },
            FieldSchema {
                name: "score".to_string(),
                data_type: DataType::Int,
                nullable: true,
            },
            FieldSchema {
                name: "title".to_string(),
                data_type: DataType::Text,
                nullable: true,
            },
        ],
    );
    register_scalar_index(
        &catalog,
        "planner_mixed_order_fallback",
        "planner_mixed_order_fallback_idx",
        vec!["tenant", "created_at", "score"],
    );

    // Act
    let physical_plan = build_plan(
        &catalog,
        "SELECT title FROM planner_mixed_order_fallback \
         WHERE tenant = 'tenant-a' \
         ORDER BY created_at DESC, score ASC LIMIT 2",
        "planner_mixed_order_fallback",
    );

    // Assert
    assert_eq!(
        physical_plan.read.selected_index.as_deref(),
        Some("planner_mixed_order_fallback_idx")
    );
    assert_eq!(
        physical_plan.read.access_path,
        physical::ReadAccessPath::PrefixScan
    );
    assert_eq!(physical_plan.read.access_path_reason, "scalar-index-prefix");
    assert_eq!(physical_plan.read.fallback_reason, None);
    assert_eq!(physical_plan.top_k.mode, physical::TopKMode::Heap);
    assert_eq!(physical_plan.read.early_stop, physical::EarlyStopMode::None);
}

#[test]
fn should_use_prefix_scan_when_mixed_row_id_suffix_needs_final_sort() {
    // Arrange
    let catalog = Catalog::new();
    register_collection_fields(
        &catalog,
        "planner_mixed_order_row_id",
        vec![
            FieldSchema {
                name: "tenant".to_string(),
                data_type: DataType::Text,
                nullable: true,
            },
            FieldSchema {
                name: "score".to_string(),
                data_type: DataType::Int,
                nullable: true,
            },
            FieldSchema {
                name: "title".to_string(),
                data_type: DataType::Text,
                nullable: true,
            },
        ],
    );
    register_scalar_index(
        &catalog,
        "planner_mixed_order_row_id",
        "planner_mixed_order_row_id_idx",
        vec!["tenant", "score"],
    );

    // Act
    let physical_plan = build_plan(
        &catalog,
        "SELECT title FROM planner_mixed_order_row_id \
         WHERE tenant = 'tenant-a' \
         ORDER BY score DESC, id ASC LIMIT 2",
        "planner_mixed_order_row_id",
    );

    // Assert
    assert_eq!(
        physical_plan.read.selected_index.as_deref(),
        Some("planner_mixed_order_row_id_idx")
    );
    assert_eq!(
        physical_plan.read.access_path,
        physical::ReadAccessPath::PrefixScan
    );
    assert_eq!(physical_plan.top_k.mode, physical::TopKMode::Heap);
    assert_eq!(physical_plan.read.early_stop, physical::EarlyStopMode::None);
}

#[test]
fn should_use_nonselective_prefix_scan_when_mixed_row_id_suffix_needs_final_sort() {
    // Arrange
    let catalog = Catalog::new();
    register_collection_fields(
        &catalog,
        "planner_nonselective_mixed_order_row_id",
        vec![
            FieldSchema {
                name: "score".to_string(),
                data_type: DataType::Int,
                nullable: true,
            },
            FieldSchema {
                name: "title".to_string(),
                data_type: DataType::Text,
                nullable: true,
            },
        ],
    );
    register_scalar_index(
        &catalog,
        "planner_nonselective_mixed_order_row_id",
        "planner_nonselective_mixed_order_row_id_idx",
        vec!["score"],
    );

    // Act
    let physical_plan = build_plan(
        &catalog,
        "SELECT id FROM planner_nonselective_mixed_order_row_id \
         ORDER BY score DESC, id ASC LIMIT 2",
        "planner_nonselective_mixed_order_row_id",
    );

    // Assert
    assert_eq!(
        physical_plan.read.selected_index.as_deref(),
        Some("planner_nonselective_mixed_order_row_id_idx")
    );
    assert_eq!(
        physical_plan.read.access_path,
        physical::ReadAccessPath::PrefixScan
    );
    assert_eq!(physical_plan.read.fallback_reason, None);
    assert_eq!(physical_plan.top_k.mode, physical::TopKMode::Heap);
    assert_eq!(physical_plan.read.early_stop, physical::EarlyStopMode::None);
}

#[test]
fn should_keep_collection_scan_for_noncovered_nonselective_mixed_row_id_suffix() {
    // Arrange
    let catalog = Catalog::new();
    register_collection_fields(
        &catalog,
        "planner_noncovered_nonselective_mixed_order",
        vec![
            FieldSchema {
                name: "score".to_string(),
                data_type: DataType::Int,
                nullable: true,
            },
            FieldSchema {
                name: "title".to_string(),
                data_type: DataType::Text,
                nullable: true,
            },
        ],
    );
    register_scalar_index(
        &catalog,
        "planner_noncovered_nonselective_mixed_order",
        "planner_noncovered_nonselective_mixed_order_idx",
        vec!["score"],
    );

    // Act
    let physical_plan = build_plan(
        &catalog,
        "SELECT title FROM planner_noncovered_nonselective_mixed_order \
         ORDER BY score DESC, id ASC LIMIT 2",
        "planner_noncovered_nonselective_mixed_order",
    );

    // Assert
    assert_eq!(physical_plan.read.selected_index, None);
    assert_eq!(
        physical_plan.read.access_path,
        physical::ReadAccessPath::CollectionScan
    );
    assert_eq!(physical_plan.top_k.mode, physical::TopKMode::Heap);
    assert_eq!(physical_plan.read.early_stop, physical::EarlyStopMode::None);
}

#[test]
fn should_lower_expression_equality_to_scalar_index_seek() {
    // Arrange
    let catalog = Catalog::new();
    register_collection_fields(
        &catalog,
        "planner_expression_index_seek",
        vec![
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
        ],
    );
    let expression = expression_from_create_index(
        "CREATE INDEX planner_expression_index_seek_idx \
         ON planner_expression_index_seek USING btree (lower(title))",
    );
    register_expression_index(
        &catalog,
        "planner_expression_index_seek",
        "planner_expression_index_seek_idx",
        &expression,
    );

    // Act
    let physical_plan = build_plan(
        &catalog,
        "SELECT body FROM planner_expression_index_seek WHERE lower(title) = 'alpha'",
        "planner_expression_index_seek",
    );

    // Assert
    assert_eq!(
        physical_plan.read.selected_index.as_deref(),
        Some("planner_expression_index_seek_idx")
    );
    assert_eq!(
        physical_plan.read.access_path,
        physical::ReadAccessPath::IndexSeek
    );
    assert_eq!(physical_plan.read.access_path_reason, "scalar-index-seek");
    assert_eq!(physical_plan.read.fallback_reason, None);
}

#[test]
fn should_lower_expression_range_to_scalar_index_range_scan() {
    // Arrange
    let catalog = Catalog::new();
    register_collection_fields(
        &catalog,
        "planner_expression_index_range",
        vec![
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
        ],
    );
    let expression = expression_from_create_index(
        "CREATE INDEX planner_expression_index_range_idx \
         ON planner_expression_index_range USING btree (lower(title))",
    );
    register_expression_index(
        &catalog,
        "planner_expression_index_range",
        "planner_expression_index_range_idx",
        &expression,
    );

    // Act
    let physical_plan = build_plan(
        &catalog,
        "SELECT body FROM planner_expression_index_range \
         WHERE lower(title) >= 'm' AND lower(title) < 'z'",
        "planner_expression_index_range",
    );

    // Assert
    assert_eq!(
        physical_plan.read.selected_index.as_deref(),
        Some("planner_expression_index_range_idx")
    );
    assert_eq!(
        physical_plan.read.access_path,
        physical::ReadAccessPath::RangeScan
    );
    assert_eq!(physical_plan.read.access_path_reason, "scalar-index-range");
    assert_eq!(physical_plan.read.fallback_reason, None);
}

#[test]
fn should_lower_expression_order_limit_to_ordered_bounded_scan() {
    // Arrange
    let catalog = Catalog::new();
    register_collection_fields(
        &catalog,
        "planner_expression_index_order",
        vec![
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
        ],
    );
    let expression = expression_from_create_index(
        "CREATE INDEX planner_expression_index_order_idx \
         ON planner_expression_index_order USING btree (lower(title))",
    );
    register_expression_index(
        &catalog,
        "planner_expression_index_order",
        "planner_expression_index_order_idx",
        &expression,
    );

    // Act
    let physical_plan = build_plan(
        &catalog,
        "SELECT body FROM planner_expression_index_order \
         ORDER BY lower(title) DESC LIMIT 2",
        "planner_expression_index_order",
    );

    // Assert
    assert_eq!(
        physical_plan.read.selected_index.as_deref(),
        Some("planner_expression_index_order_idx")
    );
    assert_eq!(
        physical_plan.read.access_path,
        physical::ReadAccessPath::OrderedBoundedScan
    );
    assert_eq!(
        physical_plan.read.access_path_reason,
        "scalar-index-ordered-bounded"
    );
    assert_eq!(physical_plan.read.fallback_reason, None);
}
