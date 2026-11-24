//! Sequence option clauses shared by `CREATE SEQUENCE`, `ALTER SEQUENCE` and
//! identity columns.
use super::schema_identifiers::{parse_column_reference, parse_identifier};
use super::{DataType, SqlError};
use crate::catalog::{SequenceOptions, SequencePersistence};
use crate::sql::ast::SequenceOwnedBy;

/// Parsed clauses; `restart`, `owned_by` and `persistence` only appear where
/// the caller allows them.
#[derive(Debug, Default)]
pub(super) struct SequenceClauses {
    pub(super) options: SequenceOptions,
    pub(super) owned_by: Option<SequenceOwnedBy>,
    pub(super) persistence: Option<SequencePersistence>,
}

/// Which clauses a statement accepts.
#[derive(Debug, Clone, Copy)]
pub(super) struct SequenceClauseRules {
    pub(super) allow_restart: bool,
    pub(super) allow_owned_by: bool,
    pub(super) allow_persistence: bool,
    pub(super) statement: &'static str,
}

pub(super) fn parse_sequence_clauses(
    text: &str,
    rules: SequenceClauseRules,
) -> Result<SequenceClauses, SqlError> {
    let tokens = text.split_whitespace().collect::<Vec<_>>();
    let mut clauses = SequenceClauses::default();
    let mut index = 0;
    while index < tokens.len() {
        let keyword = tokens[index].to_ascii_lowercase();
        index += 1;
        match keyword.as_str() {
            "as" => {
                let name = take(&tokens, &mut index, rules, "AS")?;
                let data_type = DataType::parse_sql(name).map_err(SqlError::new)?;
                clauses.options.data_type = Some(data_type);
            }
            "increment" => {
                skip_keyword(&tokens, &mut index, "by");
                clauses.options.increment = Some(number(&tokens, &mut index, rules, "INCREMENT")?);
            }
            "minvalue" => {
                clauses.options.min_value =
                    Some(Some(number(&tokens, &mut index, rules, "MINVALUE")?));
            }
            "maxvalue" => {
                clauses.options.max_value =
                    Some(Some(number(&tokens, &mut index, rules, "MAXVALUE")?));
            }
            "start" => {
                skip_keyword(&tokens, &mut index, "with");
                clauses.options.start = Some(number(&tokens, &mut index, rules, "START")?);
            }
            "restart" if rules.allow_restart => {
                let explicit = skip_keyword(&tokens, &mut index, "with");
                let value = if explicit || tokens.get(index).is_some_and(|token| is_number(token)) {
                    Some(number(&tokens, &mut index, rules, "RESTART")?)
                } else {
                    None
                };
                clauses.options.restart = Some(value);
            }
            "cache" => {
                clauses.options.cache = Some(number(&tokens, &mut index, rules, "CACHE")?);
            }
            "cycle" => clauses.options.cycle = Some(true),
            "no" => {
                let next = take(&tokens, &mut index, rules, "NO")?.to_ascii_lowercase();
                match next.as_str() {
                    "minvalue" => clauses.options.min_value = Some(None),
                    "maxvalue" => clauses.options.max_value = Some(None),
                    "cycle" => clauses.options.cycle = Some(false),
                    _ => return Err(unsupported(rules, &format!("NO {}", next.to_uppercase()))),
                }
            }
            "owned" if rules.allow_owned_by => {
                if !skip_keyword(&tokens, &mut index, "by") {
                    return Err(SqlError::new("OWNED requires BY".into()));
                }
                let target = take(&tokens, &mut index, rules, "OWNED BY")?;
                clauses.owned_by = Some(owned_by(target)?);
            }
            "set" if rules.allow_persistence => {
                let next = take(&tokens, &mut index, rules, "SET")?.to_ascii_lowercase();
                clauses.persistence = Some(match next.as_str() {
                    "logged" => SequencePersistence::Permanent,
                    "unlogged" => SequencePersistence::Unlogged,
                    _ => return Err(unsupported(rules, &format!("SET {}", next.to_uppercase()))),
                });
            }
            _ => return Err(unsupported(rules, &keyword.to_uppercase())),
        }
    }
    Ok(clauses)
}

fn owned_by(target: &str) -> Result<SequenceOwnedBy, SqlError> {
    if target.eq_ignore_ascii_case("none") {
        return Ok(SequenceOwnedBy::None);
    }
    let Some((table, column)) = target.rsplit_once('.') else {
        return Err(SqlError::new(
            "invalid OWNED BY option: specify OWNED BY table.column or OWNED BY NONE".into(),
        ));
    };
    Ok(SequenceOwnedBy::Column {
        table: parse_identifier(table)?,
        column: parse_column_reference(column)?,
    })
}

fn take<'a>(
    tokens: &[&'a str],
    index: &mut usize,
    rules: SequenceClauseRules,
    clause: &str,
) -> Result<&'a str, SqlError> {
    let token = tokens
        .get(*index)
        .ok_or_else(|| SqlError::new(format!("{} {clause} requires a value", rules.statement)))?;
    *index += 1;
    Ok(token)
}

fn skip_keyword(tokens: &[&str], index: &mut usize, keyword: &str) -> bool {
    if tokens
        .get(*index)
        .is_some_and(|token| token.eq_ignore_ascii_case(keyword))
    {
        *index += 1;
        true
    } else {
        false
    }
}

fn is_number(token: &str) -> bool {
    let digits = token.strip_prefix(['-', '+']).unwrap_or(token);
    !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
}

fn number(
    tokens: &[&str],
    index: &mut usize,
    rules: SequenceClauseRules,
    clause: &str,
) -> Result<i64, SqlError> {
    let token = take(tokens, index, rules, clause)?;
    token.parse::<i64>().map_err(|_| {
        SqlError::new(format!(
            "{} {clause} expects an integer, found '{token}'",
            rules.statement
        ))
    })
}

fn unsupported(rules: SequenceClauseRules, clause: &str) -> SqlError {
    SqlError::new(format!("unsupported {} option '{clause}'", rules.statement))
}
