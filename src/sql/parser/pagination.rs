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
    if !crate::sql::pagination::is_admitted(&expr) {
        return Err(SqlError::unsupported(
            "unsupported pagination expression; expected integer literals, NULL, parameters, integer casts or +, - and *".into(),
        ));
    }
    Ok(Some(expr))
}
