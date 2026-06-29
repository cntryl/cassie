use super::expr::{split_csv, parse_expr_token};
use super::schema::{parse_if_not_exists, starts_with_keyword, parse_if_exists, parse_constraint_literal, tokenize_schema_field, parse_data_type};
use super::{ParsedStatement, SqlError, parse_statement, QueryStatement, ExplainStatement, TransactionStatement, TransactionAction, TransactionIsolation, ShowStatement, SetStatement, CreateFunctionStatement, CreateProcedureStatement, find_top_level_keyword, CreateViewStatement, DropFunctionStatement, DropProcedureStatement, DropViewStatement, CallProcedureStatement, Value, FunctionArg, DataType, Volatility};

pub(super) fn parse_explain_statement(sql: &str) -> Result<ParsedStatement, SqlError> {
    let rest = sql["EXPLAIN".len()..].trim();
    if rest.is_empty() {
        return Err(SqlError("EXPLAIN requires a statement".to_string()));
    }

    let lower = rest.to_ascii_lowercase();
    let (analyze, inner_sql) = if lower.starts_with("analyze ") {
        (true, rest[7..].trim())
    } else {
        (false, rest)
    };
    if inner_sql.is_empty() {
        return Err(SqlError("EXPLAIN requires a statement".to_string()));
    }

    let statement = parse_statement(inner_sql)?;
    Ok(ParsedStatement {
        raw_sql: sql.to_string(),
        statement: QueryStatement::Explain(ExplainStatement {
            analyze,
            statement: Box::new(statement),
        }),
    })
}

pub(super) fn is_transaction_control_statement(lower: &str) -> bool {
    lower == "begin"
        || lower.starts_with("begin ")
        || lower == "start transaction"
        || lower.starts_with("start transaction ")
        || lower == "commit"
        || lower.starts_with("commit ")
        || lower == "rollback"
        || lower.starts_with("rollback ")
        || lower == "savepoint"
        || lower.starts_with("savepoint ")
        || lower == "release"
        || lower.starts_with("release ")
}

pub(super) fn unsupported_privilege_statement(lower: &str) -> Option<&'static str> {
    if lower == "grant" || lower.starts_with("grant ") {
        return Some("GRANT is not supported in this version");
    }
    if lower == "revoke" || lower.starts_with("revoke ") {
        return Some("REVOKE is not supported in this version");
    }
    if lower == "create policy" || lower.starts_with("create policy ") {
        return Some("ROW-LEVEL SECURITY policies are not supported in this version");
    }
    if lower == "alter policy" || lower.starts_with("alter policy ") {
        return Some("ROW-LEVEL SECURITY policies are not supported in this version");
    }
    if lower == "drop policy" || lower.starts_with("drop policy ") {
        return Some("ROW-LEVEL SECURITY policies are not supported in this version");
    }
    if lower == "alter table"
        || lower.starts_with("alter table ")
            && (lower.contains(" row level security") || lower.contains("row level security "))
    {
        return Some("ROW-LEVEL SECURITY is not supported in this version");
    }
    if lower == "set row level security" || lower.starts_with("set row level security ") {
        return Some("ROW-LEVEL SECURITY is not supported in this version");
    }
    None
}

pub(super) fn is_unsupported_transaction_control_statement(lower: &str) -> bool {
    lower == "prepare transaction"
        || lower.starts_with("prepare transaction ")
        || lower == "commit prepared"
        || lower.starts_with("commit prepared ")
        || lower == "rollback prepared"
        || lower.starts_with("rollback prepared ")
        || lower == "set transaction"
        || lower.starts_with("set transaction ")
}

pub(super) fn parse_transaction_statement(sql: &str) -> Result<ParsedStatement, SqlError> {
    let trimmed = sql.trim().trim_end_matches(';').trim();
    let tokens = trimmed
        .split_whitespace()
        .map(str::to_lowercase)
        .collect::<Vec<_>>();
    let token_refs = tokens.iter().map(String::as_str).collect::<Vec<_>>();

    let statement = match token_refs.as_slice() {
        ["begin", "isolation", "level", isolation @ ..] |
["begin" | "start", "transaction", "isolation", "level", isolation @ ..] => TransactionStatement {
            action: TransactionAction::Begin,
            isolation: Some(parse_transaction_isolation(isolation)?),
        },
        ["begin"] | ["begin" | "start", "transaction"] => TransactionStatement {
            action: TransactionAction::Begin,
            isolation: None,
        },
        ["commit"] | ["commit", "transaction"] => TransactionStatement {
            action: TransactionAction::Commit,
            isolation: None,
        },
        ["rollback"] | ["rollback", "transaction"] => TransactionStatement {
            action: TransactionAction::Rollback,
            isolation: None,
        },
        ["rollback", "to", name] | ["rollback", "to", "savepoint", name] => TransactionStatement {
            action: TransactionAction::RollbackTo {
                name: parse_savepoint_name(name, "ROLLBACK TO")?,
            },
            isolation: None,
        },
        ["rollback", "to", ..] => {
            return Err(SqlError(
                "ROLLBACK TO requires a savepoint name".to_string(),
            ));
        }
        ["savepoint", name] => TransactionStatement {
            action: TransactionAction::Savepoint {
                name: parse_savepoint_name(name, "SAVEPOINT")?,
            },
            isolation: None,
        },
        ["savepoint", ..] => {
            return Err(SqlError("SAVEPOINT requires a name".to_string()));
        }
        ["release", name] | ["release", "savepoint", name] => TransactionStatement {
            action: TransactionAction::Release {
                name: parse_savepoint_name(name, "RELEASE")?,
            },
            isolation: None,
        },
        ["release", ..] => {
            return Err(SqlError("RELEASE requires a savepoint name".to_string()));
        }
        _ => {
            return Err(SqlError(
                "unsupported transaction control statement".to_string(),
            ));
        }
    };

    Ok(ParsedStatement {
        raw_sql: trimmed.to_string(),
        statement: QueryStatement::Transaction(statement),
    })
}

pub(super) fn parse_savepoint_name(raw: &str, command: &str) -> Result<String, SqlError> {
    let raw_name = raw.trim();
    let name = if raw_name.starts_with('"') && raw_name.ends_with('"') && raw_name.len() >= 2 {
        &raw_name[1..raw_name.len() - 1]
    } else {
        raw_name
    };
    if name.is_empty() {
        return Err(SqlError(format!("{command} requires a savepoint name")));
    }
    if name.chars().any(|character| {
        !(character.is_ascii_alphanumeric() || character == '_' || character == '-')
    }) {
        return Err(SqlError(format!("invalid savepoint name '{name}'")));
    }
    Ok(name.to_ascii_lowercase())
}

pub(super) fn parse_transaction_isolation(
    tokens: &[&str],
) -> Result<TransactionIsolation, SqlError> {
    match tokens {
        ["read", "committed"] => Ok(TransactionIsolation::ReadCommitted),
        ["repeatable", "read"] => Ok(TransactionIsolation::RepeatableRead),
        ["serializable"] => Ok(TransactionIsolation::Serializable),
        _ => Err(SqlError(
            "unsupported transaction control statement".to_string(),
        )),
    }
}

pub(super) fn parse_show_statement(trimmed: &str) -> Result<ParsedStatement, SqlError> {
    let trimmed = trimmed.trim().trim_end_matches(';').trim();
    let argument = trimmed[4..].trim();
    if argument.is_empty() {
        return Err(SqlError("SHOW requires a parameter".into()));
    }

    Ok(ParsedStatement {
        raw_sql: trimmed.to_string(),
        statement: QueryStatement::Show(ShowStatement {
            variable: argument.to_string(),
        }),
    })
}

pub(super) fn normalize_set_value(value: &str) -> String {
    let trimmed = value.trim();
    if (trimmed.starts_with('\'') && trimmed.ends_with('\''))
        || (trimmed.starts_with('\"') && trimmed.ends_with('\"'))
    {
        return trimmed[1..trimmed.len() - 1].to_string();
    }
    trimmed.to_string()
}

pub(super) fn parse_set_statement(trimmed: &str) -> Result<ParsedStatement, SqlError> {
    let trimmed = trimmed.trim().trim_end_matches(';').trim();
    let argument = trimmed[3..].trim();
    if argument.is_empty() {
        return Err(SqlError("SET requires a parameter".into()));
    }

    let mut variable = argument;
    let mut value = None;

    if let Some((left, right)) = argument.split_once('=') {
        variable = left.trim();
        value = Some(normalize_set_value(right.trim()));
    } else if let Some(pos) = argument.to_lowercase().find(" to ") {
        variable = argument[..pos].trim();
        value = Some(normalize_set_value(argument[pos + 4..].trim()));
    }

    if variable.trim().is_empty() {
        return Err(SqlError("invalid SET statement".into()));
    }

    let variable_lower = variable.to_lowercase();
    if variable_lower == "role"
        || variable_lower.starts_with("role ")
        || variable_lower == "session authorization"
        || variable_lower.starts_with("session authorization ")
    {
        return Err(SqlError(
            "SET ROLE and SET SESSION AUTHORIZATION are not supported in this version".into(),
        ));
    }

    Ok(ParsedStatement {
        raw_sql: trimmed.to_string(),
        statement: QueryStatement::Set(SetStatement {
            variable: variable.to_string(),
            value,
        }),
    })
}

pub(super) fn parse_create_function_statement(sql: &str) -> Result<ParsedStatement, SqlError> {
    let trimmed = sql.trim().trim_end_matches(';').trim();
    let rest = trimmed[15..].trim();
    if rest.to_lowercase().contains("security definer") {
        return Err(SqlError(
            "SECURITY DEFINER is not supported in this version".into(),
        ));
    }

    let (if_not_exists, rest) = parse_if_not_exists(rest);

    let name_end = rest
        .find('(')
        .ok_or_else(|| SqlError("CREATE FUNCTION requires argument list".into()))?;
    let name = rest[..name_end].trim();
    if name.is_empty() {
        return Err(SqlError("CREATE FUNCTION requires a name".into()));
    }

    let rest = &rest[name_end + 1..];
    let close = rest
        .find(')')
        .ok_or_else(|| SqlError("CREATE FUNCTION argument list is missing ')'".into()))?;
    let args_raw = rest[..close].trim();

    let args = parse_function_args(args_raw)?;

    let mut remaining = rest[(close + 1)..].trim_start();
    if !starts_with_keyword(remaining, "returns") {
        return Err(SqlError("CREATE FUNCTION requires RETURNS clause".into()));
    }

    remaining = remaining[7..].trim();
    let (return_type_raw, remaining) = split_keyword(remaining, "as")
        .ok_or_else(|| SqlError("CREATE FUNCTION requires AS clause".into()))?;
    let (return_type, volatility) = parse_return_clause(return_type_raw)?;

    let body = parse_quoted_or_raw_body(remaining)?;

    Ok(ParsedStatement {
        raw_sql: trimmed.to_string(),
        statement: QueryStatement::CreateFunction(CreateFunctionStatement {
            name: name.to_string(),
            if_not_exists,
            args,
            return_type,
            volatility,
            body,
        }),
    })
}

pub(super) fn parse_create_procedure_statement(sql: &str) -> Result<ParsedStatement, SqlError> {
    let trimmed = sql.trim().trim_end_matches(';').trim();
    let rest = trimmed[16..].trim();
    if rest.to_lowercase().contains("security definer") {
        return Err(SqlError(
            "SECURITY DEFINER is not supported in this version".into(),
        ));
    }

    let (if_not_exists, rest) = parse_if_not_exists(rest);

    let name_end = rest
        .find('(')
        .ok_or_else(|| SqlError("CREATE PROCEDURE requires argument list".into()))?;
    let name = rest[..name_end].trim();
    if name.is_empty() {
        return Err(SqlError("CREATE PROCEDURE requires a name".into()));
    }

    let rest = &rest[name_end + 1..];
    let close = rest
        .find(')')
        .ok_or_else(|| SqlError("CREATE PROCEDURE argument list is missing ')'".into()))?;
    let args_raw = rest[..close].trim();

    let args = parse_function_args(args_raw)?;
    let mut remaining = rest[(close + 1)..].trim_start();

    if !starts_with_keyword(remaining, "as") {
        return Err(SqlError("CREATE PROCEDURE requires AS clause".into()));
    }
    remaining = remaining[2..].trim();

    let body = parse_quoted_or_raw_body(remaining)?;

    Ok(ParsedStatement {
        raw_sql: trimmed.to_string(),
        statement: QueryStatement::CreateProcedure(CreateProcedureStatement {
            name: name.to_string(),
            if_not_exists,
            args,
            body,
        }),
    })
}

pub(super) fn parse_create_view_statement(sql: &str) -> Result<ParsedStatement, SqlError> {
    let trimmed = sql.trim().trim_end_matches(';').trim();
    let rest = trimmed[11..].trim();
    let (if_not_exists, rest) = parse_if_not_exists(rest);

    let as_pos = find_top_level_keyword(rest, 0, "as")
        .ok_or_else(|| SqlError("CREATE VIEW requires AS clause".into()))?;
    let name = rest[..as_pos].trim();
    if name.is_empty() {
        return Err(SqlError("CREATE VIEW requires a name".into()));
    }
    if name.split_whitespace().count() != 1 {
        return Err(SqlError("CREATE VIEW supports only one view name".into()));
    }

    let body = rest[as_pos + 2..].trim();
    if body.is_empty() {
        return Err(SqlError("CREATE VIEW requires a query body".into()));
    }

    Ok(ParsedStatement {
        raw_sql: trimmed.to_string(),
        statement: QueryStatement::CreateView(CreateViewStatement {
            name: name.to_string(),
            if_not_exists,
            query: body.to_string(),
        }),
    })
}

pub(super) fn parse_drop_function_statement(sql: &str) -> Result<ParsedStatement, SqlError> {
    let trimmed = sql.trim().trim_end_matches(';').trim();
    let rest = trimmed[14..].trim();
    let (if_exists, rest) = parse_if_exists(rest);

    if rest.is_empty() {
        return Err(SqlError("missing function name for DROP FUNCTION".into()));
    }

    Ok(ParsedStatement {
        raw_sql: trimmed.to_string(),
        statement: QueryStatement::DropFunction(DropFunctionStatement {
            name: rest.to_string(),
            if_exists,
        }),
    })
}

pub(super) fn parse_drop_procedure_statement(sql: &str) -> Result<ParsedStatement, SqlError> {
    let trimmed = sql.trim().trim_end_matches(';').trim();
    let rest = trimmed[15..].trim();
    let (if_exists, rest) = parse_if_exists(rest);

    if rest.is_empty() {
        return Err(SqlError("missing procedure name for DROP PROCEDURE".into()));
    }

    Ok(ParsedStatement {
        raw_sql: trimmed.to_string(),
        statement: QueryStatement::DropProcedure(DropProcedureStatement {
            name: rest.to_string(),
            if_exists,
        }),
    })
}

pub(super) fn parse_drop_view_statement(sql: &str) -> Result<ParsedStatement, SqlError> {
    let trimmed = sql.trim().trim_end_matches(';').trim();
    let rest = trimmed[10..].trim();
    let (if_exists, rest) = parse_if_exists(rest);

    if rest.is_empty() {
        return Err(SqlError("missing view name for DROP VIEW".into()));
    }

    if rest.split_whitespace().count() != 1 {
        return Err(SqlError("DROP VIEW supports only one view name".into()));
    }

    Ok(ParsedStatement {
        raw_sql: trimmed.to_string(),
        statement: QueryStatement::DropView(DropViewStatement {
            name: rest.to_string(),
            if_exists,
        }),
    })
}

pub(super) fn parse_call_statement(sql: &str) -> Result<ParsedStatement, SqlError> {
    let trimmed = sql.trim().trim_end_matches(';').trim();
    let rest = trimmed[4..].trim();

    let open = rest
        .find('(')
        .ok_or_else(|| SqlError("CALL requires argument list".into()))?;
    let close = rest
        .rfind(')')
        .ok_or_else(|| SqlError("CALL argument list is missing ')'".into()))?;
    if close < open {
        return Err(SqlError("invalid CALL syntax".into()));
    }

    let name = rest[..open].trim();
    if name.is_empty() {
        return Err(SqlError("CALL requires a procedure name".into()));
    }

    let args_raw = rest[(open + 1)..close].trim();
    let args = if args_raw.is_empty() {
        Vec::new()
    } else {
        split_csv(args_raw)
            .into_iter()
            .map(parse_expr_token)
            .collect::<Result<Vec<_>, _>>()?
    };

    Ok(ParsedStatement {
        raw_sql: trimmed.to_string(),
        statement: QueryStatement::CallProcedure(CallProcedureStatement {
            name: name.to_string(),
            args,
        }),
    })
}

pub(super) fn parse_optional_role_password(raw: &str) -> Result<Option<String>, SqlError> {
    if raw.eq_ignore_ascii_case("null") {
        return Ok(None);
    }

    let value = parse_constraint_literal(raw)?;
    match value {
        Value::String(password) => Ok(Some(password)),
        Value::Null => Ok(None),
        other => Ok(Some(other.to_string())),
    }
}

pub(super) fn parse_function_args(raw: &str) -> Result<Vec<FunctionArg>, SqlError> {
    if raw.trim().is_empty() {
        return Ok(Vec::new());
    }

    let mut args = Vec::new();
    for token in split_csv(raw) {
        let token = token.trim();
        if token.is_empty() {
            continue;
        }
        let parts = tokenize_schema_field(token);
        if parts.is_empty() {
            return Err(SqlError("invalid function argument".into()));
        }

        let name = parts[0].trim();
        if name.is_empty() || name.contains(',') {
            return Err(SqlError("invalid function argument".into()));
        }

        if parts.len() < 2 {
            return Err(SqlError(format!(
                "missing data type for function argument '{name}'"
            )));
        }
        let data_type = parse_data_type(parts[1].as_str())?;

        args.push(FunctionArg {
            name: name.to_string(),
            data_type,
        });
    }

    Ok(args)
}

pub(super) fn parse_return_clause(raw: &str) -> Result<(DataType, Volatility), SqlError> {
    let tokens = raw.split_whitespace().collect::<Vec<_>>();
    if tokens.is_empty() {
        return Err(SqlError(
            "CREATE FUNCTION RETURNS clause is missing a type".into(),
        ));
    }

    let data_type = parse_data_type(tokens[0])?;
    let volatility = if tokens.len() > 1 {
        match tokens[1].to_lowercase().as_str() {
            "immutable" => {
                if tokens.len() > 2 {
                    return Err(SqlError(
                        "unexpected token after function volatility".into(),
                    ));
                }
                Volatility::Immutable
            }
            "stable" => {
                if tokens.len() > 2 {
                    return Err(SqlError(
                        "unexpected token after function volatility".into(),
                    ));
                }
                Volatility::Stable
            }
            "volatile" => {
                if tokens.len() > 2 {
                    return Err(SqlError(
                        "unexpected token after function volatility".into(),
                    ));
                }
                Volatility::Volatile
            }
            _ => {
                return Err(SqlError(format!(
                    "unsupported function volatility '{}'; use IMMUTABLE/STABLE/VOLATILE",
                    tokens[1]
                )));
            }
        }
    } else {
        Volatility::Immutable
    };

    if tokens.len() > 2 {
        return Err(SqlError("unexpected token after RETURNS clause".into()));
    }

    Ok((data_type, volatility))
}

pub(super) fn parse_quoted_or_raw_body(raw: &str) -> Result<String, SqlError> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err(SqlError("empty function/procedure body".into()));
    }

    if let Ok((value, remainder)) = parse_sql_quoted_string(raw) {
        if remainder.trim().is_empty() {
            return Ok(value);
        }
    }

    Ok(raw.to_string())
}

pub(super) fn parse_sql_quoted_string(raw: &str) -> Result<(String, &str), SqlError> {
    if raw.starts_with('"') && raw.ends_with('"') && raw.len() >= 2 {
        return Ok((raw[1..raw.len() - 1].to_string(), ""));
    }

    if raw.starts_with('\'') && raw.ends_with('\'') && raw.len() >= 2 {
        return Ok((raw[1..raw.len() - 1].to_string(), ""));
    }

    Err(SqlError("not a quoted string".into()))
}

pub(super) fn split_keyword<'a>(raw: &'a str, keyword: &'a str) -> Option<(&'a str, &'a str)> {
    let lower = raw.to_lowercase();
    let keyword_len = keyword.len();
    let idx = lower.find(&format!(" {keyword} "))?;

    let before = raw[..idx].trim();
    let pattern_len = keyword_len + 2;
    let after = raw[idx + pattern_len..].trim_start();
    if after.is_empty() {
        None
    } else {
        Some((before, after))
    }
}
