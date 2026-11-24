//! Resolve bound column names in source scope before rejecting row variables.
use super::{CassieError, Expr, HashSet, SelectStatement};

pub(super) fn validate(
    select: &SelectStatement,
    known_fields: &HashSet<String>,
    projection_aliases: &HashSet<String>,
) -> Result<(), CassieError> {
    for (bound, clause) in [
        (select.limit.as_ref(), "LIMIT"),
        (select.offset.as_ref(), "OFFSET"),
    ] {
        if let Some(expression) = bound {
            super::validate_expression(expression, known_fields, projection_aliases, false)?;
            reject_variables(expression, clause, 0)?;
        }
    }
    Ok(())
}

fn reject_variables(expression: &Expr, clause: &str, depth: usize) -> Result<(), CassieError> {
    if depth > 128 {
        return Err(CassieError::ResourceLimit(
            "pagination reference nesting exceeds 128 levels".into(),
        ));
    }
    if matches!(expression, Expr::Column(_)) {
        return Err(CassieError::Execution(format!(
            "argument of {clause} must not contain variables"
        )));
    }
    expression.try_visit_children(&mut |child| reject_variables(child, clause, depth + 1))
}
