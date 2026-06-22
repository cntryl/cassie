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
    register_collection_fields(
        catalog,
        name,
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
}

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

#[test]
fn should_build_physical_operators_in_execution_order() {
    // Arrange
    let catalog = Catalog::new();
    register_test_collection(&catalog, "planner_physical");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let parsed = parser::parse_statement(
            "SELECT title FROM planner_physical WHERE title = 'alpha' ORDER BY title DESC LIMIT 2 OFFSET 1",
        )
        .unwrap();
        let bound = binder::bind(parsed, &catalog).unwrap();
        let logical = logical::plan(&bound).unwrap();
        let logical = optimizer::optimize(logical);

        // Act
        let physical_plan = physical::build(logical);

        // Assert
        assert_eq!(physical_plan.operators.len(), 6);
        assert!(matches!(
            physical_plan.operators.first(),
            Some(Operator::Scan)
        ));
        assert!(matches!(
            physical_plan.operators.get(1),
            Some(Operator::Filter)
        ));
        assert!(matches!(
            physical_plan.operators.get(2),
            Some(Operator::Sort)
        ));
        assert!(matches!(
            physical_plan.operators.get(3),
            Some(Operator::Project)
        ));
        assert!(matches!(
            physical_plan.operators.get(4),
            Some(Operator::Offset)
        ));
        assert!(matches!(
            physical_plan.operators.get(5),
            Some(Operator::Limit)
        ));
    });
}

#[test]
fn should_keep_scan_operator_for_parallel_scan_candidates() {
    // Arrange
    let catalog = Catalog::new();
    register_test_collection(&catalog, "planner_parallel_scan");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let parsed = parser::parse_statement("SELECT title FROM planner_parallel_scan")
            .expect("parse should succeed");
        let bound = binder::bind(parsed, &catalog).expect("bind should succeed");
        let logical = logical::plan(&bound).expect("logical plan");

        // Act
        let physical_plan = physical::build(logical);

        // Assert
        assert_eq!(physical_plan.operators.len(), 2);
        assert!(matches!(physical_plan.operators[0], Operator::Scan));
        assert!(matches!(physical_plan.operators[1], Operator::Project));
    });
}

#[test]
fn should_mark_literal_equality_filter_as_pushed_down() {
    // Arrange
    let catalog = Catalog::new();
    register_test_collection(&catalog, "planner_predicate_pushdown");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let parsed = parser::parse_statement(
            "SELECT title FROM planner_predicate_pushdown WHERE title = 'alpha'",
        )
        .unwrap();
        let bound = binder::bind(parsed, &catalog).unwrap();
        let logical = logical::plan(&bound).unwrap();
        let logical = optimizer::optimize(logical);

        // Act
        let physical_plan = physical::build(logical);

        // Assert
        assert!(physical_plan.predicate_pushdown);
    });
}

#[test]
fn should_mark_projected_scan_fields_for_projection_pruning() {
    // Arrange
    let catalog = Catalog::new();
    register_test_collection(&catalog, "planner_projection_pruning");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let parsed = parser::parse_statement(
            "SELECT title FROM planner_projection_pruning WHERE body = 'alpha'",
        )
        .unwrap();
        let bound = binder::bind(parsed, &catalog).unwrap();
        let logical = logical::plan(&bound).unwrap();
        let logical = optimizer::optimize(logical);

        // Act
        let physical_plan = physical::build(logical);

        // Assert
        assert_eq!(
            physical_plan.projected_scan_fields,
            vec!["title".to_string(), "body".to_string()]
        );
    });
}

#[test]
fn should_mark_scan_limit_for_limit_pushdown() {
    // Arrange
    let catalog = Catalog::new();
    register_test_collection(&catalog, "planner_limit_pushdown");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let parsed =
            parser::parse_statement("SELECT title FROM planner_limit_pushdown LIMIT 20 OFFSET 5")
                .unwrap();
        let bound = binder::bind(parsed, &catalog).unwrap();
        let logical = logical::plan(&bound).unwrap();
        let logical = optimizer::optimize(logical);

        // Act
        let physical_plan = physical::build(logical);

        // Assert
        assert_eq!(physical_plan.scan_limit, Some(25));
    });
}

#[test]
fn should_mark_order_limit_query_as_top_k() {
    // Arrange
    let catalog = Catalog::new();
    register_test_collection(&catalog, "planner_top_k");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let parsed =
            parser::parse_statement("SELECT title FROM planner_top_k ORDER BY title DESC LIMIT 5")
                .unwrap();
        let bound = binder::bind(parsed, &catalog).unwrap();
        let logical = logical::plan(&bound).unwrap();
        let logical = optimizer::optimize(logical);

        // Act
        let physical_plan = physical::build(logical);

        // Assert
        assert!(physical_plan.top_k);
        assert_eq!(physical_plan.top_k_limit, Some(5));
        assert_eq!(physical_plan.top_k_mode, physical::TopKMode::Heap);
        assert_eq!(physical_plan.early_stop, physical::EarlyStopMode::None);
    });
}

#[test]
fn should_plan_join_source_with_physical_join_operator() {
    // Arrange
    let catalog = Catalog::new();
    register_test_collection(&catalog, "planner_join_left");
    register_test_collection(&catalog, "planner_join_right");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let parsed = parser::parse_statement(
            "SELECT planner_join_left.title FROM planner_join_left JOIN planner_join_right ON planner_join_left.title = planner_join_right.title",
        )
        .unwrap();
        let bound = binder::bind(parsed, &catalog).unwrap();
        let logical = logical::plan(&bound).unwrap();

        // Act
        let physical_plan = physical::build(logical);

        // Assert
        assert_eq!(physical_plan.collection, "join");
        assert!(matches!(
            physical_plan.logical.source,
            QuerySource::Join {
                kind: JoinKind::Inner,
                ..
            }
        ));
        assert!(matches!(
            physical_plan.operators.get(1),
            Some(Operator::Join)
        ));
        assert_eq!(physical_plan.join_strategy.as_deref(), Some("hash"));
    });
}

#[test]
fn should_mark_row_id_filter_as_point_lookup_access_path() {
    // Arrange
    let catalog = Catalog::new();
    register_test_collection(&catalog, "planner_point_lookup");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let parsed = parser::parse_statement(
            "SELECT id, title FROM planner_point_lookup WHERE id = 'alpha'",
        )
        .unwrap();
        let bound = binder::bind(parsed, &catalog).unwrap();
        let logical = logical::plan(&bound).unwrap();
        let logical = optimizer::optimize(logical);

        // Act
        let physical_plan = physical::build(logical);

        // Assert
        assert_eq!(
            physical_plan.access_path,
            physical::ReadAccessPath::PointLookup
        );
        assert_eq!(physical_plan.access_path_reason, "point-lookup-id");
        assert_eq!(physical_plan.fallback_reason, None);
        assert_eq!(
            physical_plan.pagination_strategy,
            physical::PaginationStrategy::None
        );
        assert_eq!(physical_plan.top_k_mode, physical::TopKMode::None);
        assert_eq!(
            physical_plan.early_stop,
            physical::EarlyStopMode::PointLookup
        );
        assert_eq!(
            physical_plan.projection_shape,
            physical::ProjectionShape::MaterializedProjection
        );
    });
}

#[test]
fn should_mark_row_id_order_limit_as_storage_top_k() {
    // Arrange
    let catalog = Catalog::new();
    register_test_collection(&catalog, "planner_row_id_top_k");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let parsed = parser::parse_statement(
            "SELECT id, title FROM planner_row_id_top_k ORDER BY id ASC LIMIT 5",
        )
        .unwrap();
        let bound = binder::bind(parsed, &catalog).unwrap();
        let logical = logical::plan(&bound).unwrap();
        let logical = optimizer::optimize(logical);

        // Act
        let physical_plan = physical::build(logical);

        // Assert
        assert_eq!(
            physical_plan.access_path,
            physical::ReadAccessPath::CollectionScan
        );
        assert_eq!(physical_plan.access_path_reason, "row-key-top-k");
        assert_eq!(
            physical_plan.pagination_strategy,
            physical::PaginationStrategy::Limit
        );
        assert_eq!(physical_plan.top_k_mode, physical::TopKMode::Storage);
        assert_eq!(
            physical_plan.early_stop,
            physical::EarlyStopMode::StorageTopK
        );
    });
}

#[test]
fn should_mark_row_id_cursor_as_keyset_pagination() {
    // Arrange
    let catalog = Catalog::new();
    register_test_collection(&catalog, "planner_row_id_keyset");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let parsed = parser::parse_statement(
            "SELECT id, title FROM planner_row_id_keyset WHERE id > 'cursor' ORDER BY id ASC LIMIT 5",
        )
        .unwrap();
        let bound = binder::bind(parsed, &catalog).unwrap();
        let logical = logical::plan(&bound).unwrap();
        let logical = optimizer::optimize(logical);

        // Act
        let physical_plan = physical::build(logical);

        // Assert
        assert_eq!(physical_plan.access_path_reason, "row-key-keyset");
        assert_eq!(
            physical_plan.pagination_strategy,
            physical::PaginationStrategy::Keyset
        );
        assert_eq!(physical_plan.top_k_mode, physical::TopKMode::None);
        assert_eq!(physical_plan.early_stop, physical::EarlyStopMode::Keyset);
    });
}

#[test]
fn should_mark_row_id_offset_page_as_degraded_offset() {
    // Arrange
    let catalog = Catalog::new();
    register_test_collection(&catalog, "planner_row_id_offset_page");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let parsed = parser::parse_statement(
            "SELECT id, title FROM planner_row_id_offset_page ORDER BY id ASC LIMIT 5 OFFSET 2",
        )
        .unwrap();
        let bound = binder::bind(parsed, &catalog).unwrap();
        let logical = logical::plan(&bound).unwrap();
        let logical = optimizer::optimize(logical);

        // Act
        let physical_plan = physical::build(logical);

        // Assert
        assert_eq!(physical_plan.access_path_reason, "row-key-ordered-page");
        assert_eq!(
            physical_plan.fallback_reason,
            Some("offset-degraded".to_string())
        );
        assert_eq!(
            physical_plan.pagination_strategy,
            physical::PaginationStrategy::DegradedOffset
        );
        assert_eq!(physical_plan.early_stop, physical::EarlyStopMode::None);
    });
}

#[test]
fn should_fallback_from_point_lookup_to_collection_scan_with_offset() {
    // Arrange
    let catalog = Catalog::new();
    register_test_collection(&catalog, "planner_point_lookup_offset");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let parsed = parser::parse_statement(
            "SELECT id, title FROM planner_point_lookup_offset WHERE id = 'alpha' OFFSET 1",
        )
        .unwrap();
        let bound = binder::bind(parsed, &catalog).unwrap();
        let logical = logical::plan(&bound).unwrap();
        let logical = optimizer::optimize(logical);

        // Act
        let physical_plan = physical::build(logical);

        // Assert
        assert_eq!(
            physical_plan.access_path,
            physical::ReadAccessPath::CollectionScan
        );
        assert_eq!(
            physical_plan.fallback_reason,
            Some("offset-degraded".to_string())
        );
        assert_eq!(
            physical_plan.pagination_strategy,
            physical::PaginationStrategy::Offset
        );
    });
}

#[test]
fn should_mark_composite_equality_as_prefix_scan() {
    // Arrange
    let catalog = Catalog::new();
    register_collection_fields(
        &catalog,
        "planner_prefix_scan",
        vec![
            FieldSchema {
                name: "tenant_id".to_string(),
                data_type: DataType::Text,
                nullable: true,
            },
            FieldSchema {
                name: "status".to_string(),
                data_type: DataType::Text,
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
        "planner_prefix_scan",
        "planner_prefix_scan_tenant_status_idx",
        vec!["tenant_id", "status"],
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let parsed = parser::parse_statement(
            "SELECT title FROM planner_prefix_scan WHERE tenant_id = 'tenant-a' AND status = 'open'",
        )
        .unwrap();
        let bound = binder::bind(parsed, &catalog).unwrap();
        let logical = logical::plan(&bound).unwrap();

        // Act
        let physical_plan = physical::build_with_indexes(
            logical,
            catalog.list_indexes("planner_prefix_scan"),
            &Default::default(),
        );

        // Assert
        assert_eq!(
            physical_plan.selected_index.as_deref(),
            Some("planner_prefix_scan_tenant_status_idx")
        );
        assert_eq!(physical_plan.access_path, physical::ReadAccessPath::PrefixScan);
        assert_eq!(physical_plan.access_path_reason, "scalar-index-prefix");
    });
}

#[test]
fn should_mark_range_filter_as_range_scan() {
    // Arrange
    let catalog = Catalog::new();
    register_collection_fields(
        &catalog,
        "planner_range_scan",
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
    register_scalar_index(
        &catalog,
        "planner_range_scan",
        "planner_range_scan_title_idx",
        vec!["title"],
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let parsed = parser::parse_statement(
            "SELECT title FROM planner_range_scan WHERE title >= 'alpha' AND title < 'omega'",
        )
        .unwrap();
        let bound = binder::bind(parsed, &catalog).unwrap();
        let logical = logical::plan(&bound).unwrap();

        // Act
        let physical_plan = physical::build_with_indexes(
            logical,
            catalog.list_indexes("planner_range_scan"),
            &Default::default(),
        );

        // Assert
        assert_eq!(
            physical_plan.selected_index.as_deref(),
            Some("planner_range_scan_title_idx")
        );
        assert_eq!(
            physical_plan.access_path,
            physical::ReadAccessPath::RangeScan
        );
        assert_eq!(physical_plan.access_path_reason, "scalar-index-range");
    });
}

#[test]
fn should_mark_order_limit_as_ordered_bounded_scan_when_index_matches() {
    // Arrange
    let catalog = Catalog::new();
    register_collection_fields(
        &catalog,
        "planner_ordered_bounded",
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
    register_scalar_index(
        &catalog,
        "planner_ordered_bounded",
        "planner_ordered_bounded_title_idx",
        vec!["title"],
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let parsed = parser::parse_statement(
            "SELECT title FROM planner_ordered_bounded ORDER BY title ASC LIMIT 3",
        )
        .unwrap();
        let bound = binder::bind(parsed, &catalog).unwrap();
        let logical = logical::plan(&bound).unwrap();

        // Act
        let physical_plan = physical::build_with_indexes(
            logical,
            catalog.list_indexes("planner_ordered_bounded"),
            &Default::default(),
        );

        // Assert
        assert_eq!(
            physical_plan.selected_index.as_deref(),
            Some("planner_ordered_bounded_title_idx")
        );
        assert_eq!(
            physical_plan.access_path,
            physical::ReadAccessPath::OrderedBoundedScan
        );
        assert_eq!(physical_plan.top_k_mode, physical::TopKMode::Storage);
    });
}

#[test]
fn should_report_fallback_when_secondary_ordering_proof_is_missing() {
    // Arrange
    let catalog = Catalog::new();
    register_collection_fields(
        &catalog,
        "planner_ordering_fallback",
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
    register_scalar_index(
        &catalog,
        "planner_ordering_fallback",
        "planner_ordering_fallback_title_idx",
        vec!["title"],
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let parsed = parser::parse_statement(
            "SELECT body FROM planner_ordering_fallback WHERE title = 'alpha' ORDER BY body ASC LIMIT 1",
        )
        .unwrap();
        let bound = binder::bind(parsed, &catalog).unwrap();
        let logical = logical::plan(&bound).unwrap();

        // Act
        let physical_plan = physical::build_with_indexes(
            logical,
            catalog.list_indexes("planner_ordering_fallback"),
            &Default::default(),
        );

        // Assert
        assert_eq!(
            physical_plan.selected_index.as_deref(),
            Some("planner_ordering_fallback_title_idx")
        );
        assert_eq!(physical_plan.access_path, physical::ReadAccessPath::CollectionScan);
        assert_eq!(
            physical_plan.fallback_reason.as_deref(),
            Some("index-order-proof-missing")
        );
    });
}

#[test]
fn should_mark_exists_predicate_as_semi_join() {
    // Arrange
    let catalog = Catalog::new();
    register_test_collection(&catalog, "planner_semi_outer");
    register_test_collection(&catalog, "planner_semi_inner");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let parsed = parser::parse_statement(
            "SELECT title FROM planner_semi_outer WHERE EXISTS (SELECT title FROM planner_semi_inner)",
        )
        .unwrap();
        let bound = binder::bind(parsed, &catalog).unwrap();
        let logical = logical::plan(&bound).unwrap();
        let logical = optimizer::optimize(logical);

        // Act
        let physical_plan = physical::build(logical);

        // Assert
        assert_eq!(physical_plan.join_strategy.as_deref(), Some("semi"));
        assert_eq!(physical_plan.early_stop, physical::EarlyStopMode::Exists);
    });
}

#[test]
fn should_mark_not_exists_predicate_as_anti_join() {
    // Arrange
    let catalog = Catalog::new();
    register_test_collection(&catalog, "planner_anti_outer");
    register_test_collection(&catalog, "planner_anti_inner");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let parsed = parser::parse_statement(
            "SELECT title FROM planner_anti_outer WHERE NOT EXISTS (SELECT title FROM planner_anti_inner)",
        )
        .unwrap();
        let bound = binder::bind(parsed, &catalog).unwrap();
        let logical = logical::plan(&bound).unwrap();
        let logical = optimizer::optimize(logical);

        // Act
        let physical_plan = physical::build(logical);

        // Assert
        assert_eq!(physical_plan.join_strategy.as_deref(), Some("anti"));
        assert_eq!(physical_plan.early_stop, physical::EarlyStopMode::Exists);
    });
}

#[test]
fn should_build_physical_operators_for_hybrid_search() {
    // Arrange
    let catalog = Catalog::new();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        catalog
            .register_collection(
                "planner_hybrid_physical",
                vec![
                    ("body".to_string(), DataType::Text),
                    ("embedding".to_string(), DataType::Vector(2)),
                ],
            );
        let parsed = parser::parse_statement(
            "SELECT id, hybrid_score(search_score(body, 'red'), vector_score(embedding, '[1,0]')) AS score FROM planner_hybrid_physical ORDER BY score DESC LIMIT 1",
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
            .any(|operator| matches!(operator, Operator::FullTextSearch)));
        assert!(physical_plan
            .operators
            .iter()
            .any(|operator| matches!(operator, Operator::VectorSearch)));
    });
}
