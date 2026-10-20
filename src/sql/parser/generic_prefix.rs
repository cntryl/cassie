//! Generic prefix operators bind below arithmetic, with bounded chain depth.
use super::qualified_operators::Index;
use super::{depth, parse_arithmetic_expression_indexed};
use crate::sql::ast::{Expr, UnaryNumericOp};
use crate::sql::SqlError;

pub(super) fn parse(raw: &str, qualified_operators: &Index<'_>) -> Result<Option<Expr>, SqlError> {
    let mut operators = [UnaryNumericOp::Plus; depth::MAX_POSTFIX_DEPTH];
    let mut count = 0;
    let mut operand = raw;
    loop {
        operand = super::super::lexical::trim_separators(operand);
        let Some((token, op)) = prefix(operand)? else {
            break;
        };
        if count == operators.len() {
            return Err(depth::limit_error());
        }
        let tail = &operand[token.len()..];
        if !token.ends_with(')') {
            check_tail(tail, op)?;
        }
        operators[count] = op;
        count += 1;
        operand = tail;
    }
    if count == 0 {
        return Ok(None);
    }
    let mut expression = parse_arithmetic_expression_indexed(operand, qualified_operators)?;
    depth::validate(&expression, count)?;
    for op in operators[..count].iter().rev() {
        expression = Expr::UnaryNumeric {
            op: *op,
            expr: Box::new(expression),
        };
    }
    Ok(Some(expression))
}

fn generic(raw: &str) -> Result<Option<(&str, UnaryNumericOp)>, SqlError> {
    if let Some(prefix) = super::qualified_operators::prefix(raw)? {
        return Ok(Some(prefix));
    }
    Ok([
        ("||/", UnaryNumericOp::CubeRoot),
        ("|/", UnaryNumericOp::SquareRoot),
        ("@", UnaryNumericOp::Absolute),
        ("~", UnaryNumericOp::Complement),
    ]
    .into_iter()
    .find(|(token, _)| raw.starts_with(token)))
}

// A high-precedence sign preceding a generic prefix wraps that prefix's
// complete arithmetic operand: - @ -7+10 = -(abs(-7+10)). Ordinary signs
// before a literal remain owned by the existing arithmetic parser.
fn prefix(raw: &str) -> Result<Option<(&str, UnaryNumericOp)>, SqlError> {
    if let Some(operator) = generic(raw)? {
        return Ok(Some(operator));
    }
    let sign = match raw.as_bytes().first() {
        Some(b'+') => ("+", UnaryNumericOp::Plus),
        Some(b'-') => ("-", UnaryNumericOp::Minus),
        _ => return Ok(None),
    };
    let mut rest = raw;
    loop {
        rest = super::super::lexical::trim_separators(rest);
        if rest.starts_with(['+', '-']) {
            rest = &rest[1..];
        } else {
            return Ok(generic(rest)?.map(|_| sign));
        }
    }
}

pub(super) fn check_tail(tail: &str, op: UnaryNumericOp) -> Result<(), SqlError> {
    let signs = matches!(op, UnaryNumericOp::Plus | UnaryNumericOp::Minus);
    if tail.starts_with(super::super::operators::operator_character)
        && !(signs && tail.starts_with(['+', '-']))
    {
        return Err(SqlError::syntax(
            "compound prefix operator requires its own signature".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
#[path = "generic_prefix_tests.rs"]
mod tests;
