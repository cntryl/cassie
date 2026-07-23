use super::{Cassie, CassieSession, ColumnMeta, QueryError, QueryResult, Value};
use crate::catalog::{
    canonical_schema_name, DEFAULT_SCHEMA, INFORMATION_SCHEMA, PG_CATALOG_SCHEMA,
};

pub(super) fn execute_show(
    session: Option<&CassieSession>,
    statement: &crate::sql::ast::ShowStatement,
) -> Result<QueryResult, QueryError> {
    let variable = statement.variable.trim().to_ascii_lowercase();
    if variable.is_empty() {
        return Err(QueryError::General("SHOW requires a variable".to_string()));
    }

    if variable == "transaction isolation level" {
        Ok(QueryResult {
            columns: vec![ColumnMeta::text("transaction_isolation")],
            rows: vec![vec![Value::String("read committed".to_string())]],
            command: "SHOW".to_string(),
        })
    } else {
        let value = session.map_or_else(
            || default_setting(&variable),
            |session| session.setting(&variable),
        )?;
        Ok(QueryResult {
            columns: vec![ColumnMeta::text(setting_display_name(&variable))],
            rows: vec![vec![Value::String(value)]],
            command: "SHOW".to_string(),
        })
    }
}

pub(super) fn execute_set(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    statement: &crate::sql::ast::SetStatement,
) -> Result<QueryResult, QueryError> {
    let variable = statement.variable.trim().to_ascii_lowercase();
    if variable.is_empty() {
        return Err(QueryError::General("SET requires a variable".to_string()));
    }

    if variable == "search_path" {
        let value = statement.value.as_deref().unwrap_or("").trim();
        let Some(session) = session else {
            return Err(QueryError::General(
                "SET search_path requires a session".to_string(),
            ));
        };
        let path = parse_search_path(value);
        validate_search_path(cassie, session, &path)?;
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
    Ok(QueryResult {
        columns: Vec::new(),
        rows: Vec::new(),
        command: "SET".to_string(),
    })
}

fn default_setting(name: &str) -> Result<String, crate::app::CassieError> {
    CassieSession::new("postgres".to_string(), None).setting(name)
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

fn validate_search_path(
    cassie: &Cassie,
    session: &CassieSession,
    path: &[String],
) -> Result<(), QueryError> {
    let database = session
        .current_database()
        .unwrap_or(cassie.default_database.as_str());
    for schema in path {
        if matches!(
            schema.as_str(),
            DEFAULT_SCHEMA | PG_CATALOG_SCHEMA | INFORMATION_SCHEMA
        ) {
            continue;
        }
        let scoped = canonical_schema_name(database, schema);
        if !cassie.catalog.namespace_exists(&scoped) {
            return Err(QueryError::Cassie(
                crate::app::CassieError::CatalogObjectNotFound {
                    kind: crate::app::CatalogObjectKind::Schema,
                    name: scoped,
                },
            ));
        }
    }
    Ok(())
}
