use cassie::app::CassieError;
use cassie::catalog::Catalog;
use cassie::planner::{logical, optimizer, physical, physical::Operator};
use cassie::sql::ast::{
    BinaryOp, Expr, ParsedStatement, QueryStatement, SelectItem, SelectStatement, SortDirection,
};
use cassie::sql::binder::BoundStatement;
use cassie::sql::{binder, parser};
use cassie::types::{DataType, FieldSchema};

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
        catalog
            .register_collection(
                name,
                schema
                    .into_iter()
                    .map(|field| (field.name, field.data_type))
                    .collect(),
            )
            .await;
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
        let bound = binder::bind(parsed, &catalog).await.unwrap();
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
        let bound = binder::bind(parsed, &catalog).await.unwrap();
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
        let bound = binder::bind(parsed, &catalog).await.unwrap();
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
        let bound = binder::bind(parsed, &catalog).await.unwrap();
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
        let bound = binder::bind(parsed, &catalog).await.unwrap();

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
                collection: "".to_string(),
                projection: vec![SelectItem::Column {
                    name: "id".to_string(),
                    alias: None,
                }],
                filter: None,
                order: vec![],
                limit: Some(1),
                offset: Some(0),
            }),
        },
    };

    // Act
    let result = logical::plan(&bound);

    // Assert
    let error = result.unwrap_err();
    match error {
        CassieError::Planner(message) => assert!(message.contains("collection")),
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
                collection: "planner_projectionless".to_string(),
                projection: vec![],
                filter: None,
                order: vec![],
                limit: Some(1),
                offset: Some(0),
            }),
        },
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
                collection: "planner_negative_offset".to_string(),
                projection: vec![SelectItem::Column {
                    name: "id".to_string(),
                    alias: None,
                }],
                filter: None,
                order: vec![],
                limit: Some(10),
                offset: Some(-1),
            }),
        },
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
                collection: "planner_negative_limit".to_string(),
                projection: vec![SelectItem::Column {
                    name: "id".to_string(),
                    alias: None,
                }],
                filter: None,
                order: vec![],
                limit: Some(-10),
                offset: Some(0),
            }),
        },
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
        let bound = binder::bind(parsed, &catalog).await.unwrap();

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
        let bound = binder::bind(parsed, &catalog).await.unwrap();
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
