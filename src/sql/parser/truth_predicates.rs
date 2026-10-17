//! Truth predicates bind below comparisons, preserving native left association.
use super::{depth, parse_comparison_expression, Expr, SqlError};
use crate::sql::ast::TruthValue;

pub(super) fn parse(raw: &str) -> Result<Option<Expr>, SqlError> {
    let mut tests = [(TruthValue::Unknown, false); depth::MAX_POSTFIX_DEPTH];
    let mut count = 0;
    let mut operand = raw;
    while let Some((left, truth, negated)) = suffix(operand) {
        if count == tests.len() {
            return Err(depth::limit_error());
        }
        tests[count] = (truth, negated);
        count += 1;
        operand = left;
    }
    if count == 0 {
        return Ok(None);
    }
    let mut expr = parse_comparison_expression(operand)?;
    depth::validate(&expr, count)?;
    for (truth, negated) in tests[..count].iter().rev() {
        expr = Expr::TruthTest {
            expr: Box::new(expr),
            truth: *truth,
            negated: *negated,
        };
    }
    Ok(Some(expr))
}
fn suffix(raw: &str) -> Option<(&str, TruthValue, bool)> {
    [
        (" is not true", TruthValue::True, true),
        (" is true", TruthValue::True, false),
        (" is not false", TruthValue::False, true),
        (" is false", TruthValue::False, false),
        (" is not unknown", TruthValue::Unknown, true),
        (" is unknown", TruthValue::Unknown, false),
    ]
    .into_iter()
    .find_map(|(suffix, truth, negated)| {
        super::super::lexical::strip_keyword_suffix(raw, suffix)
            .map(|operand| (operand, truth, negated))
    })
}
#[cfg(test)]
#[path = "truth_predicates_tests.rs"]
mod tests;
