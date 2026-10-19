//! Recognize row constructors only at a row-predicate boundary.
use super::super::{clauses::strip_parentheses, lexical, schema};
use super::{parse_comparison_expression, parse_expression, split_csv};
use super::{BinaryOp, Expr, SqlError};
use crate::sql::ast::{RowComparison, RowComparisonOp};

pub(super) fn comparison(left: &str, op: BinaryOp, right: &str) -> Result<Expr, SqlError> {
    if let Some(subquery) = super::subquery_syntax::comparison(left, &op, right)? {
        return Ok(subquery);
    }
    let constructors = (constructor(left)?, constructor(right)?);
    if let (Some(left), Some(right)) = constructors {
        let op = RowComparisonOp::from_binary(&op).ok_or_else(|| {
            SqlError::unsupported("operator on row constructors is not yet supported".into())
        })?;
        if left.len() != right.len() {
            return Err(SqlError::syntax(
                "unequal number of entries in row expressions".into(),
            ));
        }
        if left.is_empty() && !op.null_safe() {
            return Err(SqlError::unsupported(
                "cannot compare rows of zero length".into(),
            ));
        }
        let expression = Expr::RowCompare(Box::new(RowComparison { left, op, right }));
        super::depth::validate(&expression, 0)?;
        return Ok(expression);
    }
    Ok(Expr::Binary {
        left: Box::new(parse_comparison_expression(left)?),
        op,
        right: Box::new(parse_comparison_expression(right)?),
    })
}

pub(super) fn constructor(mut raw: &str) -> Result<Option<Vec<Expr>>, SqlError> {
    loop {
        raw = lexical::trim_separators(raw);
        if super::subquery_syntax::body(raw).is_some() {
            return Ok(None);
        }
        if schema::starts_with_keyword(raw, "row") {
            let Some(inner) = strip_parentheses(lexical::trim_separators(&raw[3..])) else {
                return Ok(None);
            };
            return fields(inner).map(Some);
        }
        let Some(inner) = strip_parentheses(raw) else {
            return Ok(None);
        };
        if split_csv(inner).len() > 1 {
            return fields(inner).map(Some);
        }
        raw = inner;
    }
}

fn fields(raw: &str) -> Result<Vec<Expr>, SqlError> {
    if lexical::trim_separators(raw).is_empty() {
        return Ok(Vec::new());
    }
    let fields = split_csv(raw);
    if fields.len() > 1664 {
        return Err(SqlError::resource_limit(
            "ROW expressions can have at most 1664 entries".into(),
        ));
    }
    fields.into_iter().map(parse_expression).collect()
}
