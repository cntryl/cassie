//! Prefix signs bind below casts and above multiplicative operators.
use super::{depth, parse_postfix_expression};
use crate::sql::ast::{Expr, UnaryNumericOp};
use crate::sql::SqlError;

pub(super) fn parse(raw: &str) -> Result<Expr, SqlError> {
    let mut signs = [UnaryNumericOp::Plus; depth::MAX_POSTFIX_DEPTH];
    let mut count = 0;
    let mut operand = raw;
    loop {
        operand = super::super::lexical::trim_separators(operand);
        let op = match operand.as_bytes().first() {
            Some(b'~') => UnaryNumericOp::Complement,
            Some(b'+') => UnaryNumericOp::Plus,
            Some(b'-') => UnaryNumericOp::Minus,
            _ => break,
        };
        if count == signs.len() {
            return Err(depth::limit_error());
        }
        signs[count] = op;
        count += 1;
        operand = &operand[1..];
    }
    let mut expression = parse_postfix_expression(operand)?;
    if count == 0 {
        return Ok(expression);
    }
    depth::validate(&expression, count)?;
    for op in signs[..count].iter().rev() {
        expression = match (op, expression) {
            (
                UnaryNumericOp::Plus,
                literal @ (Expr::IntegerLiteral(_)
                | Expr::NumericLiteral(_)
                | Expr::NumberLiteral(_)),
            ) => literal,
            (UnaryNumericOp::Minus, Expr::IntegerLiteral(value)) => {
                value.checked_neg().map_or_else(
                    || Expr::NumericLiteral(value.unsigned_abs().to_string()),
                    Expr::IntegerLiteral,
                )
            }
            (UnaryNumericOp::Minus, Expr::NumberLiteral(value)) => Expr::NumberLiteral(-value),
            (UnaryNumericOp::Minus, Expr::NumericLiteral(value)) => {
                let signed = if let Some(positive) = value.strip_prefix('-') {
                    positive.to_owned()
                } else {
                    format!("-{value}")
                };
                signed
                    .parse::<i64>()
                    .map_or_else(|_| Expr::NumericLiteral(signed), Expr::IntegerLiteral)
            }
            (op, expression) => Expr::UnaryNumeric {
                op: *op,
                expr: Box::new(expression),
            },
        };
    }
    Ok(expression)
}

#[cfg(test)]
#[path = "unary_numeric_tests.rs"]
mod tests;
