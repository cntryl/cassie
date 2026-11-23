#[path = "statements/explain_options.rs"]
mod explain_options;
#[path = "statements/session_objects.rs"]
mod session_objects;
pub(super) use session_objects::{SESSION_OBJECT_KEYWORDS, parse_session_object_statement};
#[path = "statements/settings.rs"]
mod settings;
#[path = "statements/transaction.rs"]
mod transaction;
pub(super) use settings::{parse_reset_statement, parse_set_statement, parse_show_statement};
pub(crate) use transaction::parse_terminal_transaction_control;
pub(super) use transaction::parse_transaction_statement;

use super::expr::{parse_expr_token, split_csv};
use super::schema::{
    parse_constraint_literal, parse_data_type, parse_if_exists, parse_if_not_exists,
    starts_with_keyword, tokenize_schema_field,
};
use super::{
    CallProcedureStatement, CreateFunctionStatement, CreateProcedureStatement, CreateViewStatement,
    DataType, DropFunctionStatement, DropProcedureStatement, DropViewStatement, ExplainStatement,
    FunctionArg, ParsedStatement, QueryStatement, SqlError, Value, Volatility,
    find_top_level_keyword, parse_statement,
};

pub(super) fn parse_explain_statement(sql: &str) -> Result<ParsedStatement, SqlError> {
    let rest = sql["EXPLAIN".len()..].trim();
    if rest.is_empty() {
        return Err(SqlError::new("EXPLAIN requires a statement".to_string()));
    }

    let explain_options::ExplainPrefix {
        analyze,
        statement: inner_sql,
    } = explain_options::parse_explain_prefix(rest)?;
    if inner_sql.is_empty() {
        return Err(SqlError::new("EXPLAIN requires a statement".to_string()));
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
        || lower == "end"
        || lower.starts_with("end ")
        || lower == "rollback"
        || lower.starts_with("rollback ")
        || lower == "abort"
        || lower.starts_with("abort ")
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
    if lower == "create collation" || lower.starts_with("create collation ") {
        return Some("CREATE COLLATION is not supported in this version");
    }
    if lower == "alter collation" || lower.starts_with("alter collation ") {
        return Some("ALTER COLLATION is not supported in this version");
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
    let uncommented = super::lexical::without_comments(lower);
    let normalized = uncommented.trim();
    let is_set_transaction = normalized
        .split_whitespace()
        .take(2)
        .eq(["set", "transaction"]);
    let is_selected_set_transaction = normalized.split_whitespace().eq([
        "set",
        "transaction",
        "isolation",
        "level",
        "repeatable",
        "read",
    ]);

    normalized == "prepare transaction"
        || normalized.starts_with("prepare transaction ")
        || normalized == "commit prepared"
        || normalized.starts_with("commit prepared ")
        || normalized == "rollback prepared"
        || normalized.starts_with("rollback prepared ")
        || (is_set_transaction && !is_selected_set_transaction)
}

/// Splits `CREATE [OR REPLACE] FUNCTION|PROCEDURE` from the rest of the
/// statement, reporting whether `OR REPLACE` was written.
fn routine_clause_start<'a>(trimmed: &'a str, noun: &str) -> (bool, &'a str) {
    let rest = strip_word(trimmed, "create").unwrap_or(trimmed);
    let replaced = strip_word(rest, "or").and_then(|after_or| strip_word(after_or, "replace"));
    let rest = replaced.unwrap_or(rest);
    (
        replaced.is_some(),
        strip_word(rest, noun).unwrap_or(rest).trim(),
    )
}

/// The text after a leading case-insensitive `word` and its whitespace.
fn strip_word<'a>(text: &'a str, word: &str) -> Option<&'a str> {
    let text = text.trim_start();
    let head = text.get(..word.len())?;
    let tail = &text[word.len()..];
    (head.eq_ignore_ascii_case(word) && tail.starts_with(char::is_whitespace))
        .then(|| tail.trim_start())
}

pub(super) fn parse_create_function_statement(sql: &str) -> Result<ParsedStatement, SqlError> {
    let trimmed = sql.trim().trim_end_matches(';').trim();
    let (or_replace, rest) = routine_clause_start(trimmed, "function");
    if rest.to_lowercase().contains("security definer") {
        return Err(SqlError::new(
            "SECURITY DEFINER is not supported in this version".into(),
        ));
    }

    let (if_not_exists, rest) = parse_if_not_exists(rest);

    let name_end = rest
        .find('(')
        .ok_or_else(|| SqlError::new("CREATE FUNCTION requires argument list".into()))?;
    let name = rest[..name_end].trim();
    if name.is_empty() {
        return Err(SqlError::new("CREATE FUNCTION requires a name".into()));
    }

    let rest = &rest[name_end + 1..];
    let close = rest
        .find(')')
        .ok_or_else(|| SqlError::new("CREATE FUNCTION argument list is missing ')'".into()))?;
    let args_raw = rest[..close].trim();

    let args = parse_function_args(args_raw)?;

    let mut remaining = rest[(close + 1)..].trim_start();
    if !starts_with_keyword(remaining, "returns") {
        return Err(SqlError::new(
            "CREATE FUNCTION requires RETURNS clause".into(),
        ));
    }

    remaining = remaining[7..].trim();
    let (return_type_raw, remaining) = split_keyword(remaining, "as")
        .ok_or_else(|| SqlError::new("CREATE FUNCTION requires AS clause".into()))?;
    let (return_type, volatility) = parse_return_clause(return_type_raw)?;

    let body = parse_function_body(remaining)?;

    Ok(ParsedStatement {
        raw_sql: trimmed.to_string(),
        statement: QueryStatement::CreateFunction(CreateFunctionStatement {
            name: name.to_string(),
            if_not_exists,
            or_replace,
            args,
            return_type,
            volatility,
            body,
        }),
    })
}

pub(super) fn parse_create_procedure_statement(sql: &str) -> Result<ParsedStatement, SqlError> {
    let trimmed = sql.trim().trim_end_matches(';').trim();
    let (or_replace, rest) = routine_clause_start(trimmed, "procedure");
    if rest.to_lowercase().contains("security definer") {
        return Err(SqlError::new(
            "SECURITY DEFINER is not supported in this version".into(),
        ));
    }

    let (if_not_exists, rest) = parse_if_not_exists(rest);

    let name_end = rest
        .find('(')
        .ok_or_else(|| SqlError::new("CREATE PROCEDURE requires argument list".into()))?;
    let name = rest[..name_end].trim();
    if name.is_empty() {
        return Err(SqlError::new("CREATE PROCEDURE requires a name".into()));
    }

    let rest = &rest[name_end + 1..];
    let close = rest
        .find(')')
        .ok_or_else(|| SqlError::new("CREATE PROCEDURE argument list is missing ')'".into()))?;
    let args_raw = rest[..close].trim();

    let args = parse_function_args(args_raw)?;
    let mut remaining = rest[(close + 1)..].trim_start();

    if !starts_with_keyword(remaining, "as") {
        return Err(SqlError::new("CREATE PROCEDURE requires AS clause".into()));
    }
    remaining = remaining[2..].trim();

    let body = parse_quoted_or_raw_body(remaining)?;

    Ok(ParsedStatement {
        raw_sql: trimmed.to_string(),
        statement: QueryStatement::CreateProcedure(CreateProcedureStatement {
            name: name.to_string(),
            if_not_exists,
            or_replace,
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
        .ok_or_else(|| SqlError::new("CREATE VIEW requires AS clause".into()))?;
    let name = rest[..as_pos].trim();
    if name.is_empty() {
        return Err(SqlError::new("CREATE VIEW requires a name".into()));
    }
    if name.split_whitespace().count() != 1 {
        return Err(SqlError::new(
            "CREATE VIEW supports only one view name".into(),
        ));
    }

    let body = rest[as_pos + 2..].trim();
    if body.is_empty() {
        return Err(SqlError::new("CREATE VIEW requires a query body".into()));
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
        return Err(SqlError::new(
            "missing function name for DROP FUNCTION".into(),
        ));
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
        return Err(SqlError::new(
            "missing procedure name for DROP PROCEDURE".into(),
        ));
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
    // RESTRICT is the default behavior; the option is never part of a name.
    let (cascade, rest) = match super::lexical::strip_keyword_suffix(rest, "cascade") {
        Some(rest) => (true, rest),
        None => (
            false,
            super::lexical::strip_keyword_suffix(rest, "restrict").unwrap_or(rest),
        ),
    };

    let names = split_csv(rest);
    if names.is_empty() || names.iter().any(|name| name.trim().is_empty()) {
        return Err(SqlError::new("missing view name for DROP VIEW".into()));
    }
    let mut names = names.into_iter().map(|name| name.trim().to_owned());
    let name = names.next().expect("validated DROP VIEW target list");

    Ok(ParsedStatement {
        raw_sql: trimmed.to_string(),
        statement: QueryStatement::DropView(DropViewStatement {
            name,
            additional_names: names.collect(),
            if_exists,
            cascade,
        }),
    })
}

pub(super) fn parse_call_statement(sql: &str) -> Result<ParsedStatement, SqlError> {
    let trimmed = sql.trim().trim_end_matches(';').trim();
    let rest = trimmed[4..].trim();

    let open = rest
        .find('(')
        .ok_or_else(|| SqlError::new("CALL requires argument list".into()))?;
    let close = rest
        .rfind(')')
        .ok_or_else(|| SqlError::new("CALL argument list is missing ')'".into()))?;
    if close < open {
        return Err(SqlError::new("invalid CALL syntax".into()));
    }

    let name = rest[..open].trim();
    if name.is_empty() {
        return Err(SqlError::new("CALL requires a procedure name".into()));
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
            return Err(SqlError::new("invalid function argument".into()));
        }

        let name = parts[0].trim();
        if name.is_empty() || name.contains(',') {
            return Err(SqlError::new("invalid function argument".into()));
        }

        if parts.len() < 2 {
            return Err(SqlError::new(format!(
                "missing data type for function argument '{name}'"
            )));
        }
        // A routine argument may be declared with a domain or enum type as
        // well as a built-in or composite one.
        let type_text = parts[1..].join(" ");
        let data_type = super::domain_scope::parse_declared_data_type(&type_text)?;

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
        return Err(SqlError::new(
            "CREATE FUNCTION RETURNS clause is missing a type".into(),
        ));
    }

    let (type_tokens, volatility) = match tokens.last().map(|token| token.to_ascii_lowercase()) {
        Some(last) if matches!(last.as_str(), "immutable" | "stable" | "volatile") => {
            let type_tokens = &tokens[..tokens.len() - 1];
            if type_tokens.is_empty() {
                return Err(SqlError::new(
                    "CREATE FUNCTION RETURNS clause is missing a type".into(),
                ));
            }
            let volatility = match last.as_str() {
                "immutable" => Volatility::Immutable,
                "stable" => Volatility::Stable,
                "volatile" => Volatility::Volatile,
                _ => unreachable!("recognized function volatility"),
            };
            (type_tokens, volatility)
        }
        _ => {
            // PostgreSQL routines are VOLATILE unless explicitly narrowed.
            if tokens.len() > 1 && parse_data_type(&tokens.join(" ")).is_err() {
                if parse_data_type(tokens[0]).is_ok() {
                    return Err(SqlError::new(format!(
                        "unsupported function volatility '{}'; use IMMUTABLE/STABLE/VOLATILE",
                        tokens[1]
                    )));
                }
            }
            (tokens.as_slice(), Volatility::Volatile)
        }
    };
    let data_type = parse_data_type(&type_tokens.join(" "))?;

    Ok((data_type, volatility))
}

fn parse_function_body(raw: &str) -> Result<String, SqlError> {
    let raw = raw.trim();
    let (body, remainder) = if raw.starts_with('$') {
        let delimiter_end = raw[1..]
            .find('$')
            .map(|index| index + 2)
            .ok_or_else(|| SqlError::new("unterminated dollar-quoted function body".into()))?;
        let delimiter = &raw[..delimiter_end];
        let content_start = delimiter_end;
        let content_end = raw[content_start..]
            .find(delimiter)
            .map(|index| content_start + index)
            .ok_or_else(|| SqlError::new("unterminated dollar-quoted function body".into()))?;
        (
            raw[content_start..content_end].to_string(),
            raw[content_end + delimiter.len()..].trim(),
        )
    } else if raw.starts_with('\'') {
        let (body, remainder) = parse_single_quoted_body(raw)?;
        (body, remainder.trim())
    } else {
        let (body, _) = parse_quoted_or_raw_body(raw)?;
        return Ok(normalize_scalar_select_body(body));
    };

    if !remainder.is_empty() {
        if !starts_with_keyword(remainder, "language") {
            return Err(SqlError::new(
                "unexpected clause after function body".into(),
            ));
        }
        let language = remainder[8..].trim();
        if !language.eq_ignore_ascii_case("sql") {
            return Err(SqlError::new(
                "only LANGUAGE sql is supported for functions".into(),
            ));
        }
    }
    if body.trim().is_empty() {
        return Err(SqlError::new("empty function/procedure body".into()));
    }
    Ok(normalize_scalar_select_body(body))
}

fn normalize_scalar_select_body(body: String) -> String {
    let body = body.trim();
    if body
        .get(..6)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("select"))
        && body.as_bytes().get(6).is_some_and(u8::is_ascii_whitespace)
    {
        body[7..].trim_start().to_string()
    } else {
        body.to_string()
    }
}

fn parse_single_quoted_body(raw: &str) -> Result<(String, &str), SqlError> {
    let bytes = raw.as_bytes();
    let mut body = String::new();
    let mut index = 1;
    while index < bytes.len() {
        if bytes[index] == b'\'' {
            if bytes.get(index + 1) == Some(&b'\'') {
                body.push('\'');
                index += 2;
                continue;
            }
            return Ok((body, &raw[index + 1..]));
        }
        let character = raw[index..].chars().next().expect("valid UTF-8 boundary");
        body.push(character);
        index += character.len_utf8();
    }
    Err(SqlError::new(
        "unterminated single-quoted function body".into(),
    ))
}

pub(super) fn parse_quoted_or_raw_body(raw: &str) -> Result<String, SqlError> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err(SqlError::new("empty function/procedure body".into()));
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

    Err(SqlError::new("not a quoted string".into()))
}

pub(super) fn split_keyword<'a>(raw: &'a str, keyword: &'a str) -> Option<(&'a str, &'a str)> {
    let lower = raw.to_ascii_lowercase();
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
