use cassie::app::CassieError;
use cassie::catalog::Catalog;
use cassie::planner::{logical, optimizer, physical, physical::Operator};
use cassie::sql::ast::{
    BinaryOp, Expr, InsertSource, ParsedStatement, QuerySource, QueryStatement, SelectItem,
    SelectStatement, SortDirection,
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
                source: QuerySource::Collection("".to_string()),
                ctes: vec![],
                recursive: false,
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
                source: QuerySource::Collection("planner_negative_offset".to_string()),
                ctes: vec![],
                recursive: false,
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
                source: QuerySource::Collection("planner_negative_limit".to_string()),
                ctes: vec![],
                recursive: false,
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
        let bound = binder::bind(parsed, &catalog).await.unwrap();

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
        let bound = binder::bind(parsed, &catalog).await.unwrap();

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
        let bound = binder::bind(parsed, &catalog).await.unwrap();
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
        let bound = binder::bind(parsed, &catalog).await.unwrap();
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
        let bound = binder::bind(parsed, &catalog).await.unwrap();
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
        let bound = binder::bind(parsed, &catalog).await.unwrap();
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
        let bound = binder::bind(parsed, &catalog).await.unwrap();
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
        let bound = binder::bind(parsed, &catalog).await.unwrap();
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
        let bound = binder::bind(parsed, &catalog).await.unwrap();
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
        let bound = binder::bind(parsed, &catalog).await.unwrap();
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
        catalog
            .register_index(cassie::catalog::IndexMeta {
                collection: "planner_drop_index".to_string(),
                name: "planner_idx_title".to_string(),
                field: "title".to_string(),
                kind: cassie::catalog::IndexKind::Scalar,
                unique: false,
                options: std::collections::BTreeMap::new(),
            })
            .await;

        // Act
        let parsed =
            parser::parse_statement("DROP INDEX planner_idx_title ON planner_drop_index").unwrap();
        let bound = binder::bind(parsed, &catalog).await.unwrap();
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
