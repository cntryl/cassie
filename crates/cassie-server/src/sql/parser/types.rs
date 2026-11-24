use super::expr::split_csv;
use super::schema::{
    parse_column_identifier, parse_data_type, parse_identifier, parse_if_exists, split_first_token,
};
use super::{ParsedStatement, QueryStatement, SqlError};
use crate::sql::ast::{TypeAction, TypeAttribute, TypeStatement};

pub(super) fn parse_create_type_statement(sql: &str) -> Result<ParsedStatement, SqlError> {
    let trimmed = sql.trim().trim_end_matches(';').trim();
    let after_create = trimmed["create".len()..].trim_start();
    let rest = after_create["type".len()..].trim_start();
    let (name_token, after_name) = split_first_token(rest)
        .ok_or_else(|| SqlError::syntax("CREATE TYPE requires a name".into()))?;
    let (name_token, after_name) = match name_token.find('(') {
        Some(paren) if !name_token.starts_with('"') => (
            name_token[..paren].to_owned(),
            format!("{} {after_name}", &name_token[paren..]),
        ),
        _ => (name_token, after_name.to_owned()),
    };
    let name = parse_identifier(&name_token)?;
    if name.is_empty() {
        return Err(SqlError::syntax("CREATE TYPE requires a name".into()));
    }
    let after_name = after_name.trim_start();
    let lower = after_name.to_ascii_lowercase();
    let body = if let Some(after_as) = lower.strip_prefix("as") {
        let offset = after_name.len() - after_as.len();
        let body = after_name[offset..].trim_start();
        if let Some(after_enum) = super::enum_labels::strip_enum_keyword(body) {
            return super::enum_labels::parse_create_enum(trimmed, name, after_enum);
        }
        if body.to_ascii_lowercase().starts_with("range") {
            return Err(SqlError::unsupported(
                "CREATE TYPE AS RANGE is not supported in this version".into(),
            ));
        }
        body
    } else if after_name.is_empty() || after_name.starts_with('(') {
        return Err(SqlError::unsupported(
            "CREATE TYPE without AS (shell and base types) is not supported".into(),
        ));
    } else {
        return Err(SqlError::syntax("CREATE TYPE expects AS (...)".into()));
    };
    let inner = body
        .strip_prefix('(')
        .and_then(|body| body.strip_suffix(')'))
        .ok_or_else(|| {
            SqlError::syntax("CREATE TYPE AS requires a parenthesized attribute list".into())
        })?
        .trim();
    let mut attributes: Vec<TypeAttribute> = Vec::new();
    if !inner.is_empty() {
        for item in split_csv(inner) {
            let (field, type_text) = split_first_token(item).ok_or_else(|| {
                SqlError::syntax("composite type attribute requires a name and a type".into())
            })?;
            let field = parse_column_identifier(&field)?;
            let type_text = type_text.trim();
            if type_text.is_empty() {
                return Err(SqlError::syntax(format!(
                    "composite type attribute \"{field}\" requires a type"
                )));
            }
            if type_text.to_ascii_lowercase().contains(" collate ") {
                return Err(SqlError::unsupported(
                    "COLLATE on composite type attributes is not supported".into(),
                ));
            }
            attributes.push(TypeAttribute {
                name: field,
                data_type: parse_data_type(type_text)
                    .map_err(|error| super::enum_scope::enum_field_error(type_text, error))?,
            });
        }
    }
    Ok(ParsedStatement {
        raw_sql: trimmed.to_owned(),
        statement: QueryStatement::Type(TypeStatement {
            action: TypeAction::Create { name, attributes },
        }),
    })
}

pub(super) fn parse_drop_type_statement(sql: &str) -> Result<ParsedStatement, SqlError> {
    let trimmed = sql.trim().trim_end_matches(';').trim();
    let after_drop = trimmed["drop".len()..].trim_start();
    let rest = after_drop["type".len()..].trim_start();
    let (if_exists, rest) = parse_if_exists(rest);
    let lower = rest.to_ascii_lowercase();
    let (list, cascade) = if let Some(head) = lower.strip_suffix(" cascade") {
        (&rest[..head.len()], true)
    } else if let Some(head) = lower.strip_suffix(" restrict") {
        (&rest[..head.len()], false)
    } else {
        (rest, false)
    };
    let mut names = Vec::new();
    for item in split_csv(list) {
        let name = parse_identifier(item.trim())?;
        if name.is_empty() {
            return Err(SqlError::syntax("DROP TYPE requires a type name".into()));
        }
        names.push(name);
    }
    if names.is_empty() {
        return Err(SqlError::syntax("DROP TYPE requires a type name".into()));
    }
    Ok(ParsedStatement {
        raw_sql: trimmed.to_owned(),
        statement: QueryStatement::Type(TypeStatement {
            action: TypeAction::Drop {
                names,
                if_exists,
                cascade,
            },
        }),
    })
}
