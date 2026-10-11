use super::{CassieSession, ColumnMeta, QueryError, QueryResult};
use crate::catalog::DEFAULT_SCHEMA;
use crate::runtime::QueryExecutionControls;

/// The single column `SHOW` returns, shared by Describe and execution so the
/// `RowDescription` of a prepared `SHOW` always matches its `DataRow`.
pub(crate) fn show_result_columns(statement: &crate::sql::ast::ShowStatement) -> Vec<ColumnMeta> {
    let variable = statement.variable.trim().to_ascii_lowercase();
    if variable.is_empty() {
        return Vec::new();
    }
    vec![ColumnMeta::text(show_column_name(&variable))]
}

fn show_column_name(variable: &str) -> &str {
    if variable == "transaction isolation level" {
        "transaction_isolation"
    } else {
        setting_display_name(variable)
    }
}

pub(super) fn execute_show(
    session: Option<&CassieSession>,
    statement: &crate::sql::ast::ShowStatement,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    let variable = statement.variable.trim().to_ascii_lowercase();
    if variable.is_empty() {
        return Err(QueryError::General("SHOW requires a variable".to_string()));
    }

    let value = if variable == "transaction isolation level" {
        crate::runtime::accounted::Accounted::try_new(
            controls,
            std::mem::size_of::<String>() + "read committed".len(),
            || "read committed".to_owned(),
        )?
    } else {
        match session {
            Some(session) => session.accounted_setting(&variable, controls)?,
            None => default_setting(&variable, controls)?,
        }
    };
    let mut result = super::command_output::columns(
        "SHOW",
        &[(show_column_name(&variable), crate::types::DataType::Text)],
        controls,
    )?;
    super::command_output::push_row(
        &mut result,
        &[super::command_output::Cell::Text(value.get())],
        controls,
    )?;
    Ok(result.finish()?)
}

pub(super) fn execute_set(
    session: Option<&CassieSession>,
    statement: &crate::sql::ast::SetStatement,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    let variable = statement.variable.trim().to_ascii_lowercase();
    if variable.is_empty() {
        return Err(QueryError::General("SET requires a variable".to_string()));
    }

    let output = super::command_output::empty("SET", controls)?;
    if variable == "search_path" {
        let value = statement.value.as_deref().unwrap_or("").trim();
        let Some(session) = session else {
            return Err(QueryError::General(
                "SET search_path requires a session".to_string(),
            ));
        };
        let path = parse_search_path(value);
        session.set_search_path(path);
    } else {
        let Some(session) = session else {
            return Err(QueryError::General(format!(
                "SET {} requires a session",
                statement.variable
            )));
        };
        session.set_setting(&variable, statement.value.as_deref().unwrap_or(""))?;
    }
    Ok(output)
}

fn default_setting(
    name: &str,
    controls: &QueryExecutionControls,
) -> Result<crate::runtime::accounted::Accounted<String>, crate::app::CassieError> {
    CassieSession::new("root".to_string(), None).accounted_setting(name, controls)
}

fn setting_display_name(name: &str) -> &str {
    match name {
        "datestyle" => "DateStyle",
        "timezone" => "TimeZone",
        _ => name,
    }
}

fn parse_search_path(raw: &str) -> Vec<String> {
    let path = raw
        .split(',')
        .map(|entry| entry.trim().trim_matches('"').to_ascii_lowercase())
        .filter(|entry| !entry.is_empty())
        .collect::<Vec<_>>();
    if path.is_empty() {
        return vec![DEFAULT_SCHEMA.to_string()];
    }
    path
}
