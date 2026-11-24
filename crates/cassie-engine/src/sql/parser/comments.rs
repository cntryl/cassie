//! Parser for the finite schema, table, and column COMMENT ON surface.

use crate::sql::ast::{
    CommentOnObjectKind, CommentOnStatement, Expr, IdentifierPath, ParsedStatement, QueryStatement,
};

use super::SqlError;

pub(super) fn parse_comment_statement(sql: &str) -> Result<ParsedStatement, SqlError> {
    let trimmed = sql.trim().trim_end_matches(';').trim();
    let Some(rest) = trimmed.get("COMMENT".len()..) else {
        return Err(SqlError::unsupported(
            "COMMENT ON supports only schemas, tables, and columns".to_owned(),
        ));
    };
    let rest = rest.trim_start();
    if !rest
        .get(..2)
        .is_some_and(|keyword| keyword.eq_ignore_ascii_case("ON"))
        || rest
            .get(2..)
            .and_then(|tail| tail.chars().next())
            .is_some_and(|next| !next.is_whitespace())
    {
        return Err(SqlError::unsupported(
            "COMMENT ON supports only schemas, tables, and columns".to_owned(),
        ));
    }
    let rest = rest[2..].trim_start();
    let Some((kind, target)) = split_target(rest) else {
        return Err(SqlError::syntax(
            "COMMENT ON requires an object target".to_owned(),
        ));
    };
    let Some((object, description)) = super::clauses::split_top_level(target, "IS") else {
        return Err(SqlError::syntax(
            "COMMENT ON requires an IS clause".to_owned(),
        ));
    };
    let object = object.trim();
    let description = description.trim();
    if object.is_empty() || description.is_empty() {
        return Err(SqlError::syntax(
            "COMMENT ON requires an object name and comment value".to_owned(),
        ));
    }
    let (kind, object, column) = match kind.to_ascii_lowercase().as_str() {
        "schema" => (
            CommentOnObjectKind::Schema,
            IdentifierPath::parse(object),
            None,
        ),
        "table" => (
            CommentOnObjectKind::Table,
            IdentifierPath::parse(object),
            None,
        ),
        "column" => {
            let path = IdentifierPath::parse(object).map_err(SqlError::syntax)?;
            if path.components().len() < 2 {
                return Err(SqlError::syntax(
                    "COMMENT ON COLUMN requires a relation and column name".to_owned(),
                ));
            }
            let mut components = path.components().to_vec();
            let column = components
                .pop()
                .expect("validated column path has a final component");
            (
                CommentOnObjectKind::Column,
                Ok(IdentifierPath::from_components(components)),
                Some(column),
            )
        }
        unsupported => {
            return Err(SqlError::unsupported(format!(
                "COMMENT ON {unsupported} is not supported"
            )));
        }
    };
    let object = object.map_err(SqlError::syntax)?;
    let description = match super::expr::parse_expr_token(description)? {
        Expr::StringLiteral(description) => Some(description),
        Expr::Null => None,
        _ => {
            return Err(SqlError::unsupported(
                "COMMENT ON accepts only a string literal or NULL".to_owned(),
            ));
        }
    };
    Ok(ParsedStatement {
        raw_sql: trimmed.to_owned(),
        statement: QueryStatement::CommentOn(CommentOnStatement {
            kind,
            object,
            column,
            description,
        }),
    })
}

fn split_target(rest: &str) -> Option<(&str, &str)> {
    let end = rest.find(char::is_whitespace)?;
    Some((&rest[..end], rest[end..].trim_start()))
}
