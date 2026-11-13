//! `ALTER TYPE` for composite (#828) and enum (#827) types.
use super::expr::split_csv;
use super::schema::{
    parse_column_identifier, parse_data_type, parse_identifier, split_first_token,
};
use super::{ParsedStatement, QueryStatement, SqlError};
use crate::sql::ast::{TypeAction, TypeAlteration, TypeAttribute, TypeStatement};

/// Strips `keyword` (case-insensitive, on a word boundary) from the front.
fn strip_keyword<'a>(text: &'a str, keyword: &str) -> Option<&'a str> {
    let lower = text.to_ascii_lowercase();
    let rest = lower.strip_prefix(keyword)?;
    if !(rest.is_empty() || rest.starts_with(char::is_whitespace)) {
        return None;
    }
    Some(text[keyword.len()..].trim_start())
}

/// Drops a trailing `CASCADE` or `RESTRICT`. Cassie has no typed tables, the
/// only objects `CASCADE` would recurse into, so both behave the same.
fn strip_behavior(text: &str) -> &str {
    let trimmed = text.trim_end();
    let lower = trimmed.to_ascii_lowercase();
    for suffix in [" cascade", " restrict"] {
        if lower.ends_with(suffix) {
            return trimmed[..trimmed.len() - suffix.len()].trim_end();
        }
    }
    trimmed
}

fn syntax(message: &str) -> SqlError {
    SqlError::syntax(message.to_owned())
}

fn attribute_name(raw: &str) -> Result<String, SqlError> {
    let name = parse_column_identifier(raw)?;
    if name.is_empty() {
        return Err(syntax("ALTER TYPE requires an attribute name"));
    }
    Ok(name)
}

fn type_text(raw: &str) -> Result<crate::types::DataType, SqlError> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err(syntax("ALTER TYPE requires a data type"));
    }
    if raw.to_ascii_lowercase().contains(" collate ") {
        return Err(SqlError::unsupported(
            "COLLATE on composite type attributes is not supported".into(),
        ));
    }
    parse_data_type(raw)
}

fn parse_rename(rest: &str) -> Result<TypeAlteration, SqlError> {
    if let Some(after) = strip_keyword(rest, "to") {
        let new_name = parse_identifier(after.trim())?;
        if new_name.is_empty() || after.trim().contains(char::is_whitespace) {
            return Err(syntax("ALTER TYPE ... RENAME TO requires a new name"));
        }
        return Ok(TypeAlteration::RenameTo { new_name });
    }
    if let Some(after) = strip_keyword(rest, "attribute") {
        let (from, after) =
            split_first_token(after).ok_or_else(|| syntax("RENAME ATTRIBUTE requires a name"))?;
        let after = strip_keyword(after, "to")
            .ok_or_else(|| syntax("RENAME ATTRIBUTE requires TO and a new name"))?;
        let to = attribute_name(strip_behavior(after))?;
        return Ok(TypeAlteration::RenameAttribute {
            from: attribute_name(&from)?,
            to,
        });
    }
    if let Some(after) = strip_keyword(rest, "value") {
        let (from, to) = super::enum_labels::parse_rename_value(after)?;
        return Ok(TypeAlteration::RenameValue { from, to });
    }
    Err(syntax("ALTER TYPE RENAME expects TO or ATTRIBUTE"))
}

fn parse_add(rest: &str) -> Result<TypeAlteration, SqlError> {
    if let Some(after) = strip_keyword(rest, "attribute") {
        let (name, after) =
            split_first_token(after).ok_or_else(|| syntax("ADD ATTRIBUTE requires a name"))?;
        return Ok(TypeAlteration::AddAttribute {
            attribute: TypeAttribute {
                name: attribute_name(&name)?,
                data_type: type_text(strip_behavior(after))?,
            },
        });
    }
    if let Some(after) = strip_keyword(rest, "value") {
        let added = super::enum_labels::parse_add_value(after)?;
        return Ok(TypeAlteration::AddValue {
            label: added.label,
            if_not_exists: added.if_not_exists,
            placement: added.placement,
        });
    }
    Err(syntax("ALTER TYPE ADD expects ATTRIBUTE or VALUE"))
}

fn parse_drop(rest: &str) -> Result<TypeAlteration, SqlError> {
    let after = strip_keyword(rest, "attribute")
        .ok_or_else(|| syntax("ALTER TYPE DROP expects ATTRIBUTE"))?;
    let (if_exists, after) = match strip_keyword(after, "if") {
        Some(tail) => (
            true,
            strip_keyword(tail, "exists")
                .ok_or_else(|| syntax("DROP ATTRIBUTE IF requires EXISTS"))?,
        ),
        None => (false, after),
    };
    let name = attribute_name(strip_behavior(after))?;
    Ok(TypeAlteration::DropAttribute { name, if_exists })
}

fn parse_alter_attribute(rest: &str) -> Result<TypeAlteration, SqlError> {
    let after = strip_keyword(rest, "attribute")
        .ok_or_else(|| syntax("ALTER TYPE ALTER expects ATTRIBUTE"))?;
    let (name, after) =
        split_first_token(after).ok_or_else(|| syntax("ALTER ATTRIBUTE requires a name"))?;
    let after = strip_keyword(after, "set")
        .and_then(|tail| strip_keyword(tail, "data"))
        .unwrap_or(after);
    let after = strip_keyword(after, "type")
        .ok_or_else(|| syntax("ALTER ATTRIBUTE requires [SET DATA] TYPE"))?;
    Ok(TypeAlteration::AlterAttributeType {
        name: attribute_name(&name)?,
        data_type: type_text(strip_behavior(after))?,
    })
}

pub(super) fn parse_alter_type_statement(sql: &str) -> Result<ParsedStatement, SqlError> {
    let trimmed = sql.trim().trim_end_matches(';').trim();
    let after_alter = trimmed["alter".len()..].trim_start();
    let rest = after_alter["type".len()..].trim_start();
    let (name_token, after_name) =
        split_first_token(rest).ok_or_else(|| syntax("ALTER TYPE requires a name"))?;
    let name = parse_identifier(&name_token)?;
    if name.is_empty() {
        return Err(syntax("ALTER TYPE requires a name"));
    }
    if split_csv(after_name).len() > 1 {
        return Err(SqlError::unsupported(
            "ALTER TYPE with several comma-separated actions is not supported".into(),
        ));
    }
    let alteration = if let Some(tail) = strip_keyword(after_name, "rename") {
        parse_rename(tail)?
    } else if let Some(tail) = strip_keyword(after_name, "add") {
        parse_add(tail)?
    } else if let Some(tail) = strip_keyword(after_name, "drop") {
        parse_drop(tail)?
    } else if let Some(tail) = strip_keyword(after_name, "alter") {
        parse_alter_attribute(tail)?
    } else if let Some(tail) = strip_keyword(after_name, "set") {
        let schema_name = strip_keyword(tail, "schema")
            .ok_or_else(|| SqlError::unsupported("ALTER TYPE SET supports only SCHEMA".into()))?;
        TypeAlteration::SetSchema {
            schema: parse_identifier(schema_name.trim())?,
        }
    } else if let Some(tail) = strip_keyword(after_name, "owner") {
        let owner =
            strip_keyword(tail, "to").ok_or_else(|| syntax("ALTER TYPE OWNER requires TO"))?;
        TypeAlteration::OwnerTo {
            owner: parse_identifier(owner.trim())?,
        }
    } else {
        return Err(syntax(
            "ALTER TYPE expects RENAME, ADD, DROP, ALTER, SET or OWNER",
        ));
    };
    Ok(ParsedStatement {
        raw_sql: trimmed.to_owned(),
        statement: QueryStatement::Type(TypeStatement {
            action: TypeAction::Alter { name, alteration },
        }),
    })
}

#[cfg(test)]
#[path = "types_alter_tests.rs"]
mod tests;
