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
fn should_parse_inner_join_source() {
    // Arrange
    let sql = "SELECT users.name FROM users JOIN orders ON users.id = orders.user_id";

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    let QueryStatement::Select(statement) = parsed.statement else {
        panic!("expected select statement");
    };
    assert!(matches!(
        statement.source,
        QuerySource::Join {
            kind: JoinKind::Inner,
            ..
        }
    ));
}

#[test]
fn should_parse_chained_joins_as_left_associative_sources() {
    // Arrange
    let sql = "SELECT users.name FROM users JOIN orders ON users.id = orders.user_id JOIN regions ON orders.region_id = regions.id";

    // Act
    let parsed = parse_statement(sql).expect("join chain should parse");

    // Assert
    let QueryStatement::Select(statement) = parsed.statement else {
        panic!("expected select statement");
    };
    let QuerySource::Join {
        left, right, kind, ..
    } = statement.source
    else {
        panic!("expected outer join source");
    };
    assert_eq!(kind, JoinKind::Inner);
    assert!(matches!(*right, QuerySource::Collection(ref name) if name == "regions"));
    assert!(matches!(
        *left,
        QuerySource::Join {
            kind: JoinKind::Inner,
            ..
        }
    ));
}

#[test]
fn should_parse_left_join_source() {
    // Arrange
    let sql = "SELECT users.name FROM users LEFT JOIN orders ON users.id = orders.user_id";

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    let QueryStatement::Select(statement) = parsed.statement else {
        panic!("expected select statement");
    };
    assert!(matches!(
        statement.source,
        QuerySource::Join {
            kind: JoinKind::Left,
            ..
        }
    ));
}

#[test]
fn should_parse_right_join_source() {
    // Arrange
    let sql = "SELECT users.name FROM users RIGHT JOIN orders ON users.id = orders.user_id";

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    let QueryStatement::Select(statement) = parsed.statement else {
        panic!("expected select statement");
    };
    assert!(matches!(
        statement.source,
        QuerySource::Join {
            kind: JoinKind::Right,
            ..
        }
    ));
}

#[test]
fn should_parse_full_outer_join_source() {
    // Arrange
    let sql = "SELECT users.name FROM users FULL JOIN orders ON users.key = orders.user_key";

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    let QueryStatement::Select(statement) = parsed.statement else {
        panic!("expected select statement");
    };
    assert!(matches!(
        statement.source,
        QuerySource::Join {
            kind: JoinKind::Full,
            ..
        }
    ));
}

#[test]
fn should_parse_cross_join_source() {
    // Arrange
    let sql = "SELECT users.name FROM users CROSS JOIN orders";

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    let QueryStatement::Select(statement) = parsed.statement else {
        panic!("expected select statement");
    };
    assert!(matches!(
        statement.source,
        QuerySource::Join {
            kind: JoinKind::Cross,
            ..
        }
    ));
}

#[test]
fn should_reject_right_join_without_on_predicate() {
    // Arrange
    let sql = "SELECT users.name FROM users RIGHT JOIN orders";

    // Act
    let error = parse_statement(sql).expect_err("RIGHT JOIN without ON should fail");

    // Assert
    assert_eq!(error.kind(), cassie::sql::SqlErrorKind::Syntax);
    assert!(error.message().contains("JOIN requires ON"));
}

#[test]
fn should_reject_cross_join_with_on_predicate() {
    // Arrange
    let sql = "SELECT users.name FROM users CROSS JOIN orders ON users.id = orders.user_id";

    // Act
    let error = parse_statement(sql).expect_err("CROSS JOIN with ON should fail");

    // Assert
    assert_eq!(error.kind(), cassie::sql::SqlErrorKind::Unsupported);
    assert!(error.message().contains("unsupported FROM syntax"));
}

#[test]
fn should_parse_from_subquery_source() {
    // Arrange
    let sql = "SELECT recent.title FROM (SELECT title FROM docs) AS recent";

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    let QueryStatement::Select(statement) = parsed.statement else {
        panic!("expected select statement");
    };
    assert!(matches!(
        statement.source,
        QuerySource::Subquery { ref alias, .. } if alias == "recent"
    ));
}

#[test]
fn should_parse_lateral_join_source() {
    // Arrange
    let sql = "SELECT users.name FROM users JOIN LATERAL (SELECT user_key FROM orders) AS recent ON users.key = recent.user_key";

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    let QueryStatement::Select(statement) = parsed.statement else {
        panic!("expected select statement");
    };
    assert!(matches!(
        statement.source,
        QuerySource::Join { ref right, .. } if matches!(right.as_ref(), QuerySource::Subquery { ref alias, lateral: true, .. } if alias == "recent")
    ));
}

#[test]
fn should_parse_cross_apply_join_source() {
    // Arrange
    let sql = "SELECT users.name FROM users CROSS APPLY (SELECT total FROM orders) AS recent";

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    let QueryStatement::Select(statement) = parsed.statement else {
        panic!("expected select statement");
    };
    assert!(matches!(
        statement.source,
        QuerySource::Join { kind: JoinKind::Cross, ref right, .. }
            if matches!(right.as_ref(), QuerySource::Subquery { lateral: true, .. })
    ));
}

#[test]
fn should_parse_outer_apply_join_source() {
    // Arrange
    let sql = "SELECT users.name FROM users OUTER APPLY (SELECT total FROM orders) AS recent";

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    let QueryStatement::Select(statement) = parsed.statement else {
        panic!("expected select statement");
    };
    assert!(matches!(
        statement.source,
        QuerySource::Join { kind: JoinKind::Left, ref right, .. }
            if matches!(right.as_ref(), QuerySource::Subquery { lateral: true, .. })
    ));
}

#[test]
fn should_parse_distinct_select() {
    // Arrange
    let sql = "SELECT DISTINCT category FROM docs";

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    let QueryStatement::Select(statement) = parsed.statement else {
        panic!("expected select statement");
    };
    assert!(statement.distinct);
}

#[test]
fn should_parse_distinct_on_select() {
    // Arrange
    let sql =
        "SELECT DISTINCT ON (tenant_id) tenant_id, title FROM docs ORDER BY tenant_id, score DESC";

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    let QueryStatement::Select(statement) = parsed.statement else {
        panic!("expected select statement");
    };
    assert!(!statement.distinct);
    assert_eq!(statement.distinct_on.len(), 1);
    assert!(matches!(&statement.distinct_on[0], Expr::Column(name) if name == "tenant_id"));
}

#[test]
fn should_parse_group_by_with_having() {
    // Arrange
    let sql = "SELECT category, COUNT(*) AS total FROM docs GROUP BY category HAVING COUNT(*) > 1";

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    let QueryStatement::Select(statement) = parsed.statement else {
        panic!("expected select statement");
    };
    assert_eq!(statement.group_by.len(), 1);
    assert!(statement.having.is_some());
}

#[test]
fn should_parse_union_all_select() {
    // Arrange
    let sql = "SELECT title FROM left_docs UNION ALL SELECT title FROM right_docs";

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    let QueryStatement::Select(statement) = parsed.statement else {
        panic!("expected select statement");
    };
    let set = statement.set.expect("set clause should exist");
    assert!(matches!(set.operator, SetOperator::UnionAll));
}

#[test]
fn should_parse_union_select() {
    // Arrange
    let sql = "SELECT title FROM left_docs UNION SELECT title FROM right_docs";

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    let QueryStatement::Select(statement) = parsed.statement else {
        panic!("expected select statement");
    };
    let set = statement.set.expect("expected set operation");
    assert!(matches!(set.operator, SetOperator::Union));
}

#[test]
fn should_parse_intersect_select() {
    // Arrange
    let sql = "SELECT title FROM left_docs INTERSECT SELECT title FROM right_docs";

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    let QueryStatement::Select(statement) = parsed.statement else {
        panic!("expected select statement");
    };
    let set = statement.set.expect("set clause should exist");
    assert!(matches!(set.operator, SetOperator::Intersect));
}

#[test]
fn should_parse_except_select() {
    // Arrange
    let sql = "SELECT title FROM left_docs EXCEPT SELECT title FROM right_docs";

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    let QueryStatement::Select(statement) = parsed.statement else {
        panic!("expected select statement");
    };
    let set = statement.set.expect("set clause should exist");
    assert!(matches!(set.operator, SetOperator::Except));
}

#[test]
fn should_parse_global_order_limit_after_set_operation() {
    // Arrange
    let sql = "SELECT title FROM left_docs UNION ALL SELECT title FROM right_docs ORDER BY title LIMIT 1 OFFSET 1";

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    let QueryStatement::Select(statement) = parsed.statement else {
        panic!("expected select statement");
    };
    let set = statement.set.expect("set clause should exist");
    assert!(matches!(set.operator, SetOperator::UnionAll));
    assert!(set.right.order.is_empty());
    assert_eq!(statement.order.len(), 1);
    assert_eq!(statement.limit, Some(1));
    assert_eq!(statement.offset, Some(1));
}

#[test]
fn should_parse_chained_set_operation() {
    // Arrange
    let sql = "SELECT title FROM first_docs UNION ALL SELECT title FROM second_docs UNION ALL SELECT title FROM third_docs";

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    let QueryStatement::Select(statement) = parsed.statement else {
        panic!("expected select statement");
    };
    let set = statement.set.expect("set operation expected");
    assert!(set.right.set.is_some());
}

#[test]
fn should_parse_row_number_window_function_query() {
    // Arrange
    let sql =
        "SELECT row_number() OVER (PARTITION BY category ORDER BY score DESC) AS rank FROM docs";

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    let QueryStatement::Select(statement) = parsed.statement else {
        panic!("expected select statement");
    };
    assert!(matches!(
        statement.projection.as_slice(),
        [SelectItem::WindowFunction { alias: Some(alias), .. }] if alias == "rank"
    ));
}

#[test]
fn should_parse_value_window_function_query() {
    // Arrange
    let sql = "SELECT lag(title) OVER (PARTITION BY category ORDER BY score DESC) AS previous_title FROM docs";

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    let QueryStatement::Select(statement) = parsed.statement else {
        panic!("expected select statement");
    };
    assert!(matches!(
        statement.projection.as_slice(),
        [SelectItem::WindowFunction { alias: Some(alias), .. }] if alias == "previous_title"
    ));
}

#[test]
fn should_reject_unsupported_grouping_sets_query() {
    // Arrange
    let sql = "SELECT category, COUNT(*) FROM docs GROUP BY GROUPING SETS (category)";

    // Act
    let error = parse_statement(sql).expect_err("GROUPING SETS should fail");

    // Assert
    assert_eq!(error.kind(), cassie::sql::SqlErrorKind::Unsupported);
    assert!(error.message().contains("GROUP BY"));
}
