//! Selected simple field/literal CHECK predicates.
use super::{
    parse_constant_literal, strip_parentheses, ConstraintCheck, ConstraintOperator, SqlError, Value,
};

pub(super) fn parse_check_constraint(raw: &str) -> Result<ConstraintCheck, SqlError> {
    let expression = raw.trim();
    let upper = expression.to_ascii_uppercase();
    if upper.ends_with(" NOT ENFORCED") || upper.ends_with(" ENFORCED") {
        return Err(SqlError::unsupported(
            "CHECK constraint enforcement attributes are not supported".into(),
        ));
    }
    if upper.ends_with(" NOT VALID") {
        return Err(SqlError::unsupported(
            "NOT VALID CHECK constraints are not supported".into(),
        ));
    }
    if !expression.starts_with('(') || !expression.ends_with(')') {
        return Err(SqlError::new(
            "CHECK expression must be parenthesized".to_string(),
        ));
    }
    let inner = strip_parentheses(expression)
        .ok_or_else(|| SqlError::new("invalid CHECK expression".to_string()))?
        .trim();

    crate::sql::definition_guard::expression_sql(inner)
        .map_err(|error| SqlError::unsupported(error.to_string()))?;

    let (left, op, right) = parse_simple_comparison(inner)
        .ok_or_else(|| SqlError::new("unsupported CHECK expression".to_string()))?;

    if left.starts_with('(') {
        return Err(SqlError::unsupported(
            "a CHECK constraint on a field of a composite column is not supported".to_string(),
        ));
    }
    if !left
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
    {
        return Err(SqlError::new(
            "CHECK expression field must be an identifier".to_string(),
        ));
    }

    Ok(ConstraintCheck {
        field: left.clone(),
        operator: op,
        value: right,
        bound_expression: None,
    })
}

pub(super) fn parse_simple_comparison(raw: &str) -> Option<(String, ConstraintOperator, Value)> {
    let candidates = [
        (
            " is not distinct from ",
            ConstraintOperator::IsNotDistinctFrom,
        ),
        (" is distinct from ", ConstraintOperator::IsDistinctFrom),
        (" <=", ConstraintOperator::Lte),
        (" >=", ConstraintOperator::Gte),
        (" <> ", ConstraintOperator::NotEq),
        (" != ", ConstraintOperator::NotEq),
        (" like ", ConstraintOperator::Like),
        (" < ", ConstraintOperator::Lt),
        (" > ", ConstraintOperator::Gt),
        (" = ", ConstraintOperator::Eq),
    ];

    for (operator, kind) in candidates {
        let lower = raw.to_lowercase();
        if let Some(position) = lower.find(operator) {
            let left = raw[..position].trim();
            let right = raw[position + operator.len()..].trim();
            if left.is_empty() || right.is_empty() {
                continue;
            }

            if let Ok(value) = parse_constant_literal(right) {
                return Some((left.to_string(), kind, value));
            }
        }
    }

    None
}
