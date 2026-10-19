//! Sublink syntax: `(SELECT ...)`, `ARRAY(SELECT ...)`, `[NOT] IN (SELECT ...)`
//! and `op ANY|SOME|ALL (SELECT ...)`, each with an optional row constructor
//! on the left.
use super::super::{clauses::strip_parentheses, lexical, schema::starts_with_keyword};
use super::SqlError;
use super::{parse_comparison_expression, parse_statement, BinaryOp, Expr, QueryStatement};
use crate::sql::ast::{RowComparisonOp, SubqueryExpr, SubqueryKind};

/// The text of a parenthesized query, when `raw` is exactly `( query )`.
pub(super) fn body(raw: &str) -> Option<&str> {
    let inner = strip_parentheses(lexical::trim_separators(raw))?;
    is_query(inner, 0).then_some(inner)
}

/// `SELECT ...`, or a parenthesized query optionally followed by a set
/// operation or ordering clause: `((SELECT 1))`, `(SELECT 1) UNION ...`.
/// A list such as `(SELECT 1), 2` is not a query.
fn is_query(text: &str, depth: usize) -> bool {
    let text = lexical::trim_separators(text);
    if depth > 32 {
        return false;
    }
    if text.starts_with('(') {
        let Some(close) = lexical::matching_paren(text, 0) else {
            return false;
        };
        let rest = lexical::trim_separators(&text[close + 1..]);
        return is_query(&text[1..close], depth + 1)
            && (rest.is_empty()
                || [
                    "union",
                    "intersect",
                    "except",
                    "order",
                    "limit",
                    "offset",
                    "fetch",
                ]
                .iter()
                .any(|keyword| starts_with_keyword(rest, keyword)));
    }
    ["select", "with", "values", "table"]
        .iter()
        .any(|keyword| starts_with_keyword(text, keyword))
}

fn query(raw: &str) -> Result<crate::sql::ast::ParsedStatement, SqlError> {
    let parsed = parse_statement(raw)?;
    if !matches!(parsed.statement, QueryStatement::Select(_)) {
        return Err(SqlError::syntax(
            "a subquery used as an expression must be a SELECT".into(),
        ));
    }
    Ok(parsed)
}

fn sublink(kind: SubqueryKind, raw: &str) -> Result<Expr, SqlError> {
    let expression = Expr::Subquery(Box::new(SubqueryExpr::new(kind, query(raw)?)));
    super::depth::validate(&expression, 0)?;
    Ok(expression)
}

/// `(SELECT ...)` used as a scalar expression.
pub(super) fn scalar(raw: &str) -> Result<Option<Expr>, SqlError> {
    body(raw)
        .map(|inner| sublink(SubqueryKind::Scalar, inner))
        .transpose()
}

/// `ARRAY(SELECT ...)`.
pub(super) fn array(raw: &str) -> Result<Option<Expr>, SqlError> {
    let raw = lexical::trim_separators(raw);
    if !starts_with_keyword(raw, "array") {
        return Ok(None);
    }
    body(&raw[5..])
        .map(|inner| sublink(SubqueryKind::Array, inner))
        .transpose()
}

/// The outer operands of a sublink predicate: a row constructor's fields, or
/// one expression.
fn operands(left: &str) -> Result<Vec<Expr>, SqlError> {
    if let Some(fields) = super::row_predicates::constructor(left)? {
        return Ok(fields);
    }
    Ok(vec![parse_comparison_expression(left)?])
}

/// `left [NOT] IN (SELECT ...)`.
pub(super) fn membership(left: &str, right: &str, negated: bool) -> Result<Option<Expr>, SqlError> {
    let Some(inner) = body(right) else {
        return Ok(None);
    };
    sublink(
        SubqueryKind::Quantified {
            left: operands(left)?,
            op: BinaryOp::Eq,
            all: false,
            membership: true,
            negated,
        },
        inner,
    )
    .map(Some)
}

/// `left op ANY|SOME|ALL (SELECT ...)` and `(a, b) op (SELECT x, y ...)`.
pub(super) fn comparison(left: &str, op: &BinaryOp, right: &str) -> Result<Option<Expr>, SqlError> {
    let right = lexical::trim_separators(right);
    for (keyword, all) in [("any", false), ("some", false), ("all", true)] {
        if !starts_with_keyword(right, keyword) {
            continue;
        }
        let Some(inner) = body(&right[keyword.len()..]) else {
            return array_quantifier(left, op, all, &right[keyword.len()..]);
        };
        return sublink(
            SubqueryKind::Quantified {
                left: operands(left)?,
                op: op.clone(),
                all,
                membership: false,
                negated: false,
            },
            inner,
        )
        .map(Some);
    }
    let Some(inner) = body(right) else {
        return Ok(None);
    };
    let Some(fields) = super::row_predicates::constructor(left)? else {
        return Ok(None);
    };
    let op = RowComparisonOp::from_binary(op).ok_or_else(|| {
        SqlError::unsupported("operator on row constructors is not yet supported".into())
    })?;
    sublink(SubqueryKind::Row { left: fields, op }, inner).map(Some)
}

/// `left op ANY|SOME|ALL (array)`.
fn array_quantifier(
    left: &str,
    op: &BinaryOp,
    all: bool,
    array: &str,
) -> Result<Option<Expr>, SqlError> {
    let Some(array) = strip_parentheses(lexical::trim_separators(array)) else {
        return Ok(None);
    };
    let expression = Expr::Binary {
        left: Box::new(parse_comparison_expression(left)?),
        op: BinaryOp::Quantified {
            comparison: Box::new(op.clone()),
            all,
        },
        right: Box::new(super::parse_expression(array)?),
    };
    super::depth::validate(&expression, 0)?;
    Ok(Some(expression))
}

/// PostgreSQL's result column name for an unaliased sublink: `ARRAY(...)` is
/// `array`, and a scalar subquery takes its own first column's name.
pub(in crate::sql::parser) fn output_name(subquery: &SubqueryExpr) -> Option<String> {
    use crate::sql::ast::SelectItem;
    match subquery.kind {
        SubqueryKind::Array => return Some("array".into()),
        SubqueryKind::Scalar => {}
        SubqueryKind::Row { .. } | SubqueryKind::Quantified { .. } => return None,
    }
    let QueryStatement::Select(select) = &subquery.query.statement else {
        return None;
    };
    match select.projection.first()? {
        SelectItem::Column {
            alias: Some(alias), ..
        }
        | SelectItem::Function {
            alias: Some(alias), ..
        }
        | SelectItem::Expr {
            alias: Some(alias), ..
        }
        | SelectItem::WindowFunction {
            alias: Some(alias), ..
        } => Some(alias.clone()),
        SelectItem::Column { name, .. } => crate::sql::ColumnIdentifierPath::parse(name)
            .ok()
            .map(|column| column.implicit_output_label()),
        SelectItem::Function { function, .. } => Some(
            function
                .name
                .rsplit('.')
                .next()
                .unwrap_or(&function.name)
                .to_ascii_lowercase(),
        ),
        SelectItem::Expr { .. } | SelectItem::WindowFunction { .. } | SelectItem::Wildcard => None,
    }
}

#[cfg(test)]
mod tests {
    use super::body;

    #[test]
    fn should_recognize_parenthesized_queries_but_not_lists_of_subqueries() {
        // Arrange
        let candidates = [
            "(SELECT 1)",
            "((SELECT 1))",
            "((SELECT 1) UNION (SELECT 2))",
            "(VALUES (1))",
            "((SELECT 1), 2)",
            "(1, 2)",
        ];

        // Act
        let recognized = candidates.map(|candidate| body(candidate).is_some());

        // Assert
        assert_eq!(recognized, [true, true, true, true, false, false]);
    }
}
