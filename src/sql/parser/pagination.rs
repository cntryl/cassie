//! Parse the selected finite pagination expression grammar.

use super::{Expr, SqlError};

pub(super) fn parse_bound(sql: &str, offset: bool) -> Result<Option<Expr>, SqlError> {
    if !offset && sql.eq_ignore_ascii_case("all") {
        return Ok(None);
    }
    let uncommented = crate::sql::parser::lexical::without_comments(sql);
    if uncommented
        .bytes()
        .filter(|byte| matches!(byte, b'+' | b'-' | b'*'))
        .count()
        > 128
    {
        return Err(SqlError::resource_limit(
            "pagination expression exceeds the 128 operator budget".into(),
        ));
    }
    let expr = crate::sql::parser::expr::parse_expression(sql)?;
    // Operator selection belongs to semantic admission, before the selected
    // integer pagination gate. Rejecting operand types here hides native
    // ambiguous/undefined operator errors behind an unsupported syntax error.
    Ok(Some(expr))
}
