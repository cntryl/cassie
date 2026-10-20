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
    assert!(matches!(statement.limit, Some(Expr::IntegerLiteral(5))));
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
fn should_parse_negative_offset_before_execution() {
    // Arrange
    let sql = "SELECT * FROM docs ORDER BY id OFFSET -1";

    // Act
    let parsed = parse_statement(sql);

    // Assert
    assert!(parsed.is_ok());
}

#[test]
fn should_parse_negative_limit_before_execution() {
    // Arrange
    let sql = "SELECT * FROM docs LIMIT -5";

    // Act
    let parsed = parse_statement(sql);

    // Assert
    assert!(parsed.is_ok());
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

fn select_filter(sql: &str) -> Expr {
    let QueryStatement::Select(statement) = parse_statement(sql)
        .expect("parse should succeed")
        .statement
    else {
        panic!("expected select statement");
    };
    statement.filter.expect("filter should exist")
}

fn is_between(expr: &Expr, expected_negated: bool) -> bool {
    matches!(expr, Expr::Between { negated, .. } if *negated == expected_negated)
}

#[test]
fn should_bind_between_tighter_than_a_following_conjunct() {
    // Arrange
    let sql = "SELECT a FROM t WHERE a BETWEEN 1 AND 3 AND b = 'v1'";

    // Act
    let filter = select_filter(sql);

    // Assert
    let Expr::Binary {
        left,
        op: BinaryOp::And,
        right,
    } = filter
    else {
        panic!("expected AND at the top of the filter");
    };
    assert!(is_between(&left, false));
    assert!(matches!(
        *right,
        Expr::Binary {
            op: BinaryOp::Eq,
            ..
        }
    ));
}

#[test]
fn should_bind_between_tighter_than_a_preceding_conjunct() {
    // Arrange
    let sql = "SELECT a FROM t WHERE b = 'v1' AND a BETWEEN 1 AND 3";

    // Act
    let filter = select_filter(sql);

    // Assert
    let Expr::Binary {
        left,
        op: BinaryOp::And,
        right,
    } = filter
    else {
        panic!("expected AND at the top of the filter");
    };
    assert!(matches!(
        *left,
        Expr::Binary {
            op: BinaryOp::Eq,
            ..
        }
    ));
    assert!(is_between(&right, false));
}

#[test]
fn should_split_conjunction_chains_mixing_between_with_not_between() {
    // Arrange
    let sql = "SELECT a FROM t WHERE a BETWEEN 1 AND 3 AND b NOT BETWEEN 4 AND 6 AND c = 1";

    // Act
    let filter = select_filter(sql);

    // Assert
    let Expr::Binary {
        left,
        op: BinaryOp::And,
        right,
    } = filter
    else {
        panic!("expected AND at the top of the filter");
    };
    assert!(is_between(&left, false));
    let Expr::Binary {
        left: middle,
        op: BinaryOp::And,
        right: last,
    } = *right
    else {
        panic!("expected nested AND");
    };
    assert!(is_between(&middle, true));
    assert!(matches!(
        *last,
        Expr::Binary {
            op: BinaryOp::Eq,
            ..
        }
    ));
}

#[test]
fn should_ignore_nested_or_quoted_between_when_splitting_conjunctions() {
    // Arrange
    let sql = "SELECT a FROM t WHERE (b BETWEEN 4 AND 6 OR c = ' between x and ') AND CASE WHEN d BETWEEN 1 AND 2 THEN true ELSE false END AND a BETWEEN 1 AND 3";

    // Act
    let filter = select_filter(sql);

    // Assert
    let Expr::Binary {
        left,
        op: BinaryOp::And,
        right,
    } = filter
    else {
        panic!("expected AND at the top of the filter");
    };
    assert!(matches!(
        *left,
        Expr::Binary {
            op: BinaryOp::Or,
            ..
        }
    ));
    let Expr::Binary {
        left: middle,
        op: BinaryOp::And,
        right: last,
    } = *right
    else {
        panic!("expected nested AND");
    };
    assert!(matches!(*middle, Expr::Case { .. }));
    assert!(is_between(&last, false));
}

#[test]
fn should_bind_between_tighter_than_conjunctions_in_delete_filters() {
    // Arrange
    let sql = "DELETE FROM t WHERE a BETWEEN 1 AND 3 AND b = 'v1'";

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    let QueryStatement::Delete(statement) = parsed.statement else {
        panic!("expected delete statement");
    };
    let Some(Expr::Binary {
        left,
        op: BinaryOp::And,
        ..
    }) = statement.filter
    else {
        panic!("expected AND at the top of the filter");
    };
    assert!(is_between(&left, false));
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
fn should_bind_postfix_casts_before_arithmetic_given_mixed_operands_when_building_the_ast() {
    // Arrange
    let sql = "SELECT 1 + 2::INT8 * (3 + 4)::INT8";

    // Act
    let parsed = parse_statement(sql).expect("cast and arithmetic expression parses");

    // Assert
    let QueryStatement::Select(statement) = parsed.statement else {
        panic!("expected SELECT");
    };
    let SelectItem::Expr {
        expr:
            Expr::Binary {
                left,
                op: BinaryOp::Add,
                right,
            },
        ..
    } = &statement.projection[0]
    else {
        panic!("expected addition outside multiplication and operand casts");
    };
    assert!(matches!(**left, Expr::IntegerLiteral(1)));
    let Expr::Binary {
        left,
        op: BinaryOp::Mul,
        right,
    } = right.as_ref()
    else {
        panic!("expected multiplication of two cast operands");
    };
    let Expr::Cast {
        expr,
        data_type: DataType::BigInt,
        ..
    } = left.as_ref()
    else {
        panic!("expected INT8 cast on the first multiplication operand");
    };
    assert!(matches!(**expr, Expr::IntegerLiteral(2)));
    let Expr::Cast {
        expr,
        data_type: DataType::BigInt,
        ..
    } = right.as_ref()
    else {
        panic!("expected INT8 cast on the parenthesized multiplication operand");
    };
    assert!(matches!(
        expr.as_ref(),
        Expr::Binary {
            op: BinaryOp::Add,
            ..
        }
    ));
}

#[test]
fn should_nest_casts_in_source_order_given_a_cast_chain_when_building_the_ast() {
    // Arrange
    let sql = "SELECT coalesce('7'::TEXT::INT4::INT8, 0)";

    // Act
    let parsed = parse_statement(sql).expect("function argument contains a cast chain");

    // Assert
    let QueryStatement::Select(statement) = parsed.statement else {
        panic!("expected SELECT");
    };
    let SelectItem::Function { function, .. } = &statement.projection[0] else {
        panic!("expected COALESCE projection");
    };
    let Expr::Cast {
        expr,
        data_type: DataType::BigInt,
        ..
    } = &function.args[0]
    else {
        panic!("expected final INT8 cast");
    };
    let Expr::Cast {
        expr,
        data_type: DataType::Int,
        ..
    } = expr.as_ref()
    else {
        panic!("expected preceding INT4 cast");
    };
    let Expr::Cast {
        expr,
        data_type: DataType::Text,
        ..
    } = expr.as_ref()
    else {
        panic!("expected first TEXT cast");
    };
    assert!(matches!(expr.as_ref(), Expr::StringLiteral(value) if value == "7"));
}

#[test]
fn should_preserve_dollar_quoted_contents_given_operator_text_when_parsing_literal_and_parameter_targets(
) {
    // Arrange
    let sql = "SELECT $$2::int4, 'value' /* literal */$$, $tag_2$body $other$ inner$tag_2$, $té$多$té$, $1";

    // Act
    let parsed =
        parse_statement(sql).expect("dollar-quoted literals and parameter parse separately");

    // Assert
    let QueryStatement::Select(statement) = parsed.statement else {
        panic!("expected SELECT");
    };
    let SelectItem::Expr {
        expr: Expr::StringLiteral(value),
        ..
    } = &statement.projection[0]
    else {
        panic!("expected untagged dollar-quoted string");
    };
    assert_eq!(value, "2::int4, 'value' /* literal */");
    let SelectItem::Expr {
        expr: Expr::StringLiteral(value),
        ..
    } = &statement.projection[1]
    else {
        panic!("expected tagged dollar-quoted string");
    };
    assert_eq!(value, "body $other$ inner");
    let SelectItem::Expr {
        expr: Expr::StringLiteral(value),
        ..
    } = &statement.projection[2]
    else {
        panic!("expected dollar quote with a multibyte tag");
    };
    assert_eq!(value, "多");
    assert!(matches!(
        statement.projection[3],
        SelectItem::Expr {
            expr: Expr::Param(0),
            ..
        }
    ));
}

#[test]
fn should_reject_incomplete_dollar_quotes_given_unmatched_delimiters_when_parsing_a_literal() {
    // Arrange
    let statements = [
        "SELECT $$unterminated",
        "SELECT $tag$body$other$",
        "SELECT $tag$body$TAG$",
        "SELECT $2tag$body$2tag$",
        "SELECT $$first$$$$second$$",
    ];

    // Act
    let parsed = statements.map(parse_statement);

    // Assert
    for (sql, result) in statements.into_iter().zip(parsed) {
        assert!(result.is_err(), "invalid dollar quote must reject: {sql}");
    }
}

#[test]
fn should_reject_nonexistent_qualified_types_given_builtin_aliases_when_parsing_a_cast() {
    // Arrange
    let statements = [
        "SELECT '7'::pg_catalog.int",
        "SELECT '7'::pg_catalog.integer",
        "SELECT '7'::private.int4",
        "SELECT '7'::pg_catalog.missing_type",
    ];

    // Act
    let parsed = statements.map(parse_statement);

    // Assert
    for (sql, result) in statements.into_iter().zip(parsed) {
        assert!(result.is_err(), "type identity must not be invented: {sql}");
    }
}

#[test]
fn should_reject_unselected_name_type_casts() {
    // Arrange
    let statements = [
        ("SELECT 'word'::name", "name"),
        ("SELECT 'word'::pg_catalog.name", "pg_catalog.name"),
    ];

    // Act
    let parsed = statements.map(|(sql, _)| parse_statement(sql));

    // Assert
    for ((sql, type_name), result) in statements.into_iter().zip(parsed) {
        let error = result.expect_err("unselected NAME type must reject");
        assert_eq!(error.kind(), cassie::sql::SqlErrorKind::Unsupported);
        assert_eq!(
            error.message(),
            format!("unsupported data type '{type_name}'"),
            "the cast must be rejected by type resolution: {sql}"
        );
    }
}

#[test]
fn should_bound_postfix_cast_nesting_given_an_excessive_chain_when_parsing_an_expression() {
    // Arrange
    let admitted_sql = format!("SELECT 1{}", "::INT8".repeat(128));
    let rejected_sql = format!("SELECT 1{}", "::INT8".repeat(129));

    // Act
    let admitted = parse_statement(&admitted_sql);
    let rejected = parse_statement(&rejected_sql);

    // Assert
    assert!(admitted.is_ok(), "the admitted cast boundary must parse");
    let error =
        rejected.expect_err("excessive casts must reject before recursive AST construction");
    assert_eq!(error.kind(), cassie::sql::SqlErrorKind::ResourceLimit);
}

#[test]
fn should_bound_combined_cast_depth_given_parenthesized_chains_when_parsing_an_expression() {
    // Arrange
    let inner = format!("1{}", "::INT8".repeat(64));
    let admitted_sql = format!("SELECT ({inner}){}", "::INT8".repeat(64));
    let rejected_sql = format!("SELECT ({inner}){}", "::INT8".repeat(65));
    let mut adversarial = "1".to_owned();
    let chain = "::INT8".repeat(128);
    for _ in 0..128 {
        adversarial = format!("({adversarial}){chain}");
    }

    // Act
    let admitted = parse_statement(&admitted_sql);
    let rejected = parse_statement(&rejected_sql);
    let adversarial = parse_statement(&format!("SELECT {adversarial}"));

    // Assert
    assert!(admitted.is_ok(), "128 combined casts remain admitted");
    for result in [rejected, adversarial] {
        let error = result.expect_err("parentheses must not reset the postfix depth budget");
        assert_eq!(error.kind(), cassie::sql::SqlErrorKind::ResourceLimit);
    }
}

#[test]
fn should_admit_independent_cast_chains_given_a_wide_projection_when_each_path_is_within_budget() {
    // Arrange
    let expression = format!("1{}", "::INT8".repeat(128));
    let sql = format!("SELECT {expression}, {expression}, {expression}");

    // Act
    let parsed = parse_statement(&sql);

    // Assert
    let parsed = parsed.expect("independent projection paths retain separate cast-depth budgets");
    let QueryStatement::Select(statement) = parsed.statement else {
        panic!("expected SELECT");
    };
    assert_eq!(statement.projection.len(), 3);
}

#[test]
fn should_bound_nested_query_cast_depth_given_an_exists_operand_when_postfix_casts_are_added() {
    // Arrange
    let subquery = format!("SELECT 1{}", "::INT8".repeat(64));
    let admitted_sql = format!("SELECT (EXISTS ({subquery})){}", "::BOOL".repeat(32));
    let rejected_sql = format!("SELECT (EXISTS ({subquery})){}", "::BOOL".repeat(65));

    // Act
    let admitted = parse_statement(&admitted_sql);
    let rejected = parse_statement(&rejected_sql);

    // Assert
    assert!(admitted.is_ok(), "a bounded nested query remains admitted");
    let error = rejected.expect_err("a nested query must retain its operand depth");
    assert_eq!(error.kind(), cassie::sql::SqlErrorKind::ResourceLimit);
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
