//! LIKE predicates bind above scalar comparisons and below arithmetic operands.
use super::{parse_comparison_expression, split_top_level, Expr, SqlError};
use crate::sql::ast::BinaryOp;

fn split_predicate(raw: &str) -> Option<(&str, &str, bool)> {
    [(" not like ", true), (" like ", false)]
        .into_iter()
        .filter_map(|(keyword, negated)| {
            split_top_level(raw, keyword).map(|(left, right)| (left, right, negated))
        })
        .min_by_key(|(left, _, _)| left.len())
}

pub(super) fn parse(raw: &str) -> Result<Option<Expr>, SqlError> {
    let Some((left, right, negated)) = split_predicate(raw) else {
        return Ok(None);
    };
    if let Some((_, _, next_negated)) = split_predicate(right) {
        let keyword = if next_negated { "NOT" } else { "LIKE" };
        return Err(SqlError::new(format!(
            "syntax error at or near \"{keyword}\""
        )));
    }
    let expr = parse_comparison_expression(left)?;
    let predicate = if let Some((pattern, escape)) = split_top_level(right, " escape ") {
        Expr::Like {
            expr: Box::new(expr),
            pattern: Box::new(parse_comparison_expression(pattern)?),
            escape: Box::new(parse_comparison_expression(escape)?),
        }
    } else {
        Expr::Binary {
            left: Box::new(expr),
            op: BinaryOp::Like,
            right: Box::new(parse_comparison_expression(right)?),
        }
    };
    Ok(Some(if negated {
        Expr::Not {
            expr: Box::new(predicate),
        }
    } else {
        predicate
    }))
}
