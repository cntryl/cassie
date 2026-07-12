#![allow(unused_imports)]

use cassie::app::Cassie;
use cassie::catalog::{IndexKind, IndexMeta};
use cassie::sql::ast::{
    BinaryOp, CteQuery, Expr, InsertSource, JoinKind, QuerySource, QueryStatement, SelectItem,
    SetOperator, SortDirection,
};
use cassie::sql::parse_statement;
use cassie::types::{DataType, FieldSchema, Schema};
use std::collections::BTreeMap;
use uuid::Uuid;

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
fn should_parse_table_free_literal_parameter_projection() {
    // Arrange
    let sql = "SELECT 1 AS one, NULL AS missing, $1::INT AS value";

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    let QueryStatement::Select(statement) = parsed.statement else {
        panic!("expected select statement");
    };
    assert!(matches!(statement.source, QuerySource::SingleRow));
    assert!(matches!(statement.projection[0], SelectItem::Expr { .. }));
    assert!(matches!(statement.projection[1], SelectItem::Expr { .. }));
    assert!(matches!(statement.projection[2], SelectItem::Expr { .. }));
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
fn should_parse_not_exists_predicate() {
    // Arrange
    let sql = "SELECT title FROM docs WHERE NOT EXISTS (SELECT title FROM archived_docs)";

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    let QueryStatement::Select(statement) = parsed.statement else {
        panic!("expected select statement");
    };
    assert!(matches!(
        statement.filter,
        Some(Expr::Not { ref expr }) if matches!(expr.as_ref(), Expr::Exists(_))
    ));
}

#[test]
fn should_reject_not_without_expression() {
    // Arrange
    let sql = "SELECT title FROM docs WHERE NOT";

    // Act
    let error = parse_statement(sql).expect_err("NOT without an expression should fail");

    // Assert
    assert_eq!(error.kind(), cassie::sql::SqlErrorKind::Syntax);
    assert!(error.message().contains("NOT requires an expression"));
}

#[test]
fn should_reject_empty_in_list_predicate() {
    // Arrange
    let sql = "SELECT title FROM docs WHERE title IN ()";

    // Act
    let error = parse_statement(sql).expect_err("empty IN predicate should fail");

    // Assert
    assert_eq!(error.kind(), cassie::sql::SqlErrorKind::Syntax);
    assert!(error.message().contains("IN predicate"));
}

#[test]
fn should_reject_exists_without_select_subquery() {
    // Arrange
    let sql = "SELECT title FROM docs WHERE EXISTS (INSERT INTO docs (title) VALUES ('x'))";

    // Act
    let error = parse_statement(sql).expect_err("EXISTS without SELECT subquery should fail");

    // Assert
    assert_eq!(error.kind(), cassie::sql::SqlErrorKind::Syntax);
    assert!(error.message().contains("EXISTS requires"));
}
