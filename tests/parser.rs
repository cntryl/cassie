use cassie::app::Cassie;
use cassie::catalog::{IndexKind, IndexMeta};
use cassie::sql::ast::{
    BinaryOp, CteQuery, Expr, InsertSource, QuerySource, QueryStatement, SelectItem, SortDirection,
};
use cassie::sql::parse_statement;
use cassie::types::{DataType, FieldSchema, Schema};
use std::collections::BTreeMap;
use uuid::Uuid;

#[test]
fn should_parse_select_statement_with_aliases_filters_sorting_pagination() {
    // Arrange
    let sql = "SELECT title AS doc_title, search_score(body, 'world') AS score FROM docs WHERE active = true AND title <> 'bad' ORDER BY score DESC, id LIMIT 10 OFFSET 5";

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    let QueryStatement::Select(statement) = parsed.statement else {
        panic!("expected select statement");
    };

    assert_eq!(
        statement.source,
        QuerySource::Collection("docs".to_string())
    );
    assert_eq!(statement.limit, Some(10));
    assert_eq!(statement.offset, Some(5));

    assert_eq!(statement.projection.len(), 2);
    match &statement.projection[0] {
        SelectItem::Column { name, alias } => {
            assert_eq!(name, "title");
            assert_eq!(alias.as_deref(), Some("doc_title"));
        }
        _ => panic!("expected column projection"),
    }

    match &statement.projection[1] {
        SelectItem::Function { function, alias } => {
            assert_eq!(function.name, "search_score");
            assert_eq!(alias.as_deref(), Some("score"));
        }
        _ => panic!("expected function projection"),
    }

    let filter = statement.filter.expect("filter expected");
    let Expr::Binary { op: _, .. } = filter else {
        panic!("filter should be binary")
    };

    assert_eq!(statement.order.len(), 2);
    assert!(matches!(statement.order[0].direction, SortDirection::Desc));
    assert!(matches!(statement.order[1].direction, SortDirection::Asc));
}

#[test]
fn should_parse_create_function_statement() {
    // Arrange
    let sql = "CREATE FUNCTION double(x INT) RETURNS INT AS \"x * 2\"";

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    let QueryStatement::CreateFunction(statement) = parsed.statement else {
        panic!("expected create function");
    };

    assert_eq!(statement.name, "double");
    assert_eq!(statement.args.len(), 1);
    assert_eq!(statement.args[0].name, "x");
    assert_eq!(statement.args[0].data_type, DataType::Int);
    assert_eq!(statement.return_type, DataType::Int);
    assert_eq!(statement.body, "x * 2");
}

#[test]
fn should_parse_create_procedure_statement() {
    // Arrange
    let sql = "CREATE PROCEDURE log_event(message TEXT) AS \"noop\"";

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    let QueryStatement::CreateProcedure(statement) = parsed.statement else {
        panic!("expected create procedure");
    };

    assert_eq!(statement.name, "log_event");
    assert_eq!(statement.args.len(), 1);
    assert_eq!(statement.args[0].name, "message");
    assert_eq!(statement.args[0].data_type, DataType::Text);
    assert_eq!(statement.body, "noop");
}

#[test]
fn should_reject_unknown_function_during_binding() {
    // Arrange
    let cassie =
        Cassie::new_with_data_dir(format!("/tmp/cassie-parser-{}", Uuid::new_v4())).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        cassie
            .register_collection(
                "binder_docs".to_string(),
                Schema {
                    fields: vec![FieldSchema {
                        name: "id".to_string(),
                        data_type: DataType::Text,
                        nullable: true,
                    }],
                },
            )
            .await;

        // Act
        let parsed = parse_statement("SELECT unknown_fn(id) FROM binder_docs").unwrap();
        let bound = cassie::sql::binder::bind(parsed, &cassie.catalog).await;

        // Assert
        assert!(bound.is_err());
    });
}

#[test]
fn should_reject_bad_function_arity_during_binding() {
    // Arrange
    let cassie =
        Cassie::new_with_data_dir(format!("/tmp/cassie-parser-{}", Uuid::new_v4())).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        cassie
            .register_collection(
                "binder_docs_arity".to_string(),
                Schema {
                    fields: vec![FieldSchema {
                        name: "id".to_string(),
                        data_type: DataType::Text,
                        nullable: true,
                    }],
                },
            )
            .await;

        // Act
        let parsed = parse_statement("SELECT search(id) FROM binder_docs_arity").unwrap();
        let bound = cassie::sql::binder::bind(parsed, &cassie.catalog).await;

        // Assert
        assert!(bound.is_err());
    });
}

#[test]
fn should_parse_pgvector_cosine_ordering() {
    // Arrange
    let sql = "SELECT * FROM docs ORDER BY embedding <=> $1 LIMIT 5";

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    let QueryStatement::Select(statement) = parsed.statement else {
        panic!("expected select statement");
    };
    let order = &statement.order;
    assert_eq!(order.len(), 1);
    let expr = &order[0].expr;
    match expr {
        Expr::Binary {
            op: BinaryOp::PgvectorCosine,
            ..
        } => {}
        _ => panic!("expected pgvector cosine order operator"),
    }
    assert_eq!(statement.limit, Some(5));
}

#[test]
fn should_parse_pgvector_dot_ordering() {
    // Arrange
    let sql = "SELECT * FROM docs ORDER BY embedding <#> $1 LIMIT 5";

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    let QueryStatement::Select(statement) = parsed.statement else {
        panic!("expected select statement");
    };
    let expr = &statement.order[0].expr;
    match expr {
        Expr::Binary {
            op: BinaryOp::PgvectorDot,
            ..
        } => {}
        _ => panic!("expected pgvector dot order operator"),
    }
}

#[test]
fn should_parse_vector_function_argument_with_commas() {
    // Arrange
    let sql = "SELECT vector_score(embedding, '[1,0]') FROM docs";

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    let QueryStatement::Select(statement) = parsed.statement else {
        panic!("expected select statement");
    };
    let projection = &statement.projection[0];
    match projection {
        SelectItem::Function { function, .. } => {
            assert_eq!(function.name, "vector_score");
            assert_eq!(function.args.len(), 2);
            assert!(matches!(function.args[0], Expr::Column(ref column) if column == "embedding"));
            assert!(matches!(&function.args[1], Expr::StringLiteral(value) if value == "[1,0]"));
        }
        _ => panic!("expected vector function"),
    }
}

#[test]
fn should_parse_pgvector_l2_ordering() {
    // Arrange
    let sql = "SELECT * FROM docs ORDER BY embedding <-> $1 LIMIT 5";

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    let QueryStatement::Select(statement) = parsed.statement else {
        panic!("expected select statement");
    };
    let expr = &statement.order[0].expr;
    match expr {
        Expr::Binary {
            op: BinaryOp::PgvectorL2,
            ..
        } => {}
        _ => panic!("expected pgvector l2 order operator"),
    }
}

#[test]
fn should_parse_boolean_precedence_in_where_clause() {
    // Arrange
    let sql = "SELECT * FROM docs WHERE title = 'alpha' OR title = 'beta' AND active = true";

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    let QueryStatement::Select(statement) = parsed.statement else {
        panic!("expected select statement");
    };
    let filter = statement.filter.expect("filter expected");

    let Expr::Binary {
        op: or_op,
        left: left_expr,
        right: right_expr,
    } = filter
    else {
        panic!("expected binary filter");
    };
    assert!(matches!(or_op, BinaryOp::Or));

    match left_expr.as_ref() {
        Expr::Binary {
            op: BinaryOp::Eq, ..
        } => {}
        _ => panic!("expected OR left-side equality"),
    }

    match right_expr.as_ref() {
        Expr::Binary {
            op: BinaryOp::And, ..
        } => {}
        _ => panic!("expected OR right-side conjunction"),
    }
}

#[test]
fn should_parse_parenthesized_where_changes_precedence() {
    // Arrange
    let sql = "SELECT * FROM docs WHERE (title = 'alpha' OR title = 'beta') AND active = true";

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    let QueryStatement::Select(statement) = parsed.statement else {
        panic!("expected select statement");
    };
    let filter = statement.filter.expect("filter expected");

    let Expr::Binary {
        op: and_op,
        left,
        right,
    } = filter
    else {
        panic!("expected binary filter");
    };
    assert!(matches!(and_op, BinaryOp::And));

    match left.as_ref() {
        Expr::Binary {
            op: BinaryOp::Or, ..
        } => {}
        _ => panic!("expected grouped OR on the left side"),
    }

    match right.as_ref() {
        Expr::Binary {
            op: BinaryOp::Eq, ..
        } => {}
        _ => panic!("expected active = true predicate on right side"),
    }
}

#[test]
fn should_reject_negative_offset() {
    // Arrange
    let sql = "SELECT * FROM docs ORDER BY id OFFSET -1";

    // Act
    let parsed = parse_statement(sql);

    // Assert
    assert!(parsed.is_err());
}

#[test]
fn should_reject_negative_limit() {
    // Arrange
    let sql = "SELECT * FROM docs LIMIT -5";

    // Act
    let parsed = parse_statement(sql);

    // Assert
    assert!(parsed.is_err());
}

#[test]
fn should_parse_parameter_positions() {
    // Arrange
    let sql = "SELECT * FROM docs WHERE title = $2 AND id = $1";

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    let QueryStatement::Select(statement) = parsed.statement else {
        panic!("expected select statement");
    };
    let filter = statement.filter.expect("filter expected");

    let Expr::Binary { left, right, op: _ } = filter else {
        panic!("expected parameterized filter");
    };

    let (left_left, left_right) = match left.as_ref() {
        Expr::Binary {
            op: BinaryOp::Eq,
            left,
            right,
            ..
        } => (left.as_ref(), right.as_ref()),
        _ => panic!("expected lhs parameterized equality"),
    };

    assert!(matches!(left_left, Expr::Column(_)));
    assert!(matches!(left_right, Expr::Param(1)));

    let (right_left, right_right) = match right.as_ref() {
        Expr::Binary {
            op: BinaryOp::Eq,
            left,
            right,
            ..
        } => (left.as_ref(), right.as_ref()),
        _ => panic!("expected rhs parameterized equality"),
    };

    assert!(matches!(right_left, Expr::Column(_)));
    assert!(matches!(right_right, Expr::Param(0)));
}

#[test]
fn should_accept_case_insensitive_function_names_during_binding() {
    // Arrange
    let cassie =
        Cassie::new_with_data_dir(format!("/tmp/cassie-parser-{}", Uuid::new_v4())).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        cassie
            .register_collection(
                "binder_docs_case".to_string(),
                Schema {
                    fields: vec![FieldSchema {
                        name: "body".to_string(),
                        data_type: DataType::Text,
                        nullable: true,
                    }],
                },
            )
            .await;

        // Act
        let parsed =
            parse_statement("SELECT SEARCH_SCORE(body, 'q') FROM binder_docs_case").unwrap();
        let bound = cassie::sql::binder::bind(parsed, &cassie.catalog).await;

        // Assert
        assert!(bound.is_ok(), "function names should be case-insensitive");
    });
}

#[test]
fn should_allow_snippet_function_binding() {
    // Arrange
    let cassie =
        Cassie::new_with_data_dir(format!("/tmp/cassie-parser-{}", Uuid::new_v4())).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        cassie
            .register_collection(
                "binder_docs_snippet".to_string(),
                Schema {
                    fields: vec![FieldSchema {
                        name: "body".to_string(),
                        data_type: DataType::Text,
                        nullable: true,
                    }],
                },
            )
            .await;

        // Act
        let parsed = parse_statement("SELECT snippet(body, 'q') FROM binder_docs_snippet").unwrap();
        let bound = cassie::sql::binder::bind(parsed, &cassie.catalog).await;

        // Assert
        assert!(bound.is_ok());
    });
}

#[test]
fn should_parse_non_select_statement() {
    // Arrange
    let sql = "INSERT INTO docs VALUES (1)";

    // Act
    let parsed = parse_statement(sql).expect("insert statements should parse");

    // Assert
    assert!(matches!(parsed.statement, QueryStatement::Insert(_)));
}

#[test]
fn should_bind_insert_statement_for_existing_collection() {
    // Arrange
    let sql = "INSERT INTO docs VALUES (1)";
    let cassie = Cassie::new_with_data_dir(format!(
        "/tmp/cassie-parser-insert-binding-{}",
        Uuid::new_v4()
    ))
    .unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        cassie
            .catalog
            .register_collection("docs", vec![("id".to_string(), DataType::Int)])
            .await;

        // Act
        let parsed = parse_statement(sql).expect("insert statements should parse");
        let bound = cassie::sql::binder::bind(parsed, &cassie.catalog).await;

        // Assert
        assert!(
            bound.is_ok(),
            "insert binding should succeed for known tables"
        );
    });
}

#[test]
fn should_parse_insert_with_explicit_columns() {
    // Arrange
    let sql = "INSERT INTO docs (title) VALUES ('alpha')";

    // Act
    let parsed = parse_statement(sql).expect("insert parse");

    // Assert
    let QueryStatement::Insert(statement) = parsed.statement else {
        panic!("expected insert statement");
    };
    assert_eq!(statement.table, "docs");
    assert_eq!(statement.columns, vec!["title".to_string()]);
    let value = match &statement.source {
        InsertSource::Values(values) => {
            let value = values.first().expect("missing insert value");
            if let Expr::StringLiteral(value) = value {
                value
            } else {
                panic!("expected string literal");
            }
        }
        _ => panic!("expected values source"),
    };
    assert_eq!(value, "alpha");
}

#[test]
fn should_parse_insert_select_source() {
    // Arrange
    let sql = "INSERT INTO docs SELECT title FROM docs";

    // Act
    let parsed = parse_statement(sql).expect("insert parse");

    // Assert
    let QueryStatement::Insert(statement) = parsed.statement else {
        panic!("expected insert statement");
    };
    assert_eq!(statement.table, "docs");
    match statement.source {
        InsertSource::Select(_) => {}
        InsertSource::Values(_) => panic!("expected select source"),
    }
}

#[test]
fn should_reject_insert_on_conflict() {
    // Arrange
    let sql = "INSERT INTO docs VALUES (1) ON CONFLICT DO NOTHING";

    // Act
    let parsed = parse_statement(sql);

    // Assert
    assert!(parsed.is_err());
}

#[test]
fn should_parse_transaction_control_statements() {
    // Arrange
    let statements = ["BEGIN", "COMMIT", "ROLLBACK"];

    // Act
    let parsed = statements
        .iter()
        .map(|sql| parse_statement(sql))
        .collect::<Vec<_>>();

    // Assert
    assert!(parsed.iter().all(Result::is_ok));
    assert!(matches!(
        parsed[0].as_ref().unwrap().statement,
        QueryStatement::Transaction(_)
    ));
    assert!(matches!(
        parsed[1].as_ref().unwrap().statement,
        QueryStatement::Transaction(_)
    ));
    assert!(matches!(
        parsed[2].as_ref().unwrap().statement,
        QueryStatement::Transaction(_)
    ));
}

#[test]
fn should_reject_unsupported_transaction_control() {
    // Arrange
    let sql = "SAVEPOINT sp";

    // Act
    let parsed = parse_statement(sql);

    // Assert
    assert!(parsed.is_err());
}

#[test]
fn should_reject_transaction_isolation_level_changes() {
    // Arrange
    let sql = "SET TRANSACTION ISOLATION LEVEL SERIALIZABLE";

    // Act
    let parsed = parse_statement(sql);

    // Assert
    assert!(parsed.is_err());
    assert!(parsed.unwrap_err().0.contains("unsupported"));
}

#[test]
fn should_reject_two_phase_transaction_control() {
    // Arrange
    let sql = "PREPARE TRANSACTION 'tx1'";

    // Act
    let parsed = parse_statement(sql);

    // Assert
    assert!(parsed.is_err());
    assert!(parsed.unwrap_err().0.contains("unsupported"));
}

#[test]
fn should_parse_is_null_predicate() {
    // Arrange
    let sql = "SELECT title FROM docs WHERE archived_at IS NULL";

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    let QueryStatement::Select(statement) = parsed.statement else {
        panic!("expected select statement");
    };
    assert!(matches!(
        statement.filter,
        Some(Expr::IsNull { negated: false, .. })
    ));
}

#[test]
fn should_parse_in_list_predicate() {
    // Arrange
    let sql = "SELECT title FROM docs WHERE title IN ('alpha', 'beta')";

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    let QueryStatement::Select(statement) = parsed.statement else {
        panic!("expected select statement");
    };
    assert!(matches!(
        statement.filter,
        Some(Expr::InList { negated: false, .. })
    ));
}

#[test]
fn should_parse_between_predicate() {
    // Arrange
    let sql = "SELECT title FROM docs WHERE score BETWEEN 10 AND 20";

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    let QueryStatement::Select(statement) = parsed.statement else {
        panic!("expected select statement");
    };
    assert!(matches!(
        statement.filter,
        Some(Expr::Between { negated: false, .. })
    ));
}

#[test]
fn should_parse_cast_function_expression() {
    // Arrange
    let sql = "SELECT title FROM docs WHERE CAST(score AS TEXT) = '10'";

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    let QueryStatement::Select(statement) = parsed.statement else {
        panic!("expected select statement");
    };
    let Some(Expr::Binary { left, .. }) = statement.filter else {
        panic!("expected binary predicate");
    };
    assert!(matches!(*left, Expr::Cast { .. }));
}

#[test]
fn should_parse_postgres_style_cast_expression() {
    // Arrange
    let sql = "SELECT title FROM docs WHERE score::TEXT = '10'";

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    let QueryStatement::Select(statement) = parsed.statement else {
        panic!("expected select statement");
    };
    let Some(Expr::Binary { left, .. }) = statement.filter else {
        panic!("expected binary predicate");
    };
    assert!(matches!(*left, Expr::Cast { .. }));
}

#[test]
fn should_parse_order_by_nulls_last() {
    // Arrange
    let sql = "SELECT title FROM docs ORDER BY archived_at ASC NULLS LAST";

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    let QueryStatement::Select(statement) = parsed.statement else {
        panic!("expected select statement");
    };
    assert!(matches!(
        statement.order[0].nulls,
        Some(cassie::sql::ast::NullsOrder::Last)
    ));
}

#[test]
fn should_parse_exists_predicate() {
    // Arrange
    let sql = "SELECT title FROM docs WHERE EXISTS (SELECT title FROM archive)";

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    let QueryStatement::Select(statement) = parsed.statement else {
        panic!("expected select statement");
    };
    assert!(matches!(statement.filter, Some(Expr::Exists(_))));
}

#[test]
fn should_reject_empty_in_list_predicate() {
    // Arrange
    let sql = "SELECT title FROM docs WHERE title IN ()";

    // Act
    let parsed = parse_statement(sql);

    // Assert
    assert!(parsed.is_err());
    assert!(parsed.unwrap_err().0.contains("IN predicate"));
}

#[test]
fn should_reject_exists_without_select_subquery() {
    // Arrange
    let sql = "SELECT title FROM docs WHERE EXISTS (INSERT INTO docs (title) VALUES ('x'))";

    // Act
    let parsed = parse_statement(sql);

    // Assert
    assert!(parsed.is_err());
    assert!(parsed.unwrap_err().0.contains("EXISTS requires"));
}

#[test]
fn should_reject_unknown_clause_in_query() {
    // Arrange
    let sql = "SELECT * FROM docs GROUP BY title";

    // Act
    let parsed = parse_statement(sql);

    // Assert
    assert!(parsed.is_err());
}

#[test]
fn should_reject_duplicate_limit_clauses() {
    // Arrange
    let sql = "SELECT * FROM docs LIMIT 1 LIMIT 2";

    // Act
    let parsed = parse_statement(sql);

    // Assert
    assert!(parsed.is_err());
}

#[test]
fn should_reject_invalid_limit_values() {
    // Arrange
    let negative_limit = parse_statement("SELECT * FROM docs LIMIT -1");
    let zero_limit = parse_statement("SELECT * FROM docs LIMIT 0");

    // Act
    let zero_offset = parse_statement("SELECT * FROM docs OFFSET 0");

    // Assert
    assert!(negative_limit.is_err());
    assert!(zero_limit.is_ok());
    assert!(zero_offset.is_ok());
}

#[test]
fn should_accept_zero_offset_values() {
    // Arrange
    let zero_offset = parse_statement("SELECT * FROM docs OFFSET 0");

    // Act
    let parsed = zero_offset;

    // Assert
    assert!(parsed.is_ok());
}

#[test]
fn should_reject_malformed_parameter_tokens() {
    // Arrange
    let missing_number = parse_statement("SELECT * FROM docs WHERE title = $");
    let non_numeric = parse_statement("SELECT * FROM docs WHERE title = $x");

    // Act
    let zero_index = parse_statement("SELECT * FROM docs WHERE title = $0");

    // Assert
    assert!(missing_number.is_err());
    assert!(non_numeric.is_err());
    assert!(zero_index.is_err());
}

#[test]
fn should_reject_unknown_trailing_tokens_after_query() {
    // Arrange
    let sql = "SELECT * FROM docs WHERE title = 'a' FOO";

    // Act
    let parsed = parse_statement(sql);

    // Assert
    assert!(parsed.is_err());
}

#[test]
fn should_reject_unresolvable_order_by_identifier_during_binding() {
    // Arrange
    let cassie =
        Cassie::new_with_data_dir(format!("/tmp/cassie-parser-{}", Uuid::new_v4())).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        cassie
            .register_collection(
                "binder_docs_order_alias".to_string(),
                Schema {
                    fields: vec![FieldSchema {
                        name: "body".to_string(),
                        data_type: DataType::Text,
                        nullable: true,
                    }],
                },
            )
            .await;

        // Act
        let parsed = parse_statement(
            "SELECT search_score(body, 'world') AS score FROM binder_docs_order_alias ORDER BY missing_alias",
        )
        .unwrap();
        let bound = cassie::sql::binder::bind(parsed, &cassie.catalog).await;

        // Assert
        assert!(bound.is_err());
    });
}

#[test]
fn should_allow_projection_alias_order_by_during_binding() {
    // Arrange
    let cassie =
        Cassie::new_with_data_dir(format!("/tmp/cassie-parser-{}", Uuid::new_v4())).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        cassie
            .register_collection(
                "binder_docs_order_alias_ok".to_string(),
                Schema {
                    fields: vec![FieldSchema {
                        name: "body".to_string(),
                        data_type: DataType::Text,
                        nullable: true,
                    }],
                },
            )
            .await;

        // Act
        let parsed = parse_statement(
            "SELECT search_score(body, 'world') AS Score FROM binder_docs_order_alias_ok ORDER BY score",
        )
        .unwrap();
        let bound = cassie::sql::binder::bind(parsed, &cassie.catalog).await;

        // Assert
        assert!(bound.is_ok());
    });
}

#[test]
fn should_reject_unknown_projection_column_during_binding() {
    // Arrange
    let cassie =
        Cassie::new_with_data_dir(format!("/tmp/cassie-parser-{}", Uuid::new_v4())).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        cassie
            .register_collection(
                "binder_docs_projection_col".to_string(),
                Schema {
                    fields: vec![FieldSchema {
                        name: "body".to_string(),
                        data_type: DataType::Text,
                        nullable: true,
                    }],
                },
            )
            .await;

        // Act
        let parsed = parse_statement("SELECT unknown FROM binder_docs_projection_col").unwrap();
        let bound = cassie::sql::binder::bind(parsed, &cassie.catalog).await;

        // Assert
        assert!(bound.is_err());
    });
}

#[test]
fn should_parse_deterministically_across_invocations() {
    // Arrange
    let sql_one =
        "SELECT title AS doc_title, search_score(body, 'world') AS score FROM docs WHERE active = true ORDER BY score DESC LIMIT 1 OFFSET 0";
    let sql_two =
        "select title AS doc_title, search_score(body, 'world') AS score from docs where active = true order by score desc limit 1 offset 0";

    // Act
    let first = parse_statement(sql_one).unwrap();
    let second = parse_statement(sql_two).unwrap();

    // Assert
    let (_, first_statement) = match first.statement {
        QueryStatement::Select(statement) => ((), statement),
        _ => panic!("expected select statement"),
    };
    let (_, second_statement) = match second.statement {
        QueryStatement::Select(statement) => ((), statement),
        _ => panic!("expected select statement"),
    };

    assert_eq!(
        format!("{:?}", first_statement),
        format!("{:?}", second_statement)
    );
}

#[test]
fn should_parse_with_clause_with_cte_source() {
    // Arrange
    let sql = "WITH docs_cte AS (SELECT title FROM docs) SELECT title FROM docs_cte";

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    let QueryStatement::Select(statement) = parsed.statement else {
        panic!("expected select statement");
    };
    assert_eq!(statement.ctes.len(), 1);
    assert_eq!(statement.ctes[0].name, "docs_cte");
    assert!(matches!(statement.ctes[0].query, CteQuery::Simple(_)));
    assert_eq!(
        statement.source,
        QuerySource::Collection("docs_cte".to_string())
    );
}

#[test]
fn should_parse_multiple_ctes_with_dependencies() {
    // Arrange
    let sql = "WITH first AS (SELECT title FROM docs), second AS (SELECT title FROM first) SELECT title FROM second";

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    let QueryStatement::Select(statement) = parsed.statement else {
        panic!("expected select statement");
    };
    assert_eq!(statement.ctes.len(), 2);
    assert_eq!(statement.ctes[0].name, "first");
    assert_eq!(statement.ctes[1].name, "second");
    assert_eq!(
        statement.source,
        QuerySource::Collection("second".to_string())
    );
}

#[test]
fn should_parse_recursive_cte_shape() {
    // Arrange
    let sql = "with recursive counter(n) as (SELECT n FROM docs UNION ALL SELECT n FROM counter) SELECT n FROM counter";

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    let QueryStatement::Select(statement) = parsed.statement else {
        panic!("expected select statement");
    };
    assert!(statement.recursive);
    assert_eq!(statement.ctes.len(), 1);
    assert_eq!(statement.ctes[0].aliases, vec!["n".to_string()]);
    assert!(matches!(
        statement.ctes[0].query,
        CteQuery::Recursive { .. }
    ));
}

#[test]
fn should_parse_cte_column_aliases() {
    // Arrange
    let sql =
        "WITH docs_cte(title_alias) AS (SELECT title FROM docs) SELECT title_alias FROM docs_cte";

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    let QueryStatement::Select(statement) = parsed.statement else {
        panic!("expected select statement");
    };
    assert_eq!(statement.ctes[0].aliases.len(), 1);
    assert_eq!(statement.ctes[0].aliases[0], "title_alias");
    assert_eq!(
        statement.source,
        QuerySource::Collection("docs_cte".to_string())
    );
}

#[test]
fn should_parse_create_table_with_if_not_exists() {
    // Arrange
    let sql =
        "CREATE TABLE IF NOT EXISTS users (id INT, title TEXT, embedding VECTOR(3), flag BOOLEAN)";

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    let QueryStatement::CreateTable(statement) = parsed.statement else {
        panic!("expected create table statement");
    };

    assert_eq!(statement.table, "users");
    assert!(statement.if_not_exists);
    assert_eq!(statement.fields.len(), 4);
    assert_eq!(statement.fields[1].name, "title");
    assert_eq!(statement.fields[1].data_type, DataType::Text);
}

#[test]
fn should_parse_drop_table_with_if_exists() {
    // Arrange
    let sql = "DROP TABLE IF EXISTS users";

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    let QueryStatement::DropTable(statement) = parsed.statement else {
        panic!("expected drop table statement");
    };

    assert_eq!(statement.table, "users");
    assert!(statement.if_exists);
}

#[test]
fn should_parse_create_schema_if_not_exists() {
    // Arrange
    let sql = "CREATE SCHEMA IF NOT EXISTS reporting";

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    let QueryStatement::CreateSchema(statement) = parsed.statement else {
        panic!("expected create schema statement");
    };

    assert_eq!(statement.schema, "reporting");
    assert!(statement.if_not_exists);
}

#[test]
fn should_reject_create_schema_when_schema_exists_without_if_not_exists() {
    // Arrange
    let cassie =
        Cassie::new_with_data_dir(format!("/tmp/cassie-parser-schema-{}", Uuid::new_v4())).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        cassie
            .catalog
            .register_namespace("reporting", None)
            .await;

        // Act
        let parsed = parse_statement("CREATE SCHEMA reporting")
            .expect("parse should succeed");
        let bound = cassie::sql::binder::bind(parsed, &cassie.catalog).await;

        // Assert
        assert!(matches!(bound, Err(err) if err.to_string().contains("namespace 'reporting' already exists")));
    });
}

#[test]
fn should_parse_rename_table_alter_statement() {
    // Arrange
    let sql = "ALTER TABLE docs RENAME TO docs_archive";

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    let QueryStatement::AlterTable(statement) = parsed.statement else {
        panic!("expected alter table statement");
    };

    assert_eq!(statement.table, "docs");
    match statement.operation {
        cassie::sql::ast::AlterTableOperation::RenameTo { table } => {
            assert_eq!(table, "docs_archive");
        }
        _ => panic!("expected rename operation"),
    }
}

#[test]
fn should_reject_duplicate_fields_in_create_table_definition() {
    // Arrange
    let sql = "CREATE TABLE dup_cols (id TEXT, id TEXT)";

    // Act
    let parsed = parse_statement(sql);

    // Assert
    assert!(parsed.is_err());
}

#[test]
fn should_parse_create_table_field_constraints() {
    // Arrange
    let sql =
        "CREATE TABLE users (id INT PRIMARY KEY, email TEXT NOT NULL UNIQUE DEFAULT 'anon', score INT CHECK (score >= 0))";

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    let QueryStatement::CreateTable(statement) = parsed.statement else {
        panic!("expected create table statement");
    };
    assert_eq!(statement.table, "users");
    assert_eq!(statement.fields.len(), 3);

    assert!(statement.fields[0]
        .constraints
        .iter()
        .any(|c| c.primary_key));
    assert!(!statement.fields[0].constraints.iter().any(|c| c.unique));

    let email_constraints = &statement.fields[1].constraints;
    assert_eq!(email_constraints.len(), 1);
    assert!(email_constraints[0].not_null);
    assert!(email_constraints[0].unique);
    assert_eq!(
        email_constraints[0].default_value,
        Some(serde_json::Value::String("anon".to_string()))
    );

    let score_constraints = &statement.fields[2].constraints;
    assert_eq!(score_constraints.len(), 1);
    assert!(score_constraints[0].check.is_some());
}

#[test]
fn should_reject_create_table_constraints_without_parentheses() {
    // Arrange
    let sql = "CREATE TABLE broken (id INT CHECK score >= 0)";

    // Act
    let parsed = parse_statement(sql);

    // Assert
    assert!(parsed.is_err());
}

#[test]
fn should_parse_create_index_statement() {
    // Arrange
    let sql = "CREATE UNIQUE INDEX idx_users_email ON users USING btree (email) WITH (fillfactor = 90, case_sensitive = 'false')";

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    let QueryStatement::CreateIndex(statement) = parsed.statement else {
        panic!("expected create index statement");
    };

    assert_eq!(statement.name, "idx_users_email");
    assert_eq!(statement.table, "users");
    assert_eq!(statement.field, "email");
    assert!(statement.unique);
    assert!(matches!(statement.kind, cassie::catalog::IndexKind::Scalar));
    assert_eq!(statement.options.get("fillfactor"), Some(&"90".to_string()));
    assert_eq!(
        statement.options.get("case_sensitive"),
        Some(&"false".to_string())
    );
}

#[test]
fn should_parse_vector_create_index_statement() {
    // Arrange
    let sql = "CREATE INDEX idx_docs_embedding ON docs USING vector (embedding) WITH (source_field = content, metric = 'l2')";

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    let QueryStatement::CreateIndex(statement) = parsed.statement else {
        panic!("expected create index statement");
    };

    assert_eq!(statement.name, "idx_docs_embedding");
    assert_eq!(statement.table, "docs");
    assert_eq!(statement.field, "embedding");
    assert!(!statement.unique);
    assert!(matches!(statement.kind, IndexKind::Vector));
    assert_eq!(
        statement.options.get("source_field"),
        Some(&"content".to_string())
    );
    assert_eq!(statement.options.get("metric"), Some(&"l2".to_string()));
}

#[test]
fn should_parse_fulltext_create_index_statement_with_options() {
    // Arrange
    let cassie = Cassie::new_with_data_dir(format!(
        "/tmp/cassie-fulltext-index-options-{}",
        Uuid::new_v4()
    ))
    .unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        cassie
            .register_collection(
                "ft_docs_options".to_string(),
                Schema {
                    fields: vec![
                        FieldSchema {
                            name: "id".to_string(),
                            data_type: DataType::Text,
                            nullable: true,
                        },
                        FieldSchema {
                            name: "body".to_string(),
                            data_type: DataType::Text,
                            nullable: true,
                        },
                    ],
                },
            )
            .await;

        // Act
        let parsed = parse_statement(
            "CREATE INDEX idx_ft_docs_body ON ft_docs_options USING fulltext (body) WITH (boost = 2.5, k1 = 0.8, b = 0.1)",
        )
        .expect("parse should succeed");
        let bound = cassie::sql::binder::bind(parsed, &cassie.catalog)
            .await
            .expect("bind should succeed");

        // Assert
        let QueryStatement::CreateIndex(statement) = bound.statement.statement else {
            panic!("expected create index statement");
        };
        assert!(matches!(statement.kind, IndexKind::FullText));
        assert_eq!(statement.options.get("boost"), Some(&"2.5".to_string()));
        assert_eq!(statement.options.get("k1"), Some(&"0.8".to_string()));
        assert_eq!(statement.options.get("b"), Some(&"0.1".to_string()));
    });
}

#[test]
fn should_apply_fulltext_create_index_defaults() {
    // Arrange
    let cassie = Cassie::new_with_data_dir(format!(
        "/tmp/cassie-fulltext-index-defaults-{}",
        Uuid::new_v4()
    ))
    .unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        cassie
            .register_collection(
                "ft_docs_defaults".to_string(),
                Schema {
                    fields: vec![
                        FieldSchema {
                            name: "id".to_string(),
                            data_type: DataType::Text,
                            nullable: true,
                        },
                        FieldSchema {
                            name: "body".to_string(),
                            data_type: DataType::Text,
                            nullable: true,
                        },
                    ],
                },
            )
            .await;

        // Act
        let parsed = parse_statement(
            "CREATE INDEX idx_ft_docs_defaults ON ft_docs_defaults USING fulltext (body)",
        )
        .expect("parse should succeed");
        let bound = cassie::sql::binder::bind(parsed, &cassie.catalog)
            .await
            .expect("bind should succeed");

        // Assert
        let QueryStatement::CreateIndex(statement) = bound.statement.statement else {
            panic!("expected create index statement");
        };
        assert_eq!(statement.options.get("boost"), Some(&"1".to_string()));
        assert_eq!(statement.options.get("k1"), Some(&"1.2".to_string()));
        assert_eq!(statement.options.get("b"), Some(&"0.75".to_string()));
    });
}

#[test]
fn should_reject_fulltext_create_index_with_non_finite_boost() {
    // Arrange
    let cassie = Cassie::new_with_data_dir(format!(
        "/tmp/cassie-fulltext-index-non-finite-{}",
        Uuid::new_v4()
    ))
    .unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        cassie
            .register_collection(
                "ft_docs_non_finite".to_string(),
                Schema {
                    fields: vec![
                        FieldSchema {
                            name: "id".to_string(),
                            data_type: DataType::Text,
                            nullable: true,
                        },
                        FieldSchema {
                            name: "body".to_string(),
                            data_type: DataType::Text,
                            nullable: true,
                        },
                    ],
                },
            )
            .await;

        // Act
        let parsed = parse_statement(
            "CREATE INDEX idx_ft_docs_non_finite ON ft_docs_non_finite USING fulltext (body) WITH (boost = inf)",
        )
        .expect("parse should succeed");
        let bound = cassie::sql::binder::bind(parsed, &cassie.catalog).await;

        // Assert
        assert!(bound.is_err());
    });
}

#[test]
fn should_reject_duplicate_fulltext_index_on_same_field() {
    // Arrange
    let cassie = Cassie::new_with_data_dir(format!(
        "/tmp/cassie-fulltext-index-duplicate-{}",
        Uuid::new_v4()
    ))
    .unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        cassie
            .register_collection(
                "ft_docs_duplicate".to_string(),
                Schema {
                    fields: vec![
                        FieldSchema {
                            name: "id".to_string(),
                            data_type: DataType::Text,
                            nullable: true,
                        },
                        FieldSchema {
                            name: "body".to_string(),
                            data_type: DataType::Text,
                            nullable: true,
                        },
                    ],
                },
            )
            .await;

        cassie
            .catalog
            .register_index(IndexMeta {
                collection: "ft_docs_duplicate".to_string(),
                name: "idx_ft_docs_duplicate_primary".to_string(),
                field: "body".to_string(),
                kind: IndexKind::FullText,
                unique: false,
                options: BTreeMap::from_iter(vec![
                    ("boost".to_string(), "1.0".to_string()),
                    ("k1".to_string(), "1.2".to_string()),
                    ("b".to_string(), "0.75".to_string()),
                ]),
            })
            .await;

        // Act
        let parsed = parse_statement(
            "CREATE INDEX idx_ft_docs_duplicate_secondary ON ft_docs_duplicate USING fulltext (body)",
        )
        .expect("parse should succeed");
        let bound = cassie::sql::binder::bind(parsed, &cassie.catalog).await;

        // Assert
        assert!(bound.is_err());
    });
}

#[test]
fn should_reject_fulltext_index_on_non_text_field() {
    // Arrange
    let cassie = Cassie::new_with_data_dir(format!(
        "/tmp/cassie-fulltext-index-non-text-{}",
        Uuid::new_v4()
    ))
    .unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        cassie
            .register_collection(
                "ft_docs_bad_field".to_string(),
                Schema {
                    fields: vec![FieldSchema {
                        name: "score".to_string(),
                        data_type: DataType::Int,
                        nullable: true,
                    }],
                },
            )
            .await;

        // Act
        let parsed = parse_statement(
            "CREATE INDEX idx_ft_docs_bad_field ON ft_docs_bad_field USING fulltext (score)",
        )
        .expect("parse should succeed");
        let bound = cassie::sql::binder::bind(parsed, &cassie.catalog).await;

        // Assert
        assert!(bound.is_err());
    });
}

#[test]
fn should_reject_fulltext_create_index_with_unsupported_option() {
    // Arrange
    let cassie = Cassie::new_with_data_dir(format!(
        "/tmp/cassie-fulltext-index-unsupported-option-{}",
        Uuid::new_v4()
    ))
    .unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        cassie
            .register_collection(
                "ft_docs_unsupported".to_string(),
                Schema {
                    fields: vec![
                        FieldSchema {
                            name: "id".to_string(),
                            data_type: DataType::Text,
                            nullable: true,
                        },
                        FieldSchema {
                            name: "body".to_string(),
                            data_type: DataType::Text,
                            nullable: true,
                        },
                    ],
                },
            )
            .await;

        // Act
        let parsed = parse_statement(
            "CREATE INDEX idx_ft_docs_unsupported ON ft_docs_unsupported USING fulltext (body) WITH (alpha = 0.5)",
        )
        .expect("parse should succeed");
        let bound = cassie::sql::binder::bind(parsed, &cassie.catalog).await;

        // Assert
        assert!(bound.is_err());
    });
}

#[test]
fn should_reject_fulltext_create_index_with_invalid_fulltext_k1() {
    // Arrange
    let cassie = Cassie::new_with_data_dir(format!(
        "/tmp/cassie-fulltext-index-bad-k1-{}",
        Uuid::new_v4()
    ))
    .unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        cassie
            .register_collection(
                "ft_docs_bad_k1".to_string(),
                Schema {
                    fields: vec![
                        FieldSchema {
                            name: "id".to_string(),
                            data_type: DataType::Text,
                            nullable: true,
                        },
                        FieldSchema {
                            name: "body".to_string(),
                            data_type: DataType::Text,
                            nullable: true,
                        },
                    ],
                },
            )
            .await;

        // Act
        let parsed = parse_statement(
            "CREATE INDEX idx_ft_docs_bad_k1 ON ft_docs_bad_k1 USING fulltext (body) WITH (k1 = -1)",
        )
        .expect("parse should succeed");
        let bound = cassie::sql::binder::bind(parsed, &cassie.catalog).await;

        // Assert
        assert!(bound.is_err());
    });
}

#[test]
fn should_reject_vector_create_index_without_source_field() {
    // Arrange
    let cassie = Cassie::new_with_data_dir(format!(
        "/tmp/cassie-vector-index-no-source-{}",
        Uuid::new_v4()
    ))
    .unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        cassie
            .register_collection(
                "vec_docs_no_source".to_string(),
                Schema {
                    fields: vec![
                        FieldSchema {
                            name: "id".to_string(),
                            data_type: DataType::Text,
                            nullable: true,
                        },
                        FieldSchema {
                            name: "content".to_string(),
                            data_type: DataType::Text,
                            nullable: true,
                        },
                        FieldSchema {
                            name: "embedding".to_string(),
                            data_type: DataType::Vector(3),
                            nullable: true,
                        },
                    ],
                },
            )
            .await;

        // Act
        let parsed = parse_statement(
            "CREATE INDEX idx_docs_embedding ON vec_docs_no_source USING vector (embedding)",
        )
        .expect("parse should succeed");
        let bound = cassie::sql::binder::bind(parsed, &cassie.catalog).await;

        // Assert
        assert!(bound.is_err());
    });
}

#[test]
fn should_reject_vector_create_index_with_invalid_metric() {
    // Arrange
    let cassie = Cassie::new_with_data_dir(format!(
        "/tmp/cassie-vector-index-bad-metric-{}",
        Uuid::new_v4()
    ))
    .unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        cassie
            .register_collection(
                "vec_docs_invalid_metric".to_string(),
                Schema {
                    fields: vec![
                        FieldSchema {
                            name: "content".to_string(),
                            data_type: DataType::Text,
                            nullable: true,
                        },
                        FieldSchema {
                            name: "embedding".to_string(),
                            data_type: DataType::Vector(3),
                            nullable: true,
                        },
                    ],
                },
            )
            .await;

        // Act
        let parsed = parse_statement(
            "CREATE INDEX idx_docs_embedding ON vec_docs_invalid_metric USING vector (embedding) WITH (source_field = content, metric = 'unsupported')",
        )
        .expect("parse should succeed");
        let bound = cassie::sql::binder::bind(parsed, &cassie.catalog).await;

        // Assert
        assert!(bound.is_err());
    });
}

#[test]
fn should_reject_vector_create_index_on_non_vector_field() {
    // Arrange
    let cassie = Cassie::new_with_data_dir(format!(
        "/tmp/cassie-vector-index-non-vector-{}",
        Uuid::new_v4()
    ))
    .unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        cassie
            .register_collection(
                "vec_docs_not_vector".to_string(),
                Schema {
                    fields: vec![
                        FieldSchema {
                            name: "content".to_string(),
                            data_type: DataType::Text,
                            nullable: true,
                        },
                    ],
                },
            )
            .await;

        // Act
        let parsed = parse_statement(
            "CREATE INDEX idx_docs_embedding ON vec_docs_not_vector USING vector (content) WITH (source_field = content)",
        )
        .expect("parse should succeed");
        let bound = cassie::sql::binder::bind(parsed, &cassie.catalog).await;

        // Assert
        assert!(bound.is_err());
    });
}

#[test]
fn should_default_vector_metric_to_cosine() {
    // Arrange
    let cassie = Cassie::new_with_data_dir(format!(
        "/tmp/cassie-vector-index-default-metric-{}",
        Uuid::new_v4()
    ))
    .unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        cassie
            .register_collection(
                "vec_docs_default_metric".to_string(),
                Schema {
                    fields: vec![
                        FieldSchema {
                            name: "content".to_string(),
                            data_type: DataType::Text,
                            nullable: true,
                        },
                        FieldSchema {
                            name: "embedding".to_string(),
                            data_type: DataType::Vector(3),
                            nullable: true,
                        },
                    ],
                },
            )
            .await;

        // Act
        let parsed = parse_statement(
            "CREATE INDEX idx_docs_embedding ON vec_docs_default_metric USING vector (embedding) WITH (source_field = content)",
        )
        .expect("parse should succeed");
        let bound = cassie::sql::binder::bind(parsed, &cassie.catalog)
            .await
            .expect("bind should succeed");

        // Assert
        let QueryStatement::CreateIndex(statement) = bound.statement.statement else {
            panic!("expected create index statement");
        };
        assert_eq!(statement.options.get("metric"), Some(&"cosine".to_string()));
    });
}

#[test]
fn should_parse_drop_index_statement() {
    // Arrange
    let sql = "DROP INDEX IF EXISTS idx_users_email ON users";

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    let QueryStatement::DropIndex(statement) = parsed.statement else {
        panic!("expected drop index statement");
    };

    assert_eq!(statement.name, "idx_users_email");
    assert_eq!(statement.table, "users");
    assert!(statement.if_exists);
}

#[test]
fn should_reject_create_table_when_collection_exists_without_if_not_exists() {
    // Arrange
    let cassie =
        Cassie::new_with_data_dir(format!("/tmp/cassie-parser-{}", Uuid::new_v4())).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        cassie
            .register_collection(
                "existing_table".to_string(),
                Schema {
                    fields: vec![FieldSchema {
                        name: "id".to_string(),
                        data_type: DataType::Text,
                        nullable: true,
                    }],
                },
            )
            .await;

        // Act
        let parsed = parse_statement("CREATE TABLE existing_table (title TEXT)")
            .expect("parse should succeed");
        let bound = cassie::sql::binder::bind(parsed, &cassie.catalog).await;

        // Assert
        assert!(bound.is_err());
    });
}

#[test]
fn should_reject_drop_table_when_collection_missing_without_if_exists() {
    // Arrange
    let cassie =
        Cassie::new_with_data_dir(format!("/tmp/cassie-parser-{}", Uuid::new_v4())).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        // Act
        let parsed = parse_statement("DROP TABLE missing_table").expect("parse should succeed");
        let bound = cassie::sql::binder::bind(parsed, &cassie.catalog).await;

        // Assert
        assert!(bound.is_err());
    });
}
