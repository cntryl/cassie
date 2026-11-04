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

pub(crate) fn parse_terminal_transaction_control(sql: &str) -> Option<TransactionStatement> {
    let trimmed = sql.trim().trim_end_matches(';').trim();
    let mut tokens = trimmed.split_whitespace();
    let first = tokens.next()?;
    let action = if first.eq_ignore_ascii_case("rollback") || first.eq_ignore_ascii_case("abort") {
        TransactionAction::Rollback
    } else if first.eq_ignore_ascii_case("commit") || first.eq_ignore_ascii_case("end") {
        TransactionAction::Commit
    } else {
        return None;
    };
    Some(TransactionStatement {
        action,
        isolation: None,
        modes: TransactionModes::default(),
        chain: parse_end_tokens(tokens)?,
    })
}

fn parse_end_options(
    tokens: &[&str],
    statement: &mut TransactionStatement,
) -> Result<(), SqlError> {
    statement.chain = parse_end_tokens(tokens.iter().copied()).ok_or_else(invalid_transaction)?;
    Ok(())
}

fn parse_end_tokens<'a>(mut tokens: impl Iterator<Item = &'a str>) -> Option<bool> {
    let first = tokens.next();
    let first = if first.is_some_and(|token| {
        token.eq_ignore_ascii_case("work") || token.eq_ignore_ascii_case("transaction")
    }) {
        tokens.next()
    } else {
        first
    };
    let chain = match first {
        None => false,
        Some(token) if token.eq_ignore_ascii_case("and") => {
            let option = tokens.next()?;
            if option.eq_ignore_ascii_case("chain") {
                true
            } else if option.eq_ignore_ascii_case("no")
                && tokens.next()?.eq_ignore_ascii_case("chain")
            {
                false
            } else {
                return None;
            }
        }
        Some(_) => return None,
    };
    tokens.next().is_none().then_some(chain)
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

#[cfg(test)]
mod cleanup_tests {
    use super::*;

    #[test]
    fn should_share_terminal_cleanup_grammar_with_normal_transaction_parsing() {
        // Arrange
        let cases = [
            "ROLLBACK",
            "abort WORK",
            "ROLLBACK TRANSACTION AND NO CHAIN;",
            "COMMIT",
            "END TRANSACTION AND CHAIN",
        ];
        for sql in cases {
            // Act
            let terminal = parse_terminal_transaction_control(sql).expect("terminal control");
            let parsed = parse_transaction_statement(sql).expect("normal transaction parser");
            let QueryStatement::Transaction(normal) = parsed.statement else {
                panic!("transaction");
            };

            // Assert
            assert_eq!(terminal.action, normal.action);
            assert_eq!(terminal.chain, normal.chain);
        }
    }

    #[test]
    fn should_reject_nonterminal_commands_from_quarantine_cleanup() {
        // Arrange
        let cases = [
            "ROLLBACK TO SAVEPOINT s",
            "COMMIT; SELECT 1",
            "BEGIN",
            "ROLLBACK AND invalid",
            "SELECT 1",
        ];
        for sql in cases {
            // Act
            let terminal = parse_terminal_transaction_control(sql);

            // Assert
            assert!(terminal.is_none(), "{sql}");
        }
    }
}
