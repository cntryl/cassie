//! Transaction grammar retains characteristics through execution and chaining.

use crate::sql::ast::{
    ParsedStatement, QueryStatement, TransactionAction, TransactionIsolation, TransactionModes,
    TransactionStatement,
};
use crate::sql::parser::SqlError;

pub(in crate::sql::parser) fn parse_transaction_statement(
    sql: &str,
) -> Result<ParsedStatement, SqlError> {
    let trimmed = sql.trim().trim_end_matches(';').trim();
    let normalized = trimmed.replace(',', " , ").to_ascii_lowercase();
    let tokens = normalized.split_whitespace().collect::<Vec<_>>();
    let mut statement = TransactionStatement {
        action: TransactionAction::Begin,
        isolation: None,
        modes: TransactionModes::default(),
        chain: false,
    };
    let remaining = match tokens.as_slice() {
        ["begin", rest @ ..] => rest
            .strip_prefix(&["work"])
            .or_else(|| rest.strip_prefix(&["transaction"]))
            .unwrap_or(rest),
        ["start", "transaction", rest @ ..] => rest,
        ["commit" | "end", rest @ ..] => {
            statement.action = TransactionAction::Commit;
            parse_end_options(rest, &mut statement)?;
            return Ok(parsed(trimmed, statement));
        }
        ["rollback", "work" | "transaction", "to", rest @ ..] | ["rollback", "to", rest @ ..] => {
            let rest = rest.strip_prefix(&["savepoint"]).unwrap_or(rest);
            let [name] = rest else {
                return Err(SqlError::syntax(
                    "ROLLBACK TO requires a savepoint name".into(),
                ));
            };
            statement.action = TransactionAction::RollbackTo {
                name: parse_savepoint_name(name, "ROLLBACK TO")?,
            };
            return Ok(parsed(trimmed, statement));
        }
        ["rollback" | "abort", rest @ ..] => {
            statement.action = TransactionAction::Rollback;
            parse_end_options(rest, &mut statement)?;
            return Ok(parsed(trimmed, statement));
        }
        ["savepoint", name] => {
            statement.action = TransactionAction::Savepoint {
                name: parse_savepoint_name(name, "SAVEPOINT")?,
            };
            return Ok(parsed(trimmed, statement));
        }
        ["release", name] | ["release", "savepoint", name] => {
            statement.action = TransactionAction::Release {
                name: parse_savepoint_name(name, "RELEASE")?,
            };
            return Ok(parsed(trimmed, statement));
        }
        ["savepoint", ..] => return Err(SqlError::syntax("SAVEPOINT requires a name".into())),
        ["release", ..] => {
            return Err(SqlError::syntax("RELEASE requires a savepoint name".into()))
        }
        _ => return Err(invalid_transaction()),
    };
    parse_modes(remaining, &mut statement)?;
    Ok(parsed(trimmed, statement))
}

fn parsed(sql: &str, statement: TransactionStatement) -> ParsedStatement {
    ParsedStatement {
        raw_sql: sql.to_string(),
        statement: QueryStatement::Transaction(statement),
    }
}

fn parse_end_options(
    tokens: &[&str],
    statement: &mut TransactionStatement,
) -> Result<(), SqlError> {
    let tokens = tokens
        .strip_prefix(&["work"])
        .or_else(|| tokens.strip_prefix(&["transaction"]))
        .unwrap_or(tokens);
    statement.chain = match tokens {
        [] | ["and", "no", "chain"] => false,
        ["and", "chain"] => true,
        _ => return Err(invalid_transaction()),
    };
    Ok(())
}

fn parse_modes(mut tokens: &[&str], statement: &mut TransactionStatement) -> Result<(), SqlError> {
    while !tokens.is_empty() {
        let consumed = match tokens {
            ["isolation", "level", "read", "uncommitted", ..] => {
                statement.isolation = Some(TransactionIsolation::ReadUncommitted);
                4
            }
            ["isolation", "level", "read", "committed", ..] => {
                statement.isolation = Some(TransactionIsolation::ReadCommitted);
                4
            }
            ["isolation", "level", "repeatable", "read", ..] => {
                statement.isolation = Some(TransactionIsolation::RepeatableRead);
                4
            }
            ["isolation", "level", "serializable", ..] => {
                statement.isolation = Some(TransactionIsolation::Serializable);
                3
            }
            ["read", "only", ..] => {
                statement.modes.read_only = Some(true);
                2
            }
            ["read", "write", ..] => {
                statement.modes.read_only = Some(false);
                2
            }
            ["not", "deferrable", ..] => {
                statement.modes.deferrable = Some(false);
                2
            }
            ["deferrable", ..] => {
                statement.modes.deferrable = Some(true);
                1
            }
            _ => return Err(invalid_transaction()),
        };
        tokens = &tokens[consumed..];
        if tokens.first() == Some(&",") {
            tokens = &tokens[1..];
            if tokens.is_empty() {
                return Err(invalid_transaction());
            }
        }
    }
    Ok(())
}

fn invalid_transaction() -> SqlError {
    SqlError::syntax("unsupported transaction control statement".into())
}

fn parse_savepoint_name(raw: &str, command: &str) -> Result<String, SqlError> {
    let name = raw
        .strip_prefix('"')
        .and_then(|raw| raw.strip_suffix('"'))
        .unwrap_or(raw);
    if name.is_empty() {
        return Err(SqlError::syntax(format!(
            "{command} requires a savepoint name"
        )));
    }
    if name.chars().any(|character| {
        !(character.is_ascii_alphanumeric() || character == '_' || character == '-')
    }) {
        return Err(SqlError::syntax(format!("invalid savepoint name '{name}'")));
    }
    Ok(name.to_ascii_lowercase())
}
