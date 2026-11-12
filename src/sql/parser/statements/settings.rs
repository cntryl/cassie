//! Typed setting operations preserve DEFAULT versus quoted string values.

use crate::sql::ast::{
    Expr, ParsedStatement, QueryStatement, SetStatement, SettingAction, ShowStatement,
};
use crate::sql::parser::{expr::parse_expr_token, lexical, SqlError};

pub(in crate::sql::parser) fn parse_show_statement(sql: &str) -> Result<ParsedStatement, SqlError> {
    let sql = sql.trim().trim_end_matches(';').trim();
    let variable = sql[4..].trim();
    let variable = if variable
        .split_whitespace()
        .map(str::to_ascii_lowercase)
        .eq(["time", "zone"])
    {
        "timezone"
    } else {
        variable
    };
    if variable.is_empty() {
        return Err(SqlError::syntax("SHOW requires a parameter".into()));
    }
    Ok(ParsedStatement {
        raw_sql: sql.to_string(),
        statement: QueryStatement::Show(ShowStatement {
            variable: variable.to_string(),
        }),
    })
}

pub(in crate::sql::parser) fn parse_reset_statement(
    sql: &str,
) -> Result<ParsedStatement, SqlError> {
    let sql = sql.trim().trim_end_matches(';').trim();
    let variable = lexical::trim_separators(&sql[5..]);
    if variable.is_empty() || variable.split_whitespace().count() != 1 {
        return Err(SqlError::syntax(
            "RESET requires one parameter or ALL".into(),
        ));
    }
    let action = if variable.eq_ignore_ascii_case("all") {
        SettingAction::ResetAll
    } else {
        SettingAction::Reset
    };
    Ok(parsed(sql, variable, None, action))
}

pub(in crate::sql::parser) fn parse_set_statement(sql: &str) -> Result<ParsedStatement, SqlError> {
    let sql = sql.trim().trim_end_matches(';').trim();
    let argument = lexical::trim_separators(&sql[3..]);
    if lexical::pattern_end(argument, 0, "ROLE").is_some()
        || lexical::pattern_end(argument, 0, "SESSION AUTHORIZATION").is_some()
    {
        return Err(SqlError::unsupported(
            "SET ROLE and SET SESSION AUTHORIZATION are not supported in this version".into(),
        ));
    }
    if lexical::pattern_end(argument, 0, "LOCAL TRANSACTION").is_some()
        || lexical::pattern_end(argument, 0, "SESSION TRANSACTION").is_some()
    {
        return Err(SqlError::unsupported(
            "unsupported SET TRANSACTION form".into(),
        ));
    }
    let session_end = lexical::pattern_end(argument, 0, "SESSION ");
    let argument = session_end.map_or(argument, |end| &argument[end..]);
    if argument.is_empty() {
        return Err(SqlError::syntax("SET requires a parameter".into()));
    }
    let uncommented_argument = lexical::without_comments(argument);
    let normalized_argument = uncommented_argument.to_ascii_lowercase();
    let mut words = normalized_argument.split_whitespace();
    if words.next() == Some("transaction") {
        return match parse_set_transaction_isolation(sql, uncommented_argument.trim())? {
            Some(statement) => Ok(statement),
            None => Err(SqlError::unsupported(
                "unsupported SET TRANSACTION form".into(),
            )),
        };
    }
    if let Some(end) = lexical::pattern_end(argument, 0, "LOCAL ") {
        return parse_set_local(sql, &argument[end..]);
    }
    if let Some(statement) = parse_set_names(sql, argument)? {
        return Ok(statement);
    }
    if let Some(end) = lexical::pattern_end(argument, 0, "CHARACTERISTICS AS TRANSACTION ") {
        return parse_session_characteristics(sql, &argument[end..], session_end.is_some());
    }
    let (variable, value) = if let Some(end) = lexical::pattern_end(argument, 0, "TIME ZONE ") {
        ("timezone", argument[end..].trim())
    } else if let Some(end) = lexical::pattern_end(argument, 0, "SCHEMA ") {
        let Expr::StringLiteral(schema) = parse_expr_token(argument[end..].trim())? else {
            return Err(SqlError::syntax(
                "SET SCHEMA requires a schema string".into(),
            ));
        };
        // SET SCHEMA is a one-entry search_path, so protect commas and case.
        let schema = format!("\"{}\"", schema.replace('"', "\"\""));
        return Ok(parsed(sql, "search_path", Some(schema), SettingAction::Set));
    } else if let Some((position, end)) = lexical::find_top_level_span(argument, 0, "=") {
        (argument[..position].trim(), argument[end..].trim())
    } else if let Some(position) = crate::sql::parser::find_top_level_keyword(argument, 0, "to") {
        (argument[..position].trim(), argument[position + 2..].trim())
    } else {
        return Err(SqlError::syntax("SET requires TO or = and a value".into()));
    };
    if variable.is_empty() || value.is_empty() {
        return Err(SqlError::syntax("invalid SET statement".into()));
    }
    if variable.eq_ignore_ascii_case("role")
        || variable.eq_ignore_ascii_case("session authorization")
    {
        return Err(SqlError::unsupported(
            "SET ROLE and SET SESSION AUTHORIZATION are not supported in this version".into(),
        ));
    }
    if variable.eq_ignore_ascii_case("transaction_isolation") {
        return Err(SqlError::unsupported(
            "set transaction_isolation assignments are not supported; use SET TRANSACTION ISOLATION LEVEL".into(),
        ));
    }
    if value.eq_ignore_ascii_case("DEFAULT")
        || variable == "timezone" && value.eq_ignore_ascii_case("LOCAL")
    {
        return Ok(parsed(sql, variable, None, SettingAction::SetDefault));
    }
    let value = if value.starts_with('\'') || lexical::dollar_quoted_content(value).is_some() {
        let Expr::StringLiteral(value) = parse_expr_token(value)? else {
            return Err(SqlError::syntax("invalid SET value".into()));
        };
        value
    } else {
        value.to_string()
    };
    Ok(parsed(sql, variable, Some(value), SettingAction::Set))
}

fn parse_session_characteristics(
    sql: &str,
    characteristics: &str,
    session: bool,
) -> Result<ParsedStatement, SqlError> {
    if !session {
        return Err(SqlError::syntax(
            "session transaction characteristics require SET SESSION".into(),
        ));
    }
    let (variable, value) = match characteristics
        .to_ascii_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["read", "only"] => ("default_transaction_read_only", "on"),
        ["read", "write"] => ("default_transaction_read_only", "off"),
        ["deferrable"] => ("default_transaction_deferrable", "on"),
        ["not", "deferrable"] => ("default_transaction_deferrable", "off"),
        _ => {
            return Err(SqlError::unsupported(
                "unsupported session transaction characteristics".into(),
            ))
        }
    };
    Ok(parsed(
        sql,
        variable,
        Some(value.to_string()),
        SettingAction::Set,
    ))
}

fn parse_set_local(sql: &str, argument: &str) -> Result<ParsedStatement, SqlError> {
    let mut statement = parse_set_statement(&format!("SET {argument}"))?;
    if let QueryStatement::Set(set) = &mut statement.statement {
        if set.variable == "transaction_isolation" {
            return Err(SqlError::unsupported(
                "SET LOCAL TRANSACTION ISOLATION is not supported in this version".into(),
            ));
        }
        set.local = true;
    }
    statement.raw_sql = sql.to_string();
    Ok(statement)
}

fn parse_set_transaction_isolation(
    sql: &str,
    argument: &str,
) -> Result<Option<ParsedStatement>, SqlError> {
    let Some(end) = lexical::pattern_end(argument, 0, "TRANSACTION ISOLATION LEVEL") else {
        return Ok(None);
    };
    let isolation = match argument[end..].trim().to_ascii_lowercase().as_str() {
        "read committed" => "read committed",
        "read uncommitted" => "read uncommitted",
        "repeatable read" => "repeatable read",
        "serializable" => "serializable",
        _ => {
            return Err(SqlError::unsupported(
                "unsupported transaction isolation level".into(),
            ));
        }
    };
    Ok(Some(parsed(
        sql,
        "transaction_isolation",
        Some(isolation.to_owned()),
        SettingAction::Set,
    )))
}

fn parse_set_names(sql: &str, argument: &str) -> Result<Option<ParsedStatement>, SqlError> {
    let Some(end) = lexical::pattern_end(argument, 0, "NAMES") else {
        return Ok(None);
    };
    let value = lexical::trim_separators(&argument[end..]);
    if value.starts_with('=') || lexical::pattern_end(value, 0, "TO").is_some() {
        return Ok(None);
    }
    if value.is_empty() || value.eq_ignore_ascii_case("DEFAULT") {
        return Ok(Some(parsed(
            sql,
            "client_encoding",
            None,
            SettingAction::SetDefault,
        )));
    }
    if !value.starts_with('\'') && lexical::dollar_quoted_content(value).is_none() {
        return Err(SqlError::syntax(
            "SET NAMES requires a string encoding name or DEFAULT".into(),
        ));
    }
    let Expr::StringLiteral(value) = parse_expr_token(value)? else {
        return Err(SqlError::syntax(
            "SET NAMES requires a string encoding name or DEFAULT".into(),
        ));
    };
    Ok(Some(parsed(
        sql,
        "client_encoding",
        Some(value),
        SettingAction::Set,
    )))
}

fn parsed(
    sql: &str,
    variable: &str,
    value: Option<String>,
    action: SettingAction,
) -> ParsedStatement {
    ParsedStatement {
        raw_sql: sql.to_string(),
        statement: QueryStatement::Set(SetStatement {
            variable: variable.to_ascii_lowercase(),
            value,
            action,
            local: false,
        }),
    }
}
