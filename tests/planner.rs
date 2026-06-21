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
        include_fields: Vec::new(),
        kind: IndexKind::Scalar,
        unique: false,
        options: BTreeMap::new(),
    });
}

#[test]
fn should_plan_select_collection_projection_filter_limit_offset() {
    // Arrange
    let catalog = Catalog::new();
    register_test_collection(&catalog, "planner_projection");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let parsed = parser::parse_statement(
            "SELECT title, body FROM planner_projection WHERE title = 'alpha' ORDER BY title DESC LIMIT 2 OFFSET 1",
        )
        .unwrap();
        let bound = binder::bind(parsed, &catalog).unwrap();
        let plan = logical::plan(&bound).unwrap();

        // Act
        // assertions are direct on the logical plan shape

        // Assert
        assert_eq!(plan.collection, "planner_projection");
        assert_eq!(plan.projection.len(), 2);
        assert!(matches!(
            &plan.projection[0],
            SelectItem::Column { name, alias } if name == "title" && alias.is_none()
        ));
        assert!(matches!(
            &plan.projection[1],
            SelectItem::Column { name, alias } if name == "body" && alias.is_none()
        ));
        assert!(plan.filter.is_some());
        assert_eq!(plan.order.len(), 1);
        assert!(matches!(
            &plan.order[0].expr,
            Expr::Column(field) if field == "title"
        ));
        assert!(matches!(plan.order[0].direction, SortDirection::Desc));
        assert_eq!(plan.limit, Some(2));
        assert_eq!(plan.offset, Some(1));
    });
}

#[test]
fn should_default_offset_to_zero_in_optimizer() {
    // Arrange
    let catalog = Catalog::new();
    register_test_collection(&catalog, "planner_offset_default");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let parsed = parser::parse_statement(
            "SELECT title FROM planner_offset_default ORDER BY title ASC LIMIT 3",
        )
        .unwrap();
        let bound = binder::bind(parsed, &catalog).unwrap();
        let logical = logical::plan(&bound).unwrap();

        // Act
        let optimized = optimizer::optimize(logical);

        // Assert
        assert_eq!(optimized.offset, Some(0));
        assert_eq!(optimized.limit, Some(3));
        assert!(matches!(optimized.order[0].direction, SortDirection::Asc));
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
fn should_select_scalar_index_for_equality_filter() {
    // Arrange
    let catalog = Catalog::new();
    register_test_collection(&catalog, "planner_index_aware");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        catalog.register_index(cassie::catalog::IndexMeta {
            collection: "planner_index_aware".to_string(),
            name: "planner_index_aware_title_idx".to_string(),
            field: "title".to_string(),
            fields: vec!["title".to_string()],
            include_fields: Vec::new(),
            kind: cassie::catalog::IndexKind::Scalar,
            unique: false,
            options: Default::default(),
        });
        let parsed =
            parser::parse_statement("SELECT body FROM planner_index_aware WHERE title = 'alpha'")
                .unwrap();
        let bound = binder::bind(parsed, &catalog).unwrap();
        let logical = logical::plan(&bound).unwrap();
        let logical = optimizer::optimize(logical);

        // Act
        let cardinality_stats = std::collections::HashMap::new();
        let physical_plan =
            physical::build_with_indexes(logical, bound.indexes, &cardinality_stats);

        // Assert
        assert_eq!(
            physical_plan.selected_index.as_deref(),
            Some("planner_index_aware_title_idx")
        );
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
            physical::build_with_indexes(logical, bound.indexes, &cardinality_stats);

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
        },
    );
    catalog.register_index(cassie::catalog::IndexMeta {
        collection: "planner_cardinality_hydrated".to_string(),
        name: "planner_cardinality_hydrated_title_idx".to_string(),
        field: "title".to_string(),
        fields: vec!["title".to_string()],
        include_fields: Vec::new(),
        kind: cassie::catalog::IndexKind::Scalar,
        unique: false,
        options: Default::default(),
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
        let cardinality_stats = catalog.cardinality.read().clone();

        // Act
        let physical_plan =
            physical::build_with_indexes(logical, bound.indexes, &cardinality_stats);

        // Assert
        assert_eq!(physical_plan.estimates.scan_rows, 42);
        assert_eq!(physical_plan.estimates.index_rows, 7);
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

#[test]
fn should_mark_scalar_index_plan_as_covered() {
    // Arrange
    let catalog = Catalog::new();
    register_test_collection(&catalog, "planner_covering_index");
    register_scalar_index(
        &catalog,
        "planner_covering_index",
        "planner_covering_title_idx",
        vec!["title"],
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let parsed = parser::parse_statement(
            "SELECT title FROM planner_covering_index WHERE title = 'alpha' ORDER BY title",
        )
        .unwrap();
        let bound = binder::bind(parsed, &catalog).unwrap();
        let logical = logical::plan(&bound).unwrap();

        // Act
        let physical_plan = physical::build_with_indexes(
            logical,
            catalog.list_indexes("planner_covering_index"),
            &Default::default(),
        );

        // Assert
        assert_eq!(
            physical_plan.selected_index.as_deref(),
            Some("planner_covering_title_idx")
        );
        assert!(physical_plan.covered_index);
    });
}

#[test]
fn should_leave_noncovered_scalar_index_plan_uncovered() {
    // Arrange
    let catalog = Catalog::new();
    register_test_collection(&catalog, "planner_covering_fallback");
    register_scalar_index(
        &catalog,
        "planner_covering_fallback",
        "planner_covering_fallback_title_idx",
        vec!["title"],
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let parsed = parser::parse_statement(
            "SELECT body FROM planner_covering_fallback WHERE title = 'alpha'",
        )
        .unwrap();
        let bound = binder::bind(parsed, &catalog).unwrap();
        let logical = logical::plan(&bound).unwrap();

        // Act
        let physical_plan = physical::build_with_indexes(
            logical,
            catalog.list_indexes("planner_covering_fallback"),
            &Default::default(),
        );

        // Assert
        assert_eq!(
            physical_plan.selected_index.as_deref(),
            Some("planner_covering_fallback_title_idx")
        );
        assert!(!physical_plan.covered_index);
    });
}

#[test]
fn should_mark_include_column_plan_as_covered() {
    // Arrange
    let catalog = Catalog::new();
    register_test_collection(&catalog, "planner_include_covering");
    catalog.register_index(IndexMeta {
        collection: "planner_include_covering".to_string(),
        name: "planner_include_covering_title_idx".to_string(),
        field: "title".to_string(),
        fields: vec!["title".to_string()],
        include_fields: vec!["body".to_string()],
        kind: IndexKind::Scalar,
        unique: false,
        options: BTreeMap::new(),
    });
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let parsed = parser::parse_statement(
            "SELECT body FROM planner_include_covering WHERE title = 'alpha'",
        )
        .unwrap();
        let bound = binder::bind(parsed, &catalog).unwrap();
        let logical = logical::plan(&bound).unwrap();

        // Act
        let physical_plan = physical::build_with_indexes(
            logical,
            catalog.list_indexes("planner_include_covering"),
            &Default::default(),
        );

        // Assert
        assert_eq!(
            physical_plan.selected_index.as_deref(),
            Some("planner_include_covering_title_idx")
        );
        assert!(physical_plan.covered_index);
    });
}

#[test]
fn should_emit_offset_node_even_with_default_zero() {
    // Arrange
    let catalog = Catalog::new();
    register_test_collection(&catalog, "planner_default_offset_operator");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let parsed = parser::parse_statement(
            "SELECT id FROM planner_default_offset_operator ORDER BY id ASC LIMIT 5",
        )
        .unwrap();
        let bound = binder::bind(parsed, &catalog).unwrap();
        let logical = logical::plan(&bound).unwrap();

        // Act
        let optimized = optimizer::optimize(logical);
        let physical_plan = physical::build(optimized);

        // Assert
        assert_eq!(physical_plan.operators.len(), 5);
        assert!(matches!(
            physical_plan.operators.get(4),
            Some(Operator::Limit)
        ));
        assert!(matches!(
            physical_plan.operators.get(3),
            Some(Operator::Offset)
        ));
    });
}

#[test]
fn should_keep_collection_clause_values_in_logical_plan() {
    // Arrange
    let catalog = Catalog::new();
    register_test_collection(&catalog, "planner_clauses");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let parsed = parser::parse_statement(
            "SELECT * FROM planner_clauses WHERE body = 'hello' ORDER BY title ASC LIMIT 1 OFFSET 2",
        )
        .unwrap();
        let bound = binder::bind(parsed, &catalog).unwrap();

        // Act
        let logical = logical::plan(&bound).unwrap();

        // Assert
        assert_eq!(logical.collection, "planner_clauses");
        assert!(matches!(&logical.projection[..], [SelectItem::Wildcard]));
        assert_eq!(logical.limit, Some(1));
        assert_eq!(logical.offset, Some(2));
        assert_eq!(logical.order.len(), 1);

        match logical.filter.as_ref().expect("filter should exist") {
            Expr::Binary { left, right, op } => {
                assert!(matches!(op, BinaryOp::Eq));
                assert!(matches!(left.as_ref(), Expr::Column(name) if name == "body"));
                assert!(matches!(right.as_ref(), Expr::StringLiteral(query) if query == "hello"));
            }
            _ => panic!("expected filter expression"),
        }

        assert!(matches!(
            &logical.order[0].expr,
            Expr::Column(field) if field == "title"
        ));
        assert!(matches!(logical.order[0].direction, SortDirection::Asc));
    });
}

#[test]
fn should_reject_invalid_logical_plan_shape_missing_collection() {
    // Arrange
    let bound = BoundStatement {
        statement: ParsedStatement {
            raw_sql: "SELECT id FROM  LIMIT 1".to_string(),
            statement: QueryStatement::Select(SelectStatement {
                source: QuerySource::Collection("".to_string()),
                ctes: vec![],
                recursive: false,
                distinct: false,
                distinct_on: Vec::new(),
                projection: vec![SelectItem::Column {
                    name: "id".to_string(),
                    alias: None,
                }],
                filter: None,
                group_by: vec![],
                having: None,
                order: vec![],
                limit: Some(1),
                offset: Some(0),
                set: None,
            }),
        },
        indexes: Vec::new(),
    };

    // Act
    let result = logical::plan(&bound);

    // Assert
    let error = result.unwrap_err();
    match error {
        CassieError::Planner(message) => assert!(message.contains("source")),
        other => panic!("unexpected error variant: {other:?}"),
    }
}

#[test]
fn should_reject_invalid_logical_plan_shape_empty_projection() {
    // Arrange
    let bound = BoundStatement {
        statement: ParsedStatement {
            raw_sql: "SELECT FROM planner_projectionless".to_string(),
            statement: QueryStatement::Select(SelectStatement {
                source: QuerySource::Collection("planner_projectionless".to_string()),
                ctes: vec![],
                recursive: false,
                distinct: false,
                distinct_on: Vec::new(),
                projection: vec![],
                filter: None,
                group_by: vec![],
                having: None,
                order: vec![],
                limit: Some(1),
                offset: Some(0),
                set: None,
            }),
        },
        indexes: Vec::new(),
    };

    // Act
    let result = logical::plan(&bound);

    // Assert
    let error = result.unwrap_err();
    match error {
        CassieError::Planner(message) => assert!(message.contains("projection")),
        other => panic!("unexpected error variant: {other:?}"),
    }
}

#[test]
fn should_reject_invalid_logical_plan_shape_negative_offset() {
    // Arrange
    let bound = BoundStatement {
        statement: ParsedStatement {
            raw_sql: "SELECT id FROM planner_negative_offset".to_string(),
            statement: QueryStatement::Select(SelectStatement {
                source: QuerySource::Collection("planner_negative_offset".to_string()),
                ctes: vec![],
                recursive: false,
                distinct: false,
                distinct_on: Vec::new(),
                projection: vec![SelectItem::Column {
                    name: "id".to_string(),
                    alias: None,
                }],
                filter: None,
                group_by: vec![],
                having: None,
                order: vec![],
                limit: Some(10),
                offset: Some(-1),
                set: None,
            }),
        },
        indexes: Vec::new(),
    };

    // Act
    let result = logical::plan(&bound);

    // Assert
    let error = result.unwrap_err();
    match error {
        CassieError::Planner(message) => assert!(message.contains("offset")),
        other => panic!("unexpected error variant: {other:?}"),
    }
}

#[test]
fn should_reject_invalid_logical_plan_shape_negative_limit() {
    // Arrange
    let bound = BoundStatement {
        statement: ParsedStatement {
            raw_sql: "SELECT id FROM planner_negative_limit".to_string(),
            statement: QueryStatement::Select(SelectStatement {
                source: QuerySource::Collection("planner_negative_limit".to_string()),
                ctes: vec![],
                recursive: false,
                distinct: false,
                distinct_on: Vec::new(),
                projection: vec![SelectItem::Column {
                    name: "id".to_string(),
                    alias: None,
                }],
                filter: None,
                group_by: vec![],
                having: None,
                order: vec![],
                limit: Some(-10),
                offset: Some(0),
                set: None,
            }),
        },
        indexes: Vec::new(),
    };

    // Act
    let result = logical::plan(&bound);

    // Assert
    let error = result.unwrap_err();
    match error {
        CassieError::Planner(message) => assert!(message.contains("limit")),
        other => panic!("unexpected error variant: {other:?}"),
    }
}

#[test]
fn should_be_deterministic_for_repeated_planning_of_same_query() {
    // Arrange
    let catalog = Catalog::new();
    register_test_collection(&catalog, "planner_repeat_logical");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let parsed = parser::parse_statement(
            "SELECT title FROM planner_repeat_logical WHERE title = 'gamma' ORDER BY title ASC LIMIT 3 OFFSET 1",
        )
        .unwrap();
        let bound = binder::bind(parsed, &catalog).unwrap();

        // Act
        let first = logical::plan(&bound).unwrap();
        let second = logical::plan(&bound).unwrap();

        // Assert
        assert_eq!(format!("{:?}", first), format!("{:?}", second));
    });
}

#[test]
fn should_be_deterministic_for_repeated_optimization_of_same_logical_plan() {
    // Arrange
    let catalog = Catalog::new();
    register_test_collection(&catalog, "planner_repeat_optimizer");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let parsed = parser::parse_statement(
            "SELECT title FROM planner_repeat_optimizer WHERE title = 'gamma' ORDER BY title ASC LIMIT 3",
        )
        .unwrap();
        let bound = binder::bind(parsed, &catalog).unwrap();
        let logical_plan = logical::plan(&bound).unwrap();

        // Act
        let first = optimizer::optimize(logical_plan.clone());
        let second = optimizer::optimize(logical_plan);

        // Assert
        assert_eq!(format!("{:?}", first), format!("{:?}", second));
        assert_eq!(first.offset, Some(0));
        assert_eq!(second.offset, Some(0));
    });
}

#[test]
fn should_plan_non_recursive_cte_source_as_logical_source() {
    // Arrange
    let catalog = Catalog::new();
    register_test_collection(&catalog, "planner_cte_source");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let parsed = parser::parse_statement(
            "WITH docs_cte AS (SELECT title FROM planner_cte_source) SELECT title FROM docs_cte",
        )
        .unwrap();
        let bound = binder::bind(parsed, &catalog).unwrap();

        // Act
        let logical = logical::plan(&bound).unwrap();

        // Assert
        assert_eq!(logical.ctes.len(), 1);
        assert_eq!(logical.source, QuerySource::Cte("docs_cte".to_string()));
        assert_eq!(logical.collection, "docs_cte");
        assert_eq!(logical.ctes[0].name, "docs_cte");
    });
}

#[test]
fn should_preserve_recursive_cte_aliases_in_logical_plan() {
    // Arrange
    let catalog = Catalog::new();
    register_test_collection(&catalog, "planner_recursive_aliases");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let parsed = parser::parse_statement(
            "WITH RECURSIVE seq(id) AS (SELECT id FROM planner_recursive_aliases UNION ALL SELECT id FROM seq) SELECT id FROM seq",
        )
        .unwrap();
        let bound = binder::bind(parsed, &catalog).unwrap();

        // Act
        let logical = logical::plan(&bound).unwrap();

        // Assert
        assert_eq!(logical.ctes.len(), 1);
        assert_eq!(logical.ctes[0].aliases, vec!["id".to_string()]);
        assert_eq!(logical.ctes[0].name, "seq");
        let recursive = matches!(
            logical.ctes[0].query,
            cassie::sql::ast::CteQuery::Recursive { .. }
        );
        assert!(recursive);
    });
}

#[test]
fn should_plan_create_table_as_command() {
    // Arrange
    let catalog = Catalog::new();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        // Act
        let parsed =
            parser::parse_statement("CREATE TABLE planner_create (id INT, title TEXT, body TEXT)")
                .unwrap();
        let bound = binder::bind(parsed, &catalog).unwrap();
        let plan = logical::plan(&bound).unwrap();

        // Assert
        assert_eq!(plan.collection, "planner_create");
        assert!(plan.command.is_some());
        assert!(plan.projection.is_empty());
        assert!(plan.filter.is_none());
    });
}

#[test]
fn should_plan_drop_table_as_command() {
    // Arrange
    let catalog = Catalog::new();
    register_test_collection(&catalog, "planner_drop");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        // Act
        let parsed = parser::parse_statement("DROP TABLE planner_drop").unwrap();
        let bound = binder::bind(parsed, &catalog).unwrap();
        let plan = logical::plan(&bound).unwrap();

        // Assert
        assert_eq!(plan.collection, "planner_drop");
        assert!(plan.command.is_some());
    });
}

#[test]
fn should_plan_insert_values_as_command() {
    // Arrange
    let catalog = Catalog::new();
    register_test_collection(&catalog, "planner_insert_values");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        // Act
        let parsed =
            parser::parse_statement("INSERT INTO planner_insert_values (title) VALUES ('alpha')")
                .unwrap();
        let bound = binder::bind(parsed, &catalog).unwrap();
        let plan = logical::plan(&bound).unwrap();

        // Assert
        assert_eq!(plan.collection, "planner_insert_values");
        match plan.command.as_ref().expect("insert command") {
            logical::LogicalCommand::Insert(statement) => {
                assert_eq!(statement.table, "planner_insert_values");
                assert_eq!(statement.columns, vec!["title".to_string()]);
                assert!(matches!(statement.source, InsertSource::Values(_)));
            }
            _ => panic!("expected insert command"),
        }
    });
}

#[test]
fn should_plan_insert_select_as_command() {
    // Arrange
    let catalog = Catalog::new();
    register_test_collection(&catalog, "planner_insert_select_target");
    register_test_collection(&catalog, "planner_insert_select_source");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        // Act
        let parsed = parser::parse_statement(
            "INSERT INTO planner_insert_select_target (title) SELECT title FROM planner_insert_select_source",
        )
        .unwrap();
        let bound = binder::bind(parsed, &catalog).unwrap();
        let plan = logical::plan(&bound).unwrap();

        // Assert
        assert_eq!(plan.collection, "planner_insert_select_target");
        match plan.command.as_ref().expect("insert command") {
            logical::LogicalCommand::Insert(statement) => {
                assert_eq!(statement.table, "planner_insert_select_target");
                assert_eq!(statement.columns, vec!["title".to_string()]);
                assert!(matches!(statement.source, InsertSource::Select(_)));
            }
            _ => panic!("expected insert command"),
        }
    });
}

#[test]
fn should_plan_update_as_command() {
    // Arrange
    let catalog = Catalog::new();
    register_test_collection(&catalog, "planner_update");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        // Act
        let parsed = parser::parse_statement(
            "UPDATE planner_update SET title = 'alpha' WHERE body = 'old' RETURNING title",
        )
        .unwrap();
        let bound = binder::bind(parsed, &catalog).unwrap();
        let plan = logical::plan(&bound).unwrap();

        // Assert
        assert_eq!(plan.collection, "planner_update");
        match plan.command.as_ref().expect("update command") {
            logical::LogicalCommand::Update(statement) => {
                assert_eq!(statement.table, "planner_update");
                assert_eq!(statement.assignments.len(), 1);
                assert!(statement.filter.is_some());
                assert_eq!(statement.returning.len(), 1);
            }
            _ => panic!("expected update command"),
        }
    });
}

#[test]
fn should_plan_delete_as_command() {
    // Arrange
    let catalog = Catalog::new();
    register_test_collection(&catalog, "planner_delete");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        // Act
        let parsed = parser::parse_statement(
            "DELETE FROM planner_delete WHERE title = 'alpha' RETURNING title",
        )
        .unwrap();
        let bound = binder::bind(parsed, &catalog).unwrap();
        let plan = logical::plan(&bound).unwrap();

        // Assert
        assert_eq!(plan.collection, "planner_delete");
        match plan.command.as_ref().expect("delete command") {
            logical::LogicalCommand::Delete(statement) => {
                assert_eq!(statement.table, "planner_delete");
                assert!(statement.filter.is_some());
                assert_eq!(statement.returning.len(), 1);
            }
            _ => panic!("expected delete command"),
        }
    });
}

#[test]
fn should_plan_alter_table_command() {
    // Arrange
    let catalog = Catalog::new();
    register_test_collection(&catalog, "planner_alter");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        // Act
        let parsed =
            parser::parse_statement("ALTER TABLE planner_alter ADD COLUMN score FLOAT").unwrap();
        let bound = binder::bind(parsed, &catalog).unwrap();
        let plan = logical::plan(&bound).unwrap();

        // Assert
        assert_eq!(plan.collection, "planner_alter");
        assert!(plan.command.is_some());
        match plan.command.as_ref().unwrap() {
            logical::LogicalCommand::AlterTable(statement) => {
                assert_eq!(statement.table, "planner_alter");
            }
            _ => panic!("expected alter table command"),
        }
    });
}

#[test]
fn should_plan_create_index_as_command() {
    // Arrange
    let catalog = Catalog::new();
    register_test_collection(&catalog, "planner_create_index");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        // Act
        let parsed = parser::parse_statement(
            "CREATE UNIQUE INDEX planner_idx_title ON planner_create_index USING btree (title)",
        )
        .unwrap();
        let bound = binder::bind(parsed, &catalog).unwrap();
        let plan = logical::plan(&bound).unwrap();

        // Assert
        assert_eq!(plan.collection, "planner_create_index");
        assert!(plan.command.is_some());
        match plan.command.as_ref().unwrap() {
            logical::LogicalCommand::CreateIndex(statement) => {
                assert_eq!(statement.name, "planner_idx_title");
                assert!(statement.unique);
            }
            _ => panic!("expected create index command"),
        }
    });
}

#[test]
fn should_plan_drop_index_as_command() {
    // Arrange
    let catalog = Catalog::new();
    register_test_collection(&catalog, "planner_drop_index");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        catalog.register_index(cassie::catalog::IndexMeta {
            collection: "planner_drop_index".to_string(),
            name: "planner_idx_title".to_string(),
            field: "title".to_string(),
            fields: vec!["title".to_string()],
            include_fields: Vec::new(),
            kind: cassie::catalog::IndexKind::Scalar,
            unique: false,
            options: std::collections::BTreeMap::new(),
        });

        // Act
        let parsed =
            parser::parse_statement("DROP INDEX planner_idx_title ON planner_drop_index").unwrap();
        let bound = binder::bind(parsed, &catalog).unwrap();
        let plan = logical::plan(&bound).unwrap();

        // Assert
        assert_eq!(plan.collection, "planner_drop_index");
        assert!(plan.command.is_some());
        match plan.command.as_ref().unwrap() {
            logical::LogicalCommand::DropIndex(statement) => {
                assert_eq!(statement.name, "planner_idx_title");
                assert_eq!(statement.table, "planner_drop_index");
            }
            _ => panic!("expected drop index command"),
        }
    });
}
