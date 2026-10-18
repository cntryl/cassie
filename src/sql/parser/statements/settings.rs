//! Typed setting operations preserve DEFAULT versus quoted string values.

use crate::sql::ast::{
    Expr, ParsedStatement, QueryStatement, SetStatement, SettingAction, ShowStatement,
};
use crate::sql::parser::{expr::parse_expr_token, lexical, SqlError};

pub(in crate::sql::parser) fn parse_show_statement(sql: &str) -> Result<ParsedStatement, SqlError> {
    let sql = sql.trim().trim_end_matches(';').trim();
    let variable = sql[4..].trim();
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
    let session_end = lexical::pattern_end(argument, 0, "SESSION ");
    let argument = session_end.map_or(argument, |end| &argument[end..]);
    if argument.is_empty() {
        return Err(SqlError::syntax("SET requires a parameter".into()));
    }
    if lexical::pattern_end(argument, 0, "LOCAL ").is_some() {
        return Err(SqlError::unsupported(
            "SET LOCAL is not supported in this version".into(),
        ));
    }
    if let Some(statement) = parse_set_names(sql, argument)? {
        return Ok(statement);
    }
    if let Some(end) = lexical::pattern_end(argument, 0, "CHARACTERISTICS AS TRANSACTION ") {
        if session_end.is_none() {
            return Err(SqlError::syntax(
                "session transaction characteristics require SET SESSION".into(),
            ));
        }
        let (variable, value) = match argument[end..]
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
        return Ok(parsed(
            sql,
            variable,
            Some(value.to_string()),
            SettingAction::Set,
        ));
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
        }),
    }
}
