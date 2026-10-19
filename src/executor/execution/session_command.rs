use super::{CassieSession, ColumnMeta, QueryError, QueryResult};
use crate::catalog::DEFAULT_SCHEMA;
use crate::runtime::QueryExecutionControls;
use crate::sql::ast::SettingAction;

/// The single column `SHOW` returns, shared by Describe and execution so the
/// `RowDescription` of a prepared `SHOW` always matches its `DataRow`.
pub(crate) fn show_result_columns(statement: &crate::sql::ast::ShowStatement) -> Vec<ColumnMeta> {
    let variable = statement.variable.trim().to_ascii_lowercase();
    if variable.is_empty() {
        return Vec::new();
    }
    if variable == "all" {
        return SHOW_ALL_COLUMNS
            .iter()
            .copied()
            .map(ColumnMeta::text)
            .collect();
    }
    vec![ColumnMeta::text(show_column_name(&variable))]
}

const SHOW_ALL_COLUMNS: [&str; 3] = ["name", "setting", "description"];

/// `SHOW ALL`: every registered setting with its current value, ordered by
/// name as PostgreSQL orders them.
fn execute_show_all(
    session: Option<&CassieSession>,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    let fallback = CassieSession::new("root".to_string(), None);
    let session = session.unwrap_or(&fallback);
    let mut names = crate::app::all_session_settings()
        .iter()
        .map(|spec| spec.name)
        .chain([
            "transaction_deferrable",
            "transaction_isolation",
            "transaction_read_only",
        ])
        .collect::<Vec<_>>();
    names.sort_unstable();
    let mut result = super::command_output::columns(
        "SHOW",
        &SHOW_ALL_COLUMNS.map(|name| (name, crate::types::DataType::Text)),
        controls,
    )?;
    for name in names {
        let value = session.accounted_setting(name, controls)?;
        super::command_output::push_row(
            &mut result,
            &[
                super::command_output::Cell::Text(setting_display_name(name)),
                super::command_output::Cell::Text(value.get()),
                super::command_output::Cell::Text("Cassie compatibility setting"),
            ],
            controls,
        )?;
    }
    Ok(result.finish()?)
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
    if variable == "all" {
        return execute_show_all(session, controls);
    }

    let value = if variable == "transaction isolation level" && session.is_none() {
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

    let command = match statement.action {
        SettingAction::Set | SettingAction::SetDefault => "SET",
        SettingAction::Reset | SettingAction::ResetAll => "RESET",
    };
    // Admit the command response before changing session state.
    let output = super::command_output::empty(command, controls)?;
    let Some(session) = session else {
        return Err(QueryError::General(format!("{command} requires a session")));
    };
    match statement.action {
        SettingAction::ResetAll => session.reset_all_settings(),
        SettingAction::Reset | SettingAction::SetDefault => session.reset_setting(&variable)?,
        SettingAction::Set if variable == "search_path" => {
            let path = parse_search_path(statement.value.as_deref().unwrap_or(""));
            session.set_search_path(path);
        }
        SettingAction::Set if statement.local => {
            session.set_local_setting(&variable, statement.value.as_deref().unwrap_or(""))?;
        }
        SettingAction::Set => {
            session.set_setting(&variable, statement.value.as_deref().unwrap_or(""))?;
        }
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
        "intervalstyle" => "IntervalStyle",
        _ => name,
    }
}

fn parse_search_path(raw: &str) -> Vec<String> {
    let mut quoted = false;
    let mut start = 0;
    let mut path = Vec::new();
    for (position, character) in raw.char_indices() {
        match character {
            '"' => quoted = !quoted,
            ',' if !quoted => {
                push_search_path_entry(&mut path, &raw[start..position]);
                start = position + 1;
            }
            _ => {}
        }
    }
    push_search_path_entry(&mut path, &raw[start..]);
    if path.is_empty() {
        return vec![DEFAULT_SCHEMA.to_string()];
    }
    path
}

fn push_search_path_entry(path: &mut Vec<String>, entry: &str) {
    let entry = entry.trim();
    if entry.is_empty() {
        return;
    }
    path.push(
        entry
            .strip_prefix('"')
            .and_then(|name| name.strip_suffix('"'))
            .map_or_else(
                || entry.to_ascii_lowercase(),
                |name| name.replace("\"\"", "\""),
            ),
    );
}
