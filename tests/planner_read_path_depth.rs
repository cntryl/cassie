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

fn register_expression_index(catalog: &Catalog, collection: &str, name: &str, expression: Expr) {
    catalog.register_index(IndexMeta {
        collection: collection.to_string(),
        name: name.to_string(),
        field: String::new(),
        fields: Vec::new(),
        expressions: vec![serde_json::to_string(&expression).unwrap()],
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
        physical::build_with_indexes(
            logical,
            catalog.list_indexes(collection),
            &Default::default(),
        )
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
        physical_plan.selected_index.as_deref(),
        Some("planner_mixed_order_prefix_idx")
    );
    assert_eq!(
        physical_plan.access_path,
        physical::ReadAccessPath::RangeScan
    );
    assert_eq!(physical_plan.access_path_reason, "scalar-index-range");
    assert_eq!(physical_plan.fallback_reason, None);
}

#[test]
fn should_report_fallback_when_mixed_order_suffix_lacks_index_proof() {
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
        physical_plan.selected_index.as_deref(),
        Some("planner_mixed_order_fallback_idx")
    );
    assert_eq!(
        physical_plan.access_path,
        physical::ReadAccessPath::CollectionScan
    );
    assert_eq!(
        physical_plan.fallback_reason.as_deref(),
        Some("index-order-proof-missing")
    );
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
        expression,
    );

    // Act
    let physical_plan = build_plan(
        &catalog,
        "SELECT body FROM planner_expression_index_seek WHERE lower(title) = 'alpha'",
        "planner_expression_index_seek",
    );

    // Assert
    assert_eq!(
        physical_plan.selected_index.as_deref(),
        Some("planner_expression_index_seek_idx")
    );
    assert_eq!(
        physical_plan.access_path,
        physical::ReadAccessPath::IndexSeek
    );
    assert_eq!(physical_plan.access_path_reason, "scalar-index-seek");
    assert_eq!(physical_plan.fallback_reason, None);
}
