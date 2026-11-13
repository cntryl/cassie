//! `CREATE DOMAIN`, `ALTER DOMAIN` and `DROP DOMAIN` (#828).

use super::clauses::find_top_level_keyword_span;
use super::expr::split_csv;
use super::lexical::{matching_paren, pattern_end, quoted_end, trim_separators};
use super::schema::{parse_constant_literal, parse_identifier, parse_if_exists, split_first_token};
use super::{ParsedStatement, QueryStatement, SqlError};
use crate::sql::ast::{
    DomainAction, DomainAlteration, DomainCheckDefinition, DomainDefaultDefinition, TypeAction,
    TypeStatement,
};

/// Keywords that start a clause after a domain's base type.
const CLAUSE_STARTERS: [&str; 6] = [
    "collate",
    "default",
    "constraint",
    "check",
    "not null",
    "null",
];

fn statement(raw_sql: &str, action: DomainAction) -> ParsedStatement {
    ParsedStatement {
        raw_sql: raw_sql.to_owned(),
        statement: QueryStatement::Type(TypeStatement {
            action: TypeAction::Domain(action),
        }),
    }
}

fn normalized(sql: &str) -> &str {
    sql.trim().trim_end_matches(';').trim()
}

/// Skips `words` (a keyword phrase such as `create domain`) at the start of `sql`.
fn after_keywords<'a>(sql: &'a str, words: &str) -> Result<&'a str, SqlError> {
    let end = pattern_end(sql, 0, words)
        .ok_or_else(|| SqlError::syntax(format!("expected {}", words.to_ascii_uppercase())))?;
    Ok(trim_separators(&sql[end..]))
}

/// The offset of the first clause keyword at or after `from`, if any.
fn next_clause_start(text: &str, from: usize) -> Option<usize> {
    CLAUSE_STARTERS
        .iter()
        .filter_map(|keyword| find_top_level_keyword_span(text, from, keyword))
        .map(|(start, _)| start)
        .min()
}

/// The end of the first operand of a `DEFAULT` expression, so that a leading
/// `NULL` literal is not mistaken for a `NULL` constraint.
fn first_operand_end(text: &str) -> usize {
    if let Some(end) = quoted_end(text, 0) {
        return end;
    }
    if text.starts_with('(') {
        return matching_paren(text, 0).map_or(text.len(), |close| close + 1);
    }
    let word_end = text
        .find(|character: char| character.is_whitespace() || character == '(')
        .unwrap_or(text.len());
    if text[word_end..].starts_with('(') {
        return matching_paren(text, word_end).map_or(text.len(), |close| close + 1);
    }
    word_end
}

fn parse_default(raw: &str) -> Result<DomainDefaultDefinition, SqlError> {
    let raw = trim_separators(raw);
    if raw.is_empty() {
        return Err(SqlError::syntax("DEFAULT requires an expression".into()));
    }
    crate::sql::definition_guard::expression_sql(raw)
        .map_err(|error| SqlError::unsupported(error.to_string()))?;
    if let Some(expression) = crate::catalog::parse_volatile_default_expression(raw) {
        return Ok(DomainDefaultDefinition {
            sql: raw.to_owned(),
            value: None,
            expression: Some(expression),
        });
    }
    let value = parse_constant_literal(raw).map_err(|_| {
        SqlError::unsupported(format!(
            "DEFAULT {raw} is not supported: use a constant, now(), CURRENT_TIMESTAMP, LOCALTIMESTAMP, CURRENT_DATE or gen_random_uuid()"
        ))
    })?;
    Ok(DomainDefaultDefinition {
        sql: raw.to_owned(),
        value: Some(value),
        expression: None,
    })
}

/// Parses `(expression)` at the start of `text`; returns the expression text
/// and the remainder after the closing parenthesis.
fn parse_check_body(text: &str) -> Result<(String, &str), SqlError> {
    let text = trim_separators(text);
    if !text.starts_with('(') {
        return Err(SqlError::syntax(
            "CHECK requires a parenthesized expression".into(),
        ));
    }
    let close = matching_paren(text, 0).ok_or_else(|| {
        SqlError::syntax("CHECK expression is missing a closing parenthesis".into())
    })?;
    let inner = trim_separators(&text[1..close]);
    if inner.is_empty() {
        return Err(SqlError::syntax("CHECK requires an expression".into()));
    }
    crate::sql::definition_guard::expression_sql(inner)
        .map_err(|error| SqlError::unsupported(error.to_string()))?;
    super::parse_expression(inner)?;
    Ok((inner.to_owned(), &text[close + 1..]))
}

#[derive(Default)]
struct Clauses {
    default: Option<DomainDefaultDefinition>,
    not_null: bool,
    saw_null: bool,
    checks: Vec<DomainCheckDefinition>,
}

fn parse_clauses(mut rest: &str) -> Result<Clauses, SqlError> {
    let mut clauses = Clauses::default();
    loop {
        rest = trim_separators(rest);
        if rest.is_empty() {
            return Ok(clauses);
        }
        let mut constraint_name = None;
        if let Some(end) = pattern_end(rest, 0, "constraint") {
            let (token, tail) = split_first_token(trim_separators(&rest[end..]))
                .ok_or_else(|| SqlError::syntax("CONSTRAINT requires a name".into()))?;
            constraint_name = Some(parse_identifier(&token)?);
            rest = trim_separators(tail);
        }
        if let Some(end) = pattern_end(rest, 0, "check") {
            let (sql, tail) = parse_check_body(&rest[end..])?;
            clauses.checks.push(DomainCheckDefinition {
                name: constraint_name,
                sql,
            });
            rest = tail;
        } else if let Some(end) = pattern_end(rest, 0, "not null") {
            if clauses.saw_null {
                return Err(SqlError::syntax(
                    "conflicting NULL/NOT NULL constraints".into(),
                ));
            }
            clauses.not_null = true;
            rest = &rest[end..];
        } else if let Some(end) = pattern_end(rest, 0, "null") {
            if clauses.not_null {
                return Err(SqlError::syntax(
                    "conflicting NULL/NOT NULL constraints".into(),
                ));
            }
            clauses.saw_null = true;
            rest = &rest[end..];
        } else if constraint_name.is_some() {
            return Err(SqlError::syntax(
                "CONSTRAINT must be followed by CHECK or NOT NULL".into(),
            ));
        } else if let Some(end) = pattern_end(rest, 0, "default") {
            if clauses.default.is_some() {
                return Err(SqlError::syntax("multiple default values specified".into()));
            }
            let after = trim_separators(&rest[end..]);
            let stop = next_clause_start(after, first_operand_end(after)).unwrap_or(after.len());
            clauses.default = Some(parse_default(&after[..stop])?);
            rest = &after[stop..];
        } else if pattern_end(rest, 0, "collate").is_some() {
            return Err(SqlError::unsupported(
                "COLLATE on domains is not supported in this version".into(),
            ));
        } else {
            return Err(SqlError::syntax(format!(
                "unexpected text in CREATE DOMAIN near \"{}\"",
                rest.chars().take(24).collect::<String>()
            )));
        }
    }
}

pub(super) fn parse_create_domain_statement(sql: &str) -> Result<ParsedStatement, SqlError> {
    let trimmed = normalized(sql);
    let rest = after_keywords(trimmed, "create domain")?;
    let (name_token, after_name) = split_first_token(rest)
        .ok_or_else(|| SqlError::syntax("CREATE DOMAIN requires a name".into()))?;
    let name = parse_identifier(&name_token)?;
    if name.is_empty() {
        return Err(SqlError::syntax("CREATE DOMAIN requires a name".into()));
    }
    let mut after_name = trim_separators(after_name);
    if let Some(end) = pattern_end(after_name, 0, "as") {
        after_name = trim_separators(&after_name[end..]);
    }
    let type_end = next_clause_start(after_name, 0).unwrap_or(after_name.len());
    let type_text = trim_separators(&after_name[..type_end]);
    if type_text.is_empty() {
        return Err(SqlError::syntax(
            "CREATE DOMAIN requires a base type".into(),
        ));
    }
    let base = super::domain_scope::parse_declared_data_type(type_text)?;
    let clauses = parse_clauses(&after_name[type_end..])?;
    Ok(statement(
        trimmed,
        DomainAction::Create {
            name,
            base,
            default: clauses.default,
            not_null: clauses.not_null,
            checks: clauses.checks,
        },
    ))
}

pub(super) fn parse_drop_domain_statement(sql: &str) -> Result<ParsedStatement, SqlError> {
    let trimmed = normalized(sql);
    let rest = after_keywords(trimmed, "drop domain")?;
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
            return Err(SqlError::syntax(
                "DROP DOMAIN requires a domain name".into(),
            ));
        }
        names.push(name);
    }
    if names.is_empty() {
        return Err(SqlError::syntax(
            "DROP DOMAIN requires a domain name".into(),
        ));
    }
    Ok(statement(
        trimmed,
        DomainAction::Drop {
            names,
            if_exists,
            cascade,
        },
    ))
}

fn single_name(text: &str, context: &str) -> Result<String, SqlError> {
    let (token, tail) = split_first_token(trim_separators(text))
        .ok_or_else(|| SqlError::syntax(format!("{context} requires a name")))?;
    if !trim_separators(tail).is_empty() {
        return Err(SqlError::syntax(format!(
            "unexpected text after the name in {context}"
        )));
    }
    let name = parse_identifier(&token)?;
    if name.is_empty() {
        return Err(SqlError::syntax(format!("{context} requires a name")));
    }
    Ok(name)
}

fn parse_alteration(rest: &str) -> Result<DomainAlteration, SqlError> {
    let rest = trim_separators(rest);
    if let Some(end) = pattern_end(rest, 0, "set default") {
        return Ok(DomainAlteration::SetDefault(parse_default(&rest[end..])?));
    }
    if let Some(end) = pattern_end(rest, 0, "drop default") {
        return no_tail(&rest[end..], DomainAlteration::DropDefault);
    }
    if let Some(end) = pattern_end(rest, 0, "set not null") {
        return no_tail(&rest[end..], DomainAlteration::SetNotNull);
    }
    if let Some(end) = pattern_end(rest, 0, "drop not null") {
        return no_tail(&rest[end..], DomainAlteration::DropNotNull);
    }
    if let Some(end) = pattern_end(rest, 0, "add") {
        return parse_add_constraint(&rest[end..]);
    }
    if let Some(end) = pattern_end(rest, 0, "drop constraint") {
        return parse_drop_constraint(&rest[end..]);
    }
    if let Some(end) = pattern_end(rest, 0, "validate constraint") {
        return Ok(DomainAlteration::ValidateConstraint {
            name: single_name(&rest[end..], "VALIDATE CONSTRAINT")?,
        });
    }
    if let Some(end) = pattern_end(rest, 0, "rename constraint") {
        return parse_rename_constraint(&rest[end..]);
    }
    if let Some(end) = pattern_end(rest, 0, "rename to") {
        return Ok(DomainAlteration::RenameTo(single_name(
            &rest[end..],
            "RENAME TO",
        )?));
    }
    if pattern_end(rest, 0, "owner to").is_some() || pattern_end(rest, 0, "set schema").is_some() {
        return Err(SqlError::unsupported(
            "ALTER DOMAIN OWNER TO and SET SCHEMA are not supported in this version".into(),
        ));
    }
    Err(SqlError::syntax("unsupported ALTER DOMAIN action".into()))
}

fn no_tail(tail: &str, alteration: DomainAlteration) -> Result<DomainAlteration, SqlError> {
    if trim_separators(tail).is_empty() {
        Ok(alteration)
    } else {
        Err(SqlError::syntax(
            "unexpected text after ALTER DOMAIN action".into(),
        ))
    }
}

fn parse_add_constraint(rest: &str) -> Result<DomainAlteration, SqlError> {
    let mut rest = trim_separators(rest);
    let mut name = None;
    if let Some(end) = pattern_end(rest, 0, "constraint") {
        let (token, tail) = split_first_token(trim_separators(&rest[end..]))
            .ok_or_else(|| SqlError::syntax("CONSTRAINT requires a name".into()))?;
        name = Some(parse_identifier(&token)?);
        rest = trim_separators(tail);
    }
    let Some(end) = pattern_end(rest, 0, "check") else {
        return Err(SqlError::unsupported(
            "ALTER DOMAIN ADD supports only CHECK constraints in this version".into(),
        ));
    };
    let (sql, tail) = parse_check_body(&rest[end..])?;
    let tail = trim_separators(tail);
    let not_valid = if tail.is_empty() {
        false
    } else if pattern_end(tail, 0, "not valid").is_some_and(|end| end == tail.len()) {
        true
    } else {
        return Err(SqlError::syntax(
            "unexpected text after CHECK constraint".into(),
        ));
    };
    Ok(DomainAlteration::AddCheck {
        check: DomainCheckDefinition { name, sql },
        not_valid,
    })
}

fn parse_drop_constraint(rest: &str) -> Result<DomainAlteration, SqlError> {
    let (if_exists, rest) = parse_if_exists(trim_separators(rest));
    let lower = rest.to_ascii_lowercase();
    let name_text = lower
        .strip_suffix(" cascade")
        .or_else(|| lower.strip_suffix(" restrict"))
        .map_or(rest, |head| &rest[..head.len()]);
    Ok(DomainAlteration::DropConstraint {
        name: single_name(name_text, "DROP CONSTRAINT")?,
        if_exists,
    })
}

fn parse_rename_constraint(rest: &str) -> Result<DomainAlteration, SqlError> {
    let (from_token, tail) = split_first_token(trim_separators(rest))
        .ok_or_else(|| SqlError::syntax("RENAME CONSTRAINT requires a name".into()))?;
    let tail = trim_separators(tail);
    let end = pattern_end(tail, 0, "to")
        .ok_or_else(|| SqlError::syntax("RENAME CONSTRAINT requires TO".into()))?;
    Ok(DomainAlteration::RenameConstraint {
        from: parse_identifier(&from_token)?,
        to: single_name(&tail[end..], "RENAME CONSTRAINT")?,
    })
}

pub(super) fn parse_alter_domain_statement(sql: &str) -> Result<ParsedStatement, SqlError> {
    let trimmed = normalized(sql);
    let rest = after_keywords(trimmed, "alter domain")?;
    let (name_token, tail) = split_first_token(rest)
        .ok_or_else(|| SqlError::syntax("ALTER DOMAIN requires a name".into()))?;
    let name = parse_identifier(&name_token)?;
    if name.is_empty() {
        return Err(SqlError::syntax("ALTER DOMAIN requires a name".into()));
    }
    let alteration = parse_alteration(tail)?;
    Ok(statement(trimmed, DomainAction::Alter { name, alteration }))
}
