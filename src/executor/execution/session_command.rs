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

    match variable.as_str() {
        "search_path" => Ok(QueryResult {
            columns: vec![ColumnMeta::text("search_path")],
            rows: vec![vec![Value::String(
                session.map_or_else(
                    || DEFAULT_SCHEMA.to_string(),
                    |session| session.search_path().join(", "),
                ),
            )]],
            command: "SHOW".to_string(),
        }),
        "server_version" => Ok(QueryResult {
            columns: vec![ColumnMeta::text("server_version")],
            rows: vec![vec![Value::String(env!("CARGO_PKG_VERSION").to_string())]],
            command: "SHOW".to_string(),
        }),
        "transaction isolation level" => Ok(QueryResult {
            columns: vec![ColumnMeta::text("transaction_isolation")],
            rows: vec![vec![Value::String("read committed".to_string())]],
            command: "SHOW".to_string(),
        }),
        "standard_conforming_strings" => Ok(QueryResult {
            columns: vec![ColumnMeta::text("standard_conforming_strings")],
            rows: vec![vec![Value::String("on".to_string())]],
            command: "SHOW".to_string(),
        }),
        _ => Err(QueryError::General(format!(
            "unsupported SHOW variable '{}'",
            statement.variable
        ))),
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

    match variable.as_str() {
        "search_path" => {
            let value = statement.value.as_deref().unwrap_or("").trim();
            let Some(session) = session else {
                return Err(QueryError::General(
                    "SET search_path requires a session".to_string(),
                ));
            };
            let path = parse_search_path(value)?;
            validate_search_path(cassie, session, &path)?;
            session.set_search_path(path);
            Ok(QueryResult {
                columns: Vec::new(),
                rows: Vec::new(),
                command: "SET".to_string(),
            })
        }
        _ => Err(QueryError::General(format!(
            "unsupported SET variable '{}', supported variables: search_path",
            statement.variable
        ))),
    }
}

fn parse_search_path(raw: &str) -> Result<Vec<String>, QueryError> {
    let path = raw
        .split(',')
        .map(|entry| entry.trim().trim_matches('"').to_ascii_lowercase())
        .filter(|entry| !entry.is_empty())
        .collect::<Vec<_>>();
    if path.is_empty() {
        return Ok(vec![DEFAULT_SCHEMA.to_string()]);
    }
    Ok(path)
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
            return Err(QueryError::General(format!(
                "schema '{schema}' does not exist in database '{database}'"
            )));
        }
    }
    Ok(())
}
